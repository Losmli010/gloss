//! L1 库级集成测试：状态机（machine）+ 通道③④ + tokio 消费桥 +
//! MockEngine + TaskCache 的全链路时序（见 AGENTS.md）。
//!
//! 边界：真实事件线程（通道②消费、RunLoop、CompositeReader）属于 OS
//! 边界，归 L4 opt-in 层——这里取材产物以 `machine.commit_selection`
//! 直接注入，等价于事件线程回传的产物。
//!
//! 分类完全交给 LLM 层：每次任务先花一次引擎调用分类（MockEngine 脚本
//! 通常解析不出 kind，落兜底 TranslateWord），再花一次执行；脚本里带
//! `"kind"` 字段的围栏可让分类解析出具体 kind。纯流式/取消/重试语义与
//! 缓存/兜底语义都按这两次调用记账。
//!
//! 驱动方式：全部经公共 API（`TaskStateMachine` / `Channels` /
//! `start_command_runtime`），`cargo test` 直接跑。

use std::sync::Arc;
use std::time::Duration;

use gloss_app::channel::{AcquireCommand, Channels, Command, Event, PlatformEvent, Traced};
use gloss_app::machine::{
    AppState, ErrorAction, FailureOutcome, InputOutcome, OverlayView, TaskStateMachine,
};
use gloss_app::runtime::cache::TaskCache;
use gloss_app::runtime::pipeline::start_command_runtime;
use gloss_core::config::Config;
use gloss_core::config_handle::ConfigHandle;
use gloss_core::engine::AiTaskService;
use gloss_core::guard::SceneFacts;
use gloss_core::log::Span;
use gloss_core::model::Locale;
use gloss_core::model::ScreenPoint;
use gloss_core::model::{GlossError, Lang};
use gloss_core::ports::AiEngine;
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
    let runtime = start_command_runtime(service, Arc::new(TaskCache::new()), cmd_rx, ev_tx, || {})
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
            .commit_selection(*generation, TaskInput::Text { text: text.into() })
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
}

#[test]
fn engine_logs_carry_the_task_span() {
    let logs = gloss_core::log::capture_global();
    let engine = MockEngine::new().with_chunks(vec![Ok("产物".into())]);
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
        TaskKind::TranslateWord,
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
            assert_eq!(
                outcome.structured,
                OutcomeStructured::WordCard {
                    phonetic: None,
                    examples: Vec::new(),
                },
                "missing word-kind fields fall back per the contract (empty subprov)"
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
        .with_chunks(vec![Ok("兜底产物".into())]);
    let mut pipe = pipeline(&engine);

    pipe.trigger_and_feed("第一次划词", Span::none());
    assert_eq!(
        expect_classified(&mut pipe),
        TaskKind::TranslateWord,
        "the failed classification must fall back to the constant default kind"
    );
    wait_done(&mut pipe);
    assert_eq!(outcome_note(&pipe.machine), "兜底产物");
    assert_eq!(pipe.machine.state(), AppState::Show);

    let text = logs.text();
    let fallback_line = text
        .lines()
        .find(|line| line.contains("classification failed"))
        .expect("the fallback must leave a trace");
    assert!(
        !fallback_line.contains("第一次划词"),
        "the fallback warn must not carry the selection content: {fallback_line}"
    );
}

#[test]
fn classify_reads_the_kind_from_a_kind_tagged_script() {
    let engine =
        MockEngine::new().with_chunks(vec![Ok("```gloss\n{\"kind\":\"ExplainCode\"}\n```".into())]);
    let mut pipe = pipeline(&engine);

    pipe.trigger_and_feed("fn main() {}", Span::none());
    assert_eq!(
        expect_classified(&mut pipe),
        TaskKind::ExplainCode,
        "the classify call parses the kind tag from the fenced script"
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
        2,
        "first run: one classify call + one task execution"
    );

    pipe.trigger_and_feed("同一段文本", Span::none());
    assert_eq!(
        expect_classified(&mut pipe),
        TaskKind::TranslateWord,
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
        2,
        "the product cache must not reach the engine again"
    );
    assert_eq!(pipe.machine.state(), AppState::Show);
}

#[test]
fn legacy_fence_contract_falls_back_to_a_complete_card() {
    // 围栏里的 kind 标签让分类解析出代码解释；同一段旧契约脚本作为任务
    // 回复时，完成态走围栏 fallback 出完整卡。
    let engine = MockEngine::new().with_chunks(vec![Ok(
        "旧契约正文\n```gloss\n{\"kind\":\"ExplainCode\",\"title\":\"旧契约摘要\"}\n```".into(),
    )]);
    let mut pipe = pipeline(&engine);

    pipe.trigger_and_feed("gloss", Span::none());
    assert_eq!(
        expect_classified(&mut pipe),
        TaskKind::ExplainCode,
        "the classify call reads the kind tag from the legacy fence"
    );
    wait_done(&mut pipe);

    match pipe.machine.overlay_view() {
        Some(OverlayView::Outcome { outcome, .. }) => {
            assert_eq!(
                outcome.note, "旧契约正文",
                "the fence fallback strips the structured block from the note"
            );
            assert_eq!(
                outcome.structured,
                OutcomeStructured::Plain {
                    examples: Vec::new()
                },
                "the old contract's fence still lands as a complete card"
            );
        }
        other => panic!("expected outcome view, got {other:?}"),
    }
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
        code_language: None,
        structured: OutcomeStructured::Plain {
            examples: Vec::new(),
        },
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

    // 每条任务各发一次 TaskClassified：先到的一条属于已被顶掉的
    // A（或竞速下的 B），按代数分流——只有当前代的判定被采纳。
    loop {
        match pipe.events_rx.recv().unwrap() {
            Event::TaskClassified { generation, kind } => {
                assert_eq!(
                    pipe.machine.accept_classified(generation, kind),
                    generation == gen_b,
                    "only the current generation's classification is accepted"
                );
                if generation == gen_b {
                    break;
                }
            }
            other => panic!("task classified expected, got {other:?}"),
        }
    }
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
    // 两次一次性失败：第一次触发里分类吃掉第一条、任务吃掉第二条，
    // 落 Error；第二次触发时失败队列已空，照常产流。
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
    let engine = MockEngine::new().with_chunks(vec![Ok("结果".into())]);
    let mut pipe = pipeline(&engine);

    pipe.trigger_and_feed("同一段文本", Span::none());
    wait_done(&mut pipe);
    assert_eq!(engine.call_count(), 2, "first run: classify + task");

    pipe.trigger_and_feed("同一段文本", Span::none());
    wait_done(&mut pipe);
    assert_eq!(
        engine.call_count(),
        2,
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
        4,
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
        6,
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
    assert_eq!(engine.call_count(), 2);
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
fn outcome_note(machine: &TaskStateMachine) -> &str {
    match machine.overlay_view() {
        Some(OverlayView::Outcome { outcome, .. }) => &outcome.note,
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
