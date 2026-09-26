//! L1 库级集成测试：状态机（machine）+ 通道③④ + tokio 消费桥 +
//! MockEngine + moka 缓存的全链路时序（见 AGENTS.md）。
//!
//! 边界：真实事件线程（通道②消费、RunLoop、CompositeReader）属于 OS
//! 边界，归 L4 opt-in 层——这里取材产物以 `machine.accept_input` 直接
//! 注入，等价于事件线程回传的产物。
//!
//! 两条触发路径：划词手势下发 `TaskKind::Auto`（走桥的分类前半程），
//! 热键绑定固定 kind（跳过分类直达缓存/引擎）。纯流式/取消/重试语义用
//! 热键路径锁定，分类编排用手势路径锁定。
//!
//! 驱动方式：全部经公共 API（`TaskStateMachine` / `Channels` /
//! `start_command_runtime`），`cargo test` 直接跑。

use std::sync::Arc;
use std::time::Duration;

use gloss_app::channel::{AcquireCommand, Channels, Command, Event, PlatformEvent, Traced};
use gloss_app::machine::{AppState, ErrorAction, InputOutcome, OverlayView, TaskStateMachine};
use gloss_app::pipeline::start_command_runtime;
use gloss_core::cache::MokaCache;
use gloss_core::config::{Config, ModelBinding};
use gloss_core::config_handle::ConfigHandle;
use gloss_core::engine::AiTaskService;
use gloss_core::guard::SceneFacts;
use gloss_core::log::Span;
use gloss_core::model::Locale;
use gloss_core::model::ScreenPoint;
use gloss_core::model::{GlossError, Lang};
use gloss_core::ports::AiEngine;
use gloss_core::task::{InputHint, OutcomeStructured, TaskInput, TaskKind, TaskOutcome};

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
        service,
        Arc::new(MokaCache::new()),
        Arc::clone(&config),
        cmd_rx,
        ev_tx,
        || {},
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
    _runtime: gloss_app::pipeline::CommandRuntime,
}

impl Pipeline {
    #[allow(clippy::expect_used, clippy::panic)]
    fn dispatch(
        &mut self,
        request: gloss_app::machine::RunRequest,
    ) -> tokio_util::sync::CancellationToken {
        self.commands_tx
            .send(Traced {
                payload: Command::RunTask {
                    generation: request.generation,
                    task: request.task,
                    cancel: request.cancel.clone(),
                },
                span: Span::none(),
            })
            .expect("command channel open");
        request.cancel
    }

    /// 划词手势路径：触发 → 注入选区文本 → 下发 Auto 任务。
    #[allow(clippy::expect_used, clippy::panic)]
    fn trigger_and_feed(&mut self, text: &str) -> tokio_util::sync::CancellationToken {
        self.trigger_and_feed_with(text, None, Span::none())
    }

    /// 带模态提示的划词路径（代码语言提示直通分类）。
    #[allow(clippy::expect_used, clippy::panic)]
    fn trigger_and_feed_with(
        &mut self,
        text: &str,
        hint: Option<InputHint>,
        span: Span,
    ) -> tokio_util::sync::CancellationToken {
        let command = self
            .machine
            .trigger(
                &PlatformEvent::SelectionGesture {
                    pos: ScreenPoint::new(0, 0),
                },
                &self.config.snapshot(),
                Locale::Zh,
                &SceneFacts::default(),
            )
            .expect("selection gesture must acquire");
        let AcquireCommand::AcquireText { generation, .. } = &command else {
            panic!("acquire text expected");
        };
        let InputOutcome::Dispatch(request) = self.machine.accept_input(
            *generation,
            TaskInput::Text {
                text: text.into(),
                hint,
            },
        ) else {
            panic!("input should be accepted while fetching");
        };
        self.commands_tx
            .send(Traced {
                payload: Command::RunTask {
                    generation: request.generation,
                    task: request.task,
                    cancel: request.cancel.clone(),
                },
                span,
            })
            .expect("command channel open");
        request.cancel
    }

    /// 热键路径：固定 kind，不经过分类前半程。
    #[allow(clippy::expect_used, clippy::panic)]
    fn trigger_hotkey_and_feed(
        &mut self,
        kind: TaskKind,
        text: &str,
    ) -> tokio_util::sync::CancellationToken {
        let command = self
            .machine
            .trigger(
                &PlatformEvent::HotkeyTriggered {
                    binding: gloss_core::task::HotkeyBinding {
                        trigger: "Cmd+Shift+T".into(),
                        kind,
                        source: gloss_core::task::InputSource::Selection,
                    },
                },
                &self.config.snapshot(),
                Locale::Zh,
                &SceneFacts::default(),
            )
            .expect("enabled hotkey must acquire");
        let AcquireCommand::AcquireText { generation, .. } = &command else {
            panic!("acquire text expected");
        };
        let InputOutcome::Dispatch(request) = self.machine.accept_input(
            *generation,
            TaskInput::Text {
                text: text.into(),
                hint: None,
            },
        ) else {
            panic!("input should be accepted while fetching");
        };
        self.dispatch(request)
    }
}

#[test]
fn engine_logs_carry_the_task_span() {
    let logs = gloss_core::log::capture_global();
    let engine = MockEngine::new().with_chunks(vec![Ok("产物".into())]);
    let mut pipe = pipeline(&engine);

    pipe.trigger_and_feed("hello");
    wait_done(&mut pipe);
    pipe.trigger_and_feed_with("hello", None, gloss_core::log::task_span(2));
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
    let engine = MockEngine::new().with_chunks(vec![
        Ok("光泽".into()),
        Ok("：注释".into()),
        Ok(
            "\n```gloss\n{\"word\":\"gloss\",\"phonetic\":\"/ɡlɒs/\",\"senses\":[{\"pos\":\"n.\",\"meaning\":\"光泽\",\"examples\":[]}]}```"
                .into(),
        ),
    ]);
    let mut pipe = pipeline(&engine);

    let token = pipe.trigger_and_feed("gloss");
    assert_eq!(pipe.machine.state(), AppState::Translating);
    assert!(!token.is_cancelled());

    assert_eq!(
        expect_classified(&mut pipe),
        TaskKind::TranslateWord,
        "the classification replays the script and parses the gloss fence"
    );

    for _ in 0..3 {
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
        Some(OverlayView::Outcome(outcome)) => {
            assert_eq!(outcome.body, "光泽：注释", "fence stripped from body");
            assert!(
                matches!(
                    &outcome.structured,
                    gloss_core::task::OutcomeStructured::WordCard { word, senses, .. }
                        if word == "gloss" && senses.len() == 1
                ),
                "word card must be parsed from the structured block"
            );
        }
        other => panic!("expected outcome view, got {other:?}"),
    }
}

#[test]
fn classify_failure_falls_back_and_the_task_still_completes() {
    let engine = MockEngine::new()
        .with_execute_failure_once(GlossError::EngineRateLimited)
        .with_chunks(vec![Ok("兜底产物".into())]);
    let mut pipe = pipeline(&engine);

    pipe.trigger_and_feed("第一次划词");
    assert_eq!(
        expect_classified(&mut pipe),
        TaskKind::TranslateWord,
        "the failed classification must fall back to the default text kind"
    );
    wait_done(&mut pipe);
    assert_eq!(outcome_body(&pipe.machine), "兜底产物");
    assert_eq!(pipe.machine.state(), AppState::Show);
}

#[test]
fn code_language_hint_skips_the_classification_round_trip() {
    let engine = MockEngine::new().with_chunks(vec![Ok("代码解释产物".into())]);
    let mut pipe = pipeline(&engine);

    pipe.trigger_and_feed_with(
        "fn main() {}",
        Some(InputHint::CodeLanguage("rust".into())),
        Span::none(),
    );
    assert_eq!(
        expect_classified(&mut pipe),
        TaskKind::ExplainCode,
        "the hint classifies directly"
    );
    wait_done(&mut pipe);
    assert_eq!(
        engine.call_count(),
        1,
        "only the task execution may reach the engine"
    );
}

#[test]
fn cache_hit_delivers_done_without_chunks_or_engine() {
    let engine = MockEngine::new().with_chunks(vec![Ok("{\"kind\":\"TranslateWord\"}".into())]);
    let mut pipe = pipeline(&engine);

    pipe.trigger_and_feed("同一段文本");
    assert_eq!(
        expect_classified(&mut pipe),
        TaskKind::TranslateWord,
        "first run classifies via the engine (script replay)"
    );
    wait_done(&mut pipe);
    assert_eq!(
        engine.call_count(),
        2,
        "first run: one classify call + one task execution"
    );

    pipe.trigger_and_feed("同一段文本");
    assert_eq!(
        expect_classified(&mut pipe),
        TaskKind::TranslateWord,
        "second run resolves from the classify cache"
    );
    match pipe.events_rx.recv().unwrap() {
        Event::TaskDone {
            generation,
            outcome,
        } => {
            assert_eq!(generation, 2);
            assert_eq!(outcome.body, "{\"kind\":\"TranslateWord\"}");
            assert!(pipe.machine.accept_done(generation, outcome));
        }
        other => panic!("cache hit must settle directly without chunks, got {other:?}"),
    }
    assert_eq!(
        engine.call_count(),
        2,
        "classify cache + product cache must not reach the engine again"
    );
    assert_eq!(pipe.machine.state(), AppState::Show);
}

#[test]
fn hide_overlay_cancels_the_stream_and_late_events_are_dropped() {
    let engine = MockEngine::new()
        .with_chunk_delay(Duration::from_millis(150))
        .with_chunks(vec![Ok("一".into()), Ok("二".into()), Ok("三".into())]);
    let mut pipe = pipeline(&engine);

    let token = pipe.trigger_and_feed("慢慢来");
    let _ = expect_classified(&mut pipe);
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
        body: "迟到的产物".into(),
        structured: OutcomeStructured::Plain { title: None },
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

    let token_a = pipe.trigger_and_feed("A 的原文");
    let gen_a = pipe.machine.generation();

    let token_b = pipe.trigger_and_feed("B 的原文");
    let gen_b = pipe.machine.generation();
    assert!(token_a.is_cancelled(), "new trigger must cancel task A");
    assert!(!token_b.is_cancelled());
    assert_eq!(gen_b, gen_a + 1);

    assert!(
        !pipe.machine.accept_chunk(gen_a, "A 的迟到 chunk".into()),
        "stale generation must be dropped by the machine"
    );

    assert_eq!(
        expect_classified(&mut pipe),
        TaskKind::TranslateWord,
        "task B classifies (the script text parses as nothing, so the fallback applies)"
    );
    for expected in ["A1", "A2"] {
        let Event::TaskChunk { generation, delta } = pipe.events_rx.recv().unwrap() else {
            panic!("chunk expected");
        };
        assert_eq!(generation, gen_b);
        assert_eq!(delta, expected);
        assert!(pipe.machine.accept_chunk(generation, delta));
    }
    let Event::TaskDone {
        generation,
        outcome,
    } = pipe.events_rx.recv().unwrap()
    else {
        panic!("task done expected");
    };
    assert_eq!(generation, gen_b);
    assert!(pipe.machine.accept_done(generation, outcome));
    assert_eq!(pipe.machine.state(), AppState::Show);
}

#[test]
fn failure_lands_in_error_and_retry_succeeds() {
    let engine = MockEngine::new()
        .with_execute_failure_once(GlossError::EngineRateLimited)
        .with_chunks(vec![Ok("第二次的产物".into())]);
    let mut pipe = pipeline(&engine);

    let _ = pipe.trigger_hotkey_and_feed(TaskKind::TranslateSentence, "第一次");
    let Event::TaskFailed { generation, error } = pipe.events_rx.recv().unwrap() else {
        panic!("task failed expected");
    };
    assert!(pipe.machine.accept_failed(generation, &error));
    assert_eq!(pipe.machine.state(), AppState::Error);

    let _ = pipe.trigger_hotkey_and_feed(TaskKind::TranslateSentence, "第二次");
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
    assert_eq!(outcome_body(&pipe.machine), "第二次的产物");
}

#[test]
fn error_card_retry_redispatches_the_same_task() {
    let engine = MockEngine::new()
        .with_execute_failure_once(GlossError::EngineNetwork)
        .with_chunks(vec![Ok("重试后的产物".into())]);
    let mut pipe = pipeline(&engine);

    let _ = pipe.trigger_hotkey_and_feed(TaskKind::TranslateSentence, "第一次");
    let Event::TaskFailed { generation, error } = pipe.events_rx.recv().unwrap() else {
        panic!("task failed expected");
    };
    assert!(pipe.machine.accept_failed(generation, &error));
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
            task: request.task,
            cancel: request.cancel,
        }))
        .expect("command channel open");
    wait_done(&mut pipe);
    assert_eq!(pipe.machine.state(), AppState::Show);
    assert_eq!(outcome_body(&pipe.machine), "重试后的产物");
}

#[test]
fn config_change_invalidates_cache_for_the_next_task() {
    let engine = MockEngine::new().with_chunks(vec![Ok("结果".into())]);
    let mut pipe = pipeline(&engine);

    pipe.trigger_hotkey_and_feed(TaskKind::TranslateWord, "同一段文本");
    wait_done(&mut pipe);
    assert_eq!(engine.call_count(), 1, "first run must reach the engine");

    pipe.trigger_hotkey_and_feed(TaskKind::TranslateWord, "同一段文本");
    wait_done(&mut pipe);
    assert_eq!(
        engine.call_count(),
        1,
        "unchanged config must hit the cache"
    );

    pipe.config
        .save(Config {
            model_by_kind: vec![ModelBinding {
                kind: TaskKind::TranslateWord,
                model: "deepseek-reasoner".into(),
            }],
            ..Default::default()
        })
        .expect("save should succeed");

    pipe.trigger_hotkey_and_feed(TaskKind::TranslateWord, "同一段文本");
    wait_done(&mut pipe);
    assert_eq!(
        engine.call_count(),
        2,
        "model switch must miss the old cache entry"
    );

    pipe.config
        .save(Config {
            target_lang: Lang::Ja,
            model_by_kind: vec![ModelBinding {
                kind: TaskKind::TranslateWord,
                model: "deepseek-reasoner".into(),
            }],
            ..Default::default()
        })
        .expect("save should succeed");

    pipe.trigger_hotkey_and_feed(TaskKind::TranslateWord, "同一段文本");
    wait_done(&mut pipe);
    assert_eq!(
        engine.call_count(),
        3,
        "target language switch must miss the old cache entry"
    );
    assert_eq!(outcome_body(&pipe.machine), "结果");
}

#[allow(clippy::expect_used, clippy::panic)] // 测试辅助：失败即 panic 是断言语义
fn expect_classified(pipe: &mut Pipeline) -> TaskKind {
    match pipe.events_rx.recv().expect("event channel open") {
        Event::TaskClassified { generation, kind } => {
            assert!(pipe.machine.accept_classified(generation, kind));
            kind
        }
        other => panic!("task classified expected, got {other:?}"),
    }
}

#[allow(clippy::panic)] // 测试辅助：失败即 panic 是断言语义
fn outcome_body(machine: &TaskStateMachine) -> &str {
    match machine.overlay_view() {
        Some(OverlayView::Outcome(outcome)) => &outcome.body,
        other => panic!("expected outcome view, got {other:?}"),
    }
}

#[allow(clippy::expect_used, clippy::panic)] // 测试辅助：失败即 panic 是断言语义
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
