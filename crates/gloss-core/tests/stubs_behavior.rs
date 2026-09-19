//! 桩库自身的验收：预置返回、失败注入、调用计数逐条对账。

#![allow(clippy::expect_used, clippy::panic)] // 桩自测沿生产桩的断言风格，失败即停

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;

use stubs::engine::MockEngine;
use stubs::ports::{
    FixedRegionCapture, FixedSelectionReader, MemoryCache, MemoryConfigStore, RecordingHotkeyBinder,
};

use gloss_core::config::Config;
use gloss_core::model::GlossError;
use gloss_core::ports::{
    AiEngine, Cache, ConfigStore, EngineRequest, HotkeyBinder, RegionCapture, SelectionReader,
};
use gloss_core::prompt::{ChatMessage, Role};
use gloss_core::task::{HotkeyBinding, InputSource, OutcomeStructured, TaskKind, TaskOutcome};

mod stubs;

fn sample_request() -> EngineRequest {
    EngineRequest {
        kind: TaskKind::TranslateWord,
        messages: vec![ChatMessage {
            role: Role::User,
            content: "gloss".into(),
        }],
        model: "mock-model".into(),
    }
}

fn outcome(body: &str) -> TaskOutcome {
    TaskOutcome {
        kind: TaskKind::TranslateWord,
        body: body.into(),
        structured: OutcomeStructured::Plain { title: None },
    }
}

#[tokio::test]
async fn chunk_delay_paces_the_stream() {
    let engine = MockEngine::new()
        .with_chunk_delay(Duration::from_millis(30))
        .with_chunks(vec![Ok("光".into()), Ok("泽".into()), Ok("注释".into())]);
    let mut stream = engine.execute(&sample_request()).await.expect("stream");

    let start = Instant::now();
    let mut seen = Vec::new();
    while let Some(chunk) = stream.next().await {
        seen.push(chunk.expect("chunk ok"));
    }
    assert_eq!(seen, vec!["光", "泽", "注释"]);
    assert!(
        start.elapsed() >= Duration::from_millis(60),
        "two inter-chunk delays must pace the stream, got {:?}",
        start.elapsed()
    );
}

#[tokio::test]
async fn failures_are_injectable() {
    for error in [
        GlossError::EngineNetwork,
        GlossError::EngineAuth,
        GlossError::EngineRateLimited,
        GlossError::EngineResponse("bad json".into()),
    ] {
        let engine = MockEngine::new().with_execute_failure(error.clone());
        match engine.execute(&sample_request()).await {
            Err(actual) => assert_eq!(actual, error, "failure must surface verbatim"),
            Ok(_) => panic!("expected {error:?}, got a stream"),
        }
    }

    let mid_stream = MockEngine::new().with_chunks(vec![
        Ok("部分".into()),
        Err(GlossError::EngineNetwork),
        Ok("流继续".into()),
    ]);
    let mut stream = mid_stream.execute(&sample_request()).await.expect("stream");
    assert_eq!(stream.next().await, Some(Ok("部分".into())));
    assert_eq!(
        stream.next().await,
        Some(Err(GlossError::EngineNetwork)),
        "mid-stream failure must surface in order"
    );
    assert_eq!(stream.next().await, Some(Ok("流继续".into())));
    assert!(stream.next().await.is_none(), "stream ends at script end");
}

#[tokio::test]
async fn execute_failure_once_fails_exactly_once() {
    let engine = MockEngine::new()
        .with_execute_failure_once(GlossError::EngineRateLimited)
        .with_chunks(vec![Ok("恢复".into())]);

    let first = engine.execute(&sample_request()).await;
    assert_eq!(
        first.err(),
        Some(GlossError::EngineRateLimited),
        "the queued failure must fail exactly the first call"
    );

    let second = engine.execute(&sample_request()).await;
    assert!(second.is_ok(), "the next call must stream again");
    if let Ok(mut stream) = second {
        assert_eq!(stream.next().await, Some(Ok("恢复".into())));
        assert!(stream.next().await.is_none());
    }
}

#[tokio::test]
async fn execute_panic_fires_on_first_poll() {
    let engine = MockEngine::new().with_execute_panic();
    let joined = tokio::spawn(engine.execute(&sample_request()));
    match joined.await {
        Err(join_error) => assert!(
            join_error.is_panic(),
            "the injected panic must surface as a task panic: {join_error:?}"
        ),
        Ok(_) => panic!("a panicked task future must not resolve to a stream"),
    }
}

#[tokio::test]
async fn call_count_tracks_execute_invocations() {
    let engine = MockEngine::new().with_chunks(vec![Ok("x".into())]);
    assert_eq!(engine.call_count(), 0);
    let mut stream = engine.execute(&sample_request()).await.expect("stream");
    while let Some(chunk) = stream.next().await {
        chunk.expect("chunk ok");
    }
    assert_eq!(engine.call_count(), 1);

    let cloned = engine.clone();
    let _ = cloned.execute(&sample_request()).await.expect("stream");
    assert_eq!(engine.call_count(), 2);
}

#[tokio::test]
async fn empty_script_yields_empty_stream() {
    let engine = MockEngine::new();
    let mut stream = engine.execute(&sample_request()).await.expect("stream");
    assert!(stream.next().await.is_none());
}

#[test]
fn selection_reader_mock_returns_presets() {
    let mut ok = FixedSelectionReader(Ok("selected".into()));
    assert_eq!(ok.read(), Ok("selected".into()));

    let mut denied = FixedSelectionReader(Err(GlossError::AccessibilityDenied));
    assert_eq!(denied.read(), Err(GlossError::AccessibilityDenied));
}

#[test]
fn region_capture_mock_returns_png() {
    let png: Arc<[u8]> = vec![1, 2, 3].into();
    let mut capture = FixedRegionCapture(Ok(Arc::clone(&png)));
    let got = capture
        .capture(gloss_core::model::ScreenRect {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        })
        .expect("capture should succeed");
    assert!(Arc::ptr_eq(&png, &got));
}

#[test]
fn config_store_mock_round_trips_secrets() {
    let store = MemoryConfigStore::default();
    assert_eq!(store.secret("api_key"), Ok(None));
    store
        .set_secret("api_key", "sk-test")
        .expect("set should succeed");
    assert_eq!(store.secret("api_key"), Ok(Some("sk-test".into())));
}

#[test]
fn config_store_mock_round_trips_document() {
    let store = MemoryConfigStore::default();
    assert_eq!(store.load(), Ok(Config::default()));

    let config = Config {
        auto_show: false,
        ..Default::default()
    };
    store.save(&config).expect("save should succeed");
    assert_eq!(store.load(), Ok(config));
}

#[test]
fn cache_mock_stores_and_isolates_keys() {
    let cache = MemoryCache::default();
    assert!(cache.get(1).is_none());
    cache.set(1, outcome("cached"));
    assert_eq!(cache.get(1).map(|o| o.body), Some("cached".into()));
    assert!(cache.get(2).is_none(), "unrelated key must not see entry");
}

#[test]
fn hotkey_binder_mock_records_every_call() {
    let binder = RecordingHotkeyBinder::default();
    assert_eq!(binder.call_count(), 0);
    assert!(binder.last().is_none(), "no call means nothing to report");

    let first = vec![HotkeyBinding {
        trigger: "Cmd+Shift+D".into(),
        kind: TaskKind::TranslateWord,
        source: InputSource::Selection,
    }];
    assert_eq!(binder.rebind(&first), 1, "applied count mirrors the input");

    let second = vec![HotkeyBinding {
        trigger: "Cmd+Shift+E".into(),
        kind: TaskKind::ExplainCode,
        source: InputSource::Selection,
    }];
    binder.rebind(&second);
    assert_eq!(binder.call_count(), 2);
    assert_eq!(
        binder.last(),
        Some(second),
        "the last call must win, not the first"
    );
    assert_eq!(binder.rebind(&[]), 0, "an empty table applies nothing");
}
