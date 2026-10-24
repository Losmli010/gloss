//! L1 库级集成测试：状态机 + 通道③④ + tokio 消费桥 +
//! 脚本引擎桩 + 任务缓存的全链路时序。

use std::sync::Arc;
use std::time::Duration;

use gloss_app::channel::{AcquireCommand, Channels, Command, Event, PlatformEvent, Traced};
use gloss_app::machine::{
    AppState, ErrorAction, FailureOutcome, InputOutcome, OverlayView, TaskStateMachine,
};
use gloss_app::runtime::cache::TaskCache;
use gloss_app::runtime::pipeline::{TaskBridge, start_command_runtime};
use gloss_core::config::Config;
use gloss_core::config_handle::ConfigHandle;
use gloss_core::engine::AiTaskService;
use gloss_core::guard::SceneFacts;
use gloss_core::log::Span;
use gloss_core::model::Locale;
use gloss_core::model::ScreenPoint;
use gloss_core::model::{GlossError, Lang};
use gloss_core::ports::{AiEngine, TaskChunk};
use gloss_core::task::{OutcomeStructured, TaskInput, TaskKind, TaskOutcome};

mod stubs;
use stubs::engine::MockEngine;
use stubs::ports::MemoryConfigStore;

#[allow(clippy::expect_used, clippy::panic)]
fn pipeline(engine: &MockEngine) -> Pipeline {
    let service = Arc::new(AiTaskService::new(
        Arc::new(engine.clone()) as Arc<dyn AiEngine>
    ));
    let Channels {
        platform_events,
        acquire_commands,
        commands,
        events,
    } = Channels::new();
    let gloss_app::channel::CrossbeamPair { tx: pe_tx, rx: _ } = platform_events;
    let gloss_app::channel::CrossbeamPair { tx: ac_tx, rx: _ } = acquire_commands;
    let gloss_app::channel::CrossbeamPair {
        tx: ev_tx,
        rx: ev_rx,
    } = events;
    let gloss_app::channel::CommandChannel {
        tx: cmd_tx,
        rx: cmd_rx,
    } = commands;
    let config = Arc::new(ConfigHandle::with_config(
        Arc::new(MemoryConfigStore::default()),
        Config::default(),
    ));
    let runtime = start_command_runtime(
        TaskBridge::new(
            service,
            Arc::new(TaskCache::new()),
            Arc::new(MemoryConfigStore::default()) as Arc<dyn gloss_core::ports::ConfigStore>,
            ev_tx,
            || {},
        ),
        cmd_rx,
    )
    .expect("tokio bridge should start");
    Pipeline {
        machine: TaskStateMachine::new(),
        config,
        _pe_tx: pe_tx,
        _ac_tx: ac_tx,
        commands_tx: cmd_tx,
        events_rx: ev_rx,
        _runtime: runtime,
    }
}

struct Pipeline {
    machine: TaskStateMachine,
    config: Arc<ConfigHandle>,
    _pe_tx: crossbeam_channel::Sender<PlatformEvent>,
    _ac_tx: crossbeam_channel::Sender<Traced<AcquireCommand>>,
    commands_tx: tokio::sync::mpsc::UnboundedSender<Traced<Command>>,
    events_rx: crossbeam_channel::Receiver<Event>,
    _runtime: gloss_app::runtime::pipeline::CommandRuntime,
}

impl Pipeline {
    #[allow(clippy::expect_used, clippy::panic)]
    fn trigger_and_feed(&mut self, text: &str, span: Span) -> tokio_util::sync::CancellationToken {
        let command = self
            .machine
            .begin_selection_probe(
                &PlatformEvent::SelectionGesture {
                    pos: ScreenPoint::new(0, 0),
                },
                &self.config.snapshot(),
                Locale::Zh,
                &SceneFacts::default(),
            )
            .expect("selection gesture must probe");
        let AcquireCommand::AcquireText { generation } = &command else {
            panic!("acquire text expected");
        };
        let InputOutcome::Dispatch(request) = self
            .machine
            .commit_probe(*generation, TaskInput::Text { text: text.into() })
        else {
            panic!("the probe product should be committed");
        };
        self.commands_tx
            .send(Traced {
                payload: Command::RunTask {
                    generation: request.generation,
                    input: request.input,
                    options: request.options,
                    cancel: request.cancel.clone(),
                },
                span,
            })
            .expect("command channel open");
        request.cancel
    }

    #[allow(clippy::expect_used, clippy::panic)]
    fn trigger_image_feed(
        &mut self,
        png: Arc<[u8]>,
        span: Span,
    ) -> tokio_util::sync::CancellationToken {
        let command = self
            .machine
            .begin_pasteboard_probe(
                &PlatformEvent::PasteboardImageObserved,
                &self.config.snapshot(),
                Locale::Zh,
                &SceneFacts::default(),
            )
            .expect("pasteboard observation must probe");
        let AcquireCommand::AcquireClipboardImage { generation } = &command else {
            panic!("acquire clipboard image expected");
        };
        let InputOutcome::Dispatch(request) = self
            .machine
            .commit_probe(*generation, TaskInput::Image { png })
        else {
            panic!("the pasteboard product should be committed");
        };
        self.commands_tx
            .send(Traced {
                payload: Command::RunTask {
                    generation: request.generation,
                    input: request.input,
                    options: request.options,
                    cancel: request.cancel.clone(),
                },
                span,
            })
            .expect("command channel open");
        request.cancel
    }
}

#[test]
fn engine_logs_carry_the_task_span() {
    let logs = gloss_core::log::capture_global();
    let engine = MockEngine::new().with_chunks(vec![Ok("{\"note\":\"产物\"}".into())]);
    let mut pipe = pipeline(&engine);

    pipe.trigger_and_feed("hello", Span::none());
    wait_done(&mut pipe);
    pipe.trigger_and_feed("hello", gloss_core::log::task_span(2));
    wait_done(&mut pipe);

    assert!(
        logs.text()
            .lines()
            .any(|line| line.contains("cache hit") && line.contains("\"generation\":2")),
        "{}",
        logs.text()
    );
}

#[test]
fn full_flow_classifies_then_streams_and_settles() {
    let engine =
        MockEngine::new().with_chunks(vec![Ok("{\"note\":\"光泽".into()), Ok("：注释\"} ".into())]);
    let mut pipe = pipeline(&engine);

    let token = pipe.trigger_and_feed("gloss", Span::none());
    assert_eq!(pipe.machine.state(), AppState::Translating);
    assert!(!token.is_cancelled());

    assert_eq!(
        expect_classified(&mut pipe),
        TaskKind::General,
        "the classify call cannot parse a kind from the task script, the fallback applies"
    );

    for _ in 0..2 {
        let Event::TaskChunk { generation, delta } = pipe.events_rx.recv().unwrap() else {
            panic!("chunk expected");
        };
        assert!(pipe.machine.accept_chunk(generation, delta));
    }
    let Event::TaskDone {
        generation,
        outcome,
    } = pipe.events_rx.recv().unwrap()
    else {
        panic!("task done expected");
    };
    assert!(pipe.machine.accept_done(generation, outcome));

    assert_eq!(pipe.machine.state(), AppState::Show);
    match pipe.machine.overlay_view() {
        Some(OverlayView::Outcome { outcome, .. }) => {
            assert_eq!(
                outcome.note, "光泽：注释",
                "note comes from the JSON contract"
            );
            assert!(!outcome.degraded);
            assert_eq!(
                outcome.structured,
                OutcomeStructured::Plain {
                    examples: Vec::new(),
                },
                "the fallback kind lands on the plain variant with empty subprov"
            );
        }
        other => panic!("expected outcome view, got {other:?}"),
    }
}

#[test]
fn classify_failure_falls_back_and_the_task_still_completes() {
    let logs = gloss_core::log::capture_global();
    let engine = MockEngine::new()
        .with_execute_failure_once(GlossError::EngineRateLimited)
        .with_chunks(vec![Ok("{\"note\":\"兜底产物\"}".into())]);
    let mut pipe = pipeline(&engine);

    pipe.trigger_and_feed("第一次划词", Span::none());
    assert_eq!(
        expect_classified(&mut pipe),
        TaskKind::General,
        "the failed classification must fall back to the constant default kind"
    );
    wait_done(&mut pipe);
    assert_eq!(outcome_note(&pipe.machine), "兜底产物");
    assert_eq!(pipe.machine.state(), AppState::Show);

    let text = logs.text();
    let fallback_line = text
        .lines()
        .find(|line| line.contains("classify request failed"))
        .expect("the fallback must leave a trace");
    assert!(
        !fallback_line.contains("第一次划词"),
        "the fallback warn must not carry the selection content: {fallback_line}"
    );
}

#[test]
fn classify_reads_the_kind_from_a_bare_kind_script() {
    let engine = MockEngine::new().with_script_queue(vec![
        vec![Ok(TaskChunk::Delta("{\"kind\":\"ExplainCode\"}".into()))],
        vec![Ok(TaskChunk::Delta(
            "{\"note\":\"讲解\",\"structured\":{\"examples\":[]}}".into(),
        ))],
    ]);
    let mut pipe = pipeline(&engine);

    pipe.trigger_and_feed("fn main() {}", Span::none());
    assert_eq!(
        expect_classified(&mut pipe),
        TaskKind::ExplainCode,
        "the classify call parses the kind tag from the bare JSON script"
    );
    wait_done(&mut pipe);
    assert_eq!(engine.call_count(), 2, "classify + task execution");
}

#[test]
fn second_trigger_is_a_full_cache_hit_without_engine_calls() {
    let engine = MockEngine::new().with_chunks(vec![Ok("{\"note\":\"产物\"}".into())]);
    let mut pipe = pipeline(&engine);

    pipe.trigger_and_feed("同一段文本", Span::none());
    expect_classified(&mut pipe);
    wait_done(&mut pipe);
    assert_eq!(
        engine.call_count(),
        3,
        "first run: classify repairs once (2 calls) + one task execution"
    );

    pipe.trigger_and_feed("同一段文本", Span::none());
    assert_eq!(
        expect_classified(&mut pipe),
        TaskKind::General,
        "the cache replays the classified kind of the settled outcome"
    );
    match pipe.events_rx.recv().unwrap() {
        Event::TaskDone {
            generation,
            outcome,
        } => {
            assert_eq!(generation, 2);
            assert_eq!(outcome.note, "产物");
            assert!(pipe.machine.accept_done(generation, outcome));
        }
        other => panic!("cache hit must settle directly without chunks, got {other:?}"),
    }
    assert_eq!(
        engine.call_count(),
        3,
        "the product cache must not reach the engine again"
    );
    assert_eq!(pipe.machine.state(), AppState::Show);
}

#[test]
fn violating_replies_repair_then_degrade_without_cache() {
    let violating = "MARKER-违约正文，整段不是 JSON";
    let script = vec![Ok(TaskChunk::Delta(violating.into()))];
    let mut scripts = vec![vec![Ok(TaskChunk::Delta(
        "{\"kind\":\"ExplainCode\"}".into(),
    ))]];
    for _ in 0..=3 {
        scripts.push(script.clone());
    }
    let engine = MockEngine::new().with_script_queue(scripts);
    let mut pipe = pipeline(&engine);

    pipe.trigger_and_feed("gloss", Span::none());
    assert_eq!(
        expect_classified(&mut pipe),
        TaskKind::ExplainCode,
        "the classify call reads the kind tag from the bare JSON script"
    );

    pump_repair_rounds(&mut pipe, violating);
    assert_degraded_outcome(&pipe);
    assert!(
        pipe.machine.retry().is_some(),
        "the degraded card keeps the request for the retry exit"
    );

    pipe.trigger_and_feed("gloss", Span::none());
    expect_classified(&mut pipe);
    wait_done(&mut pipe);
    assert_eq!(
        engine.call_count(),
        11,
        "the degraded outcome must not poison the cache: the rerun reaches the engine again \
         (5 calls with the kind script; 6 on the rerun whose queue replays the violating reply)"
    );
}

#[allow(clippy::unwrap_used, clippy::panic)]
fn pump_repair_rounds(pipe: &mut Pipeline, violating: &str) {
    let mut repairs = 0u32;
    loop {
        match pipe.events_rx.recv().unwrap() {
            Event::TaskChunk { generation, delta } => {
                assert_eq!(delta, violating, "every round replays the violating reply");
                assert!(pipe.machine.accept_chunk(generation, delta));
            }
            Event::TaskRepairing { generation } => {
                repairs += 1;
                assert!(
                    pipe.machine.accept_repairing(generation),
                    "each repair marker clears the streaming view for the same generation"
                );
                assert!(
                    matches!(
                        pipe.machine.overlay_view(),
                        Some(OverlayView::Streaming { raw, repairing: true, .. }) if raw.is_empty()
                    ),
                    "the violated accumulation is cleared when the repair round starts"
                );
            }
            Event::TaskDone {
                generation,
                outcome,
            } => {
                assert_eq!(generation, 1);
                assert_eq!(
                    repairs, 3,
                    "the event sequence carries one repairing marker per repair round"
                );
                assert!(pipe.machine.accept_done(generation, outcome));
                break;
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
}

#[allow(clippy::panic)]
fn assert_degraded_outcome(pipe: &Pipeline) {
    assert_eq!(pipe.machine.state(), AppState::Show);
    match pipe.machine.overlay_view() {
        Some(OverlayView::Outcome { outcome, .. }) => {
            assert!(
                outcome.degraded,
                "exhausted repairs hand the violating reply to the completion parser"
            );
            assert_eq!(
                outcome.note, "",
                "the degraded card carries no reply content"
            );
        }
        other => panic!("expected outcome view, got {other:?}"),
    }
}

#[test]
fn an_image_task_repairs_through_the_same_stream() {
    let engine = MockEngine::new().with_script_queue(vec![
        vec![Ok(TaskChunk::Delta("MARKER-违约图注".into()))],
        vec![Ok(TaskChunk::Delta(
            "{\"note\":\"图注\",\"structured\":{\"examples\":[]}}".into(),
        ))],
    ]);
    let mut pipe = pipeline(&engine);

    let png: Arc<[u8]> = Arc::from(&b"png-bytes"[..]);
    pipe.trigger_image_feed(Arc::clone(&png), Span::none());
    assert_eq!(
        expect_classified(&mut pipe),
        TaskKind::ImageExplain,
        "the image task keeps its constant kind across the repair"
    );

    let mut repairs = 0u32;
    loop {
        match pipe.events_rx.recv().unwrap() {
            Event::TaskChunk { generation, delta } => {
                assert!(pipe.machine.accept_chunk(generation, delta));
            }
            Event::TaskRepairing { generation } => {
                repairs += 1;
                assert!(pipe.machine.accept_repairing(generation));
            }
            Event::TaskDone {
                generation,
                outcome,
            } => {
                assert_eq!(generation, 1);
                assert_eq!(repairs, 1);
                assert!(pipe.machine.accept_done(generation, outcome));
                break;
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
    assert_eq!(
        engine.call_count(),
        2,
        "one violated round + one repair round"
    );
    assert_eq!(pipe.machine.state(), AppState::Show);
    match pipe.machine.overlay_view() {
        Some(OverlayView::Outcome { outcome, .. }) => {
            assert_eq!(outcome.note, "图注");
            assert!(!outcome.degraded);
        }
        other => panic!("expected outcome view, got {other:?}"),
    }
}

#[test]
fn image_flow_skips_classification_and_settles_the_plain_examples() {
    let engine = MockEngine::new().with_chunks(vec![
        Ok("{\"note\":\"图表描述\",".into()),
        Ok("\"structured\":{\"examples\":[\"要点一\",\"要点二\"]}}".into()),
    ]);
    let mut pipe = pipeline(&engine);

    let png: Arc<[u8]> = Arc::from(&b"png-bytes"[..]);
    let token = pipe.trigger_image_feed(Arc::clone(&png), Span::none());
    assert_eq!(pipe.machine.state(), AppState::Translating);
    assert!(!token.is_cancelled());
    assert!(
        pipe.machine.attached_image().is_some(),
        "the commit retains the image for the settled card"
    );

    assert_eq!(
        expect_classified(&mut pipe),
        TaskKind::ImageExplain,
        "the image task's kind is a constant mapping, not a classify call"
    );
    assert_eq!(pipe.machine.generation(), 1);

    for expected in [
        "{\"note\":\"图表描述\",",
        "\"structured\":{\"examples\":[\"要点一\",\"要点二\"]}}",
    ] {
        let Event::TaskChunk { generation, delta } = pipe.events_rx.recv().unwrap() else {
            panic!("chunk expected");
        };
        assert_eq!(delta, expected);
        assert!(pipe.machine.accept_chunk(generation, delta));
    }
    wait_done(&mut pipe);

    assert_eq!(
        engine.call_count(),
        1,
        "the image task skips the classify roundtrip entirely"
    );
    assert_plain_image_outcome(&pipe, &png);
}

#[allow(clippy::panic)]
fn assert_plain_image_outcome(pipe: &Pipeline, png: &Arc<[u8]>) {
    assert_eq!(pipe.machine.state(), AppState::Show);
    match pipe.machine.overlay_view() {
        Some(OverlayView::Outcome { outcome, .. }) => {
            assert_eq!(outcome.kind, TaskKind::ImageExplain);
            assert_eq!(outcome.note, "图表描述");
            assert!(!outcome.degraded);
            assert_eq!(
                outcome.structured,
                OutcomeStructured::Plain {
                    examples: vec!["要点一".into(), "要点二".into()],
                },
                "the examples array lands in the 疏 slot via the plain variant"
            );
        }
        other => panic!("expected outcome view, got {other:?}"),
    }
    assert!(
        pipe.machine
            .attached_image()
            .is_some_and(|attached| Arc::ptr_eq(attached, png)),
        "the settled outcome card keeps the retained image"
    );
}

#[test]
fn repeated_image_copy_is_a_full_cache_hit() {
    let engine = MockEngine::new().with_chunks(vec![Ok(
        "{\"note\":\"图注\",\"structured\":{\"examples\":[]}}".into(),
    )]);
    let mut pipe = pipeline(&engine);

    let png: Arc<[u8]> = Arc::from(&b"same-png-bytes"[..]);
    pipe.trigger_image_feed(Arc::clone(&png), Span::none());
    wait_done(&mut pipe);
    assert_eq!(engine.call_count(), 1);

    pipe.trigger_image_feed(Arc::clone(&png), Span::none());
    assert_eq!(
        expect_classified(&mut pipe),
        TaskKind::ImageExplain,
        "the cache replays the settled kind of the image task"
    );
    match pipe.events_rx.recv().unwrap() {
        Event::TaskDone {
            generation,
            outcome,
        } => {
            assert_eq!(generation, 2);
            assert_eq!(outcome.note, "图注");
            assert!(pipe.machine.accept_done(generation, outcome));
        }
        other => panic!("cache hit must settle directly without chunks, got {other:?}"),
    }
    assert_eq!(
        engine.call_count(),
        1,
        "the second copy of the same image must not reach the engine"
    );
    assert_eq!(pipe.machine.state(), AppState::Show);
}

#[test]
fn hide_overlay_cancels_the_image_stream_and_drops_late_events() {
    let engine = MockEngine::new()
        .with_chunk_delay(Duration::from_millis(150))
        .with_chunks(vec![
            Ok("{\"note\":\"一".into()),
            Ok("\",\"structured\":{\"examples\":[]}}".into()),
        ]);
    let mut pipe = pipeline(&engine);

    let png: Arc<[u8]> = Arc::from(&b"png-bytes"[..]);
    let token = pipe.trigger_image_feed(Arc::clone(&png), Span::none());
    assert!(matches!(
        pipe.events_rx.recv().unwrap(),
        Event::TaskClassified {
            generation: 1,
            kind: TaskKind::ImageExplain
        }
    ));
    let Event::TaskChunk { generation, delta } = pipe.events_rx.recv().unwrap() else {
        panic!("chunk expected");
    };
    assert!(pipe.machine.accept_chunk(generation, delta));

    pipe.machine.hide_overlay();
    assert!(
        token.is_cancelled(),
        "hide must cancel the in-flight image task"
    );
    assert!(
        pipe.machine.attached_image().is_none(),
        "hide clears the attached image"
    );
    assert!(
        pipe.events_rx
            .recv_timeout(Duration::from_millis(400))
            .is_err(),
        "cancelled task must not deliver any further event"
    );
    assert!(
        !pipe.machine.accept_chunk(generation, "迟到的正文".into()),
        "products of the cancelled image task must be dropped by the machine"
    );
    assert_eq!(pipe.machine.state(), AppState::Idle);
}

#[test]
fn hide_overlay_cancels_the_stream_and_late_events_are_dropped() {
    let engine = MockEngine::new()
        .with_chunk_delay(Duration::from_millis(150))
        .with_chunks(vec![Ok("一".into()), Ok("二".into()), Ok("三".into())]);
    let mut pipe = pipeline(&engine);

    let token = pipe.trigger_and_feed("慢慢来", Span::none());
    expect_classified(&mut pipe);
    let Event::TaskChunk { generation, delta } = pipe.events_rx.recv().unwrap() else {
        panic!("chunk expected");
    };
    assert!(pipe.machine.accept_chunk(generation, delta));

    pipe.machine.hide_overlay();
    assert!(token.is_cancelled(), "hide must cancel the in-flight token");
    assert!(
        pipe.events_rx
            .recv_timeout(Duration::from_millis(400))
            .is_err(),
        "cancelled task must not deliver any further event"
    );
    let late_outcome = TaskOutcome {
        kind: TaskKind::TranslateWord,
        note: "迟到的产物".into(),
        structured: OutcomeStructured::Plain {
            examples: Vec::new(),
        },
        degraded: false,
    };
    assert!(
        !pipe.machine.accept_chunk(generation, "迟到的正文".into())
            && !pipe.machine.accept_done(generation, late_outcome),
        "products of the cancelled task must be dropped by the machine"
    );
    assert_eq!(pipe.machine.state(), AppState::Idle);
}

#[test]
fn superseded_trigger_cancels_and_filters_late_events() {
    let engine = MockEngine::new()
        .with_chunk_delay(Duration::from_millis(120))
        .with_chunks(vec![Ok("A1".into()), Ok("A2".into())]);
    let mut pipe = pipeline(&engine);

    let token_a = pipe.trigger_and_feed("A 的原文", Span::none());
    let gen_a = pipe.machine.generation();

    let token_b = pipe.trigger_and_feed("B 的原文", Span::none());
    let gen_b = pipe.machine.generation();
    assert!(token_a.is_cancelled(), "new trigger must cancel task A");
    assert!(!token_b.is_cancelled());
    assert_eq!(gen_b, gen_a + 1);

    assert!(
        !pipe.machine.accept_chunk(gen_a, "A 的迟到 chunk".into()),
        "stale generation must be dropped by the machine"
    );

    wait_classified_of_generation(&mut pipe, gen_b);
    for expected in ["A1", "A2"] {
        let Event::TaskChunk { generation, delta } = pipe.events_rx.recv().unwrap() else {
            panic!("chunk expected");
        };
        assert_eq!(generation, gen_b);
        assert_eq!(delta, expected);
        assert!(pipe.machine.accept_chunk(generation, delta));
    }
    let Event::TaskRepairing { generation } = pipe.events_rx.recv().unwrap() else {
        panic!("repair marker expected");
    };
    assert_eq!(generation, gen_b);
    assert!(pipe.machine.accept_repairing(generation));
    wait_degraded_done(&mut pipe, gen_b);
    assert_eq!(pipe.machine.state(), AppState::Show);
}

#[allow(clippy::unwrap_used, clippy::panic)]
fn wait_classified_of_generation(pipe: &mut Pipeline, current: u64) {
    loop {
        match pipe.events_rx.recv().unwrap() {
            Event::TaskClassified { generation, kind } => {
                assert_eq!(
                    pipe.machine.accept_classified(generation, kind),
                    generation == current,
                    "only the current generation's classification is accepted"
                );
                if generation == current {
                    break;
                }
            }
            other => panic!("task classified expected, got {other:?}"),
        }
    }
}

#[allow(clippy::unwrap_used, clippy::panic)]
fn wait_degraded_done(pipe: &mut Pipeline, current: u64) {
    loop {
        match pipe.events_rx.recv().unwrap() {
            Event::TaskChunk { generation, delta } => {
                assert!(pipe.machine.accept_chunk(generation, delta));
            }
            Event::TaskRepairing { generation } => {
                assert!(pipe.machine.accept_repairing(generation));
            }
            Event::TaskDone {
                generation,
                outcome,
            } => {
                assert_eq!(generation, current);
                assert!(outcome.degraded, "the replayed violating reply degrades");
                assert!(pipe.machine.accept_done(generation, outcome));
                break;
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
}

#[test]
fn failure_lands_in_error_and_retry_succeeds() {
    let engine = MockEngine::new()
        .with_execute_failure_once(GlossError::EngineRateLimited)
        .with_execute_failure_once(GlossError::EngineRateLimited)
        .with_chunks(vec![Ok("{\"note\":\"第二次的产物\"}".into())]);
    let mut pipe = pipeline(&engine);

    let _ = pipe.trigger_and_feed("第一次", Span::none());
    expect_classified(&mut pipe);
    let Event::TaskFailed { generation, error } = pipe.events_rx.recv().unwrap() else {
        panic!("task failed expected");
    };
    assert_eq!(
        pipe.machine.accept_failed(generation, &error),
        FailureOutcome::Shown
    );
    assert_eq!(pipe.machine.state(), AppState::Error);

    let _ = pipe.trigger_and_feed("第二次", Span::none());
    expect_classified(&mut pipe);
    loop {
        match pipe.events_rx.recv().unwrap() {
            Event::TaskChunk { generation, delta } => {
                assert!(pipe.machine.accept_chunk(generation, delta));
            }
            Event::TaskDone {
                generation,
                outcome,
            } => {
                assert_eq!(generation, 2);
                assert!(pipe.machine.accept_done(generation, outcome));
                break;
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
    assert_eq!(pipe.machine.state(), AppState::Show);
    assert_eq!(outcome_note(&pipe.machine), "第二次的产物");
}

#[test]
fn error_card_retry_redispatches_the_same_request() {
    let engine = MockEngine::new()
        .with_execute_failure_once(GlossError::EngineNetwork)
        .with_execute_failure_once(GlossError::EngineNetwork)
        .with_chunks(vec![Ok("{\"note\":\"重试后的产物\"}".into())]);
    let mut pipe = pipeline(&engine);

    let _ = pipe.trigger_and_feed("第一次", Span::none());
    expect_classified(&mut pipe);
    let Event::TaskFailed { generation, error } = pipe.events_rx.recv().unwrap() else {
        panic!("task failed expected");
    };
    assert_eq!(
        pipe.machine.accept_failed(generation, &error),
        FailureOutcome::Shown
    );
    match pipe.machine.overlay_view() {
        Some(OverlayView::Failed {
            action: Some(ErrorAction::Retry),
            ..
        }) => {}
        other => panic!("retryable failure expected, got {other:?}"),
    }

    let request = pipe.machine.retry().expect("retry must be available");
    assert_eq!(request.generation, generation, "retry keeps the generation");
    pipe.commands_tx
        .send(Traced::untraced(Command::RunTask {
            generation: request.generation,
            input: request.input,
            options: request.options,
            cancel: request.cancel,
        }))
        .expect("command channel open");
    wait_done(&mut pipe);
    assert_eq!(pipe.machine.state(), AppState::Show);
    assert_eq!(outcome_note(&pipe.machine), "重试后的产物");
}

#[test]
fn config_change_invalidates_cache_for_the_next_task() {
    let engine = MockEngine::new().with_chunks(vec![Ok("{\"note\":\"结果\"}".into())]);
    let mut pipe = pipeline(&engine);

    pipe.trigger_and_feed("同一段文本", Span::none());
    wait_done(&mut pipe);
    assert_eq!(
        engine.call_count(),
        3,
        "first run: classify repairs once (2 calls) + task"
    );

    pipe.trigger_and_feed("同一段文本", Span::none());
    wait_done(&mut pipe);
    assert_eq!(
        engine.call_count(),
        3,
        "unchanged config must hit the cache"
    );

    pipe.config
        .save(Config {
            model: "deepseek-reasoner".into(),
            ..Default::default()
        })
        .expect("save should succeed");

    pipe.trigger_and_feed("同一段文本", Span::none());
    wait_done(&mut pipe);
    assert_eq!(
        engine.call_count(),
        6,
        "model switch must miss the old cache entry (classify + task again)"
    );

    pipe.config
        .save(Config {
            model: "deepseek-reasoner".into(),
            target_lang: Lang::Ja,
            ..Default::default()
        })
        .expect("save should succeed");

    pipe.trigger_and_feed("同一段文本", Span::none());
    wait_done(&mut pipe);
    assert_eq!(
        engine.call_count(),
        9,
        "target language switch must miss the old cache entry"
    );
    assert_eq!(outcome_note(&pipe.machine), "结果");
}

#[test]
fn frozen_options_carry_the_factory_model_by_default() {
    let engine = MockEngine::new().with_chunks(vec![Ok("{\"note\":\"产物\"}".into())]);
    let mut pipe = pipeline(&engine);

    let _ = pipe.trigger_and_feed("fn main() {}", Span::none());
    expect_classified(&mut pipe);
    wait_done(&mut pipe);
    assert_eq!(engine.call_count(), 3, "classify repairs once + task");
}

#[allow(clippy::expect_used, clippy::panic)]
fn expect_classified(pipe: &mut Pipeline) -> TaskKind {
    match pipe.events_rx.recv().expect("event channel open") {
        Event::TaskClassified { generation, kind } => {
            assert!(pipe.machine.accept_classified(generation, kind));
            kind
        }
        other => panic!("task classified expected, got {other:?}"),
    }
}

#[allow(clippy::panic)]
fn outcome_note(machine: &TaskStateMachine) -> &str {
    match machine.overlay_view() {
        Some(OverlayView::Outcome { outcome, .. }) => &outcome.note,
        other => panic!("expected outcome view, got {other:?}"),
    }
}

#[allow(clippy::expect_used, clippy::panic)]
fn wait_done(pipe: &mut Pipeline) {
    loop {
        let event = pipe
            .events_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("task must finish within 5s (waiting for chunk or done)");
        match event {
            Event::TaskClassified { generation, kind } => {
                assert!(pipe.machine.accept_classified(generation, kind));
            }
            Event::TaskChunk { generation, delta } => {
                assert!(pipe.machine.accept_chunk(generation, delta));
            }
            Event::TaskRepairing { generation } => {
                assert!(pipe.machine.accept_repairing(generation));
            }
            Event::TaskDone {
                generation,
                outcome,
            } => {
                assert!(pipe.machine.accept_done(generation, outcome));
                return;
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
