//! L1 库级集成测试：状态机（machine）+ 通道③④ + tokio 消费桥 +
//! MockEngine + moka 缓存的全链路时序（M3 分层测试，见 AGENTS.md）。
//!
//! 边界：真实事件线程（通道②消费、RunLoop、CompositeReader）属于 OS
//! 边界，归 L4 opt-in 层——这里取材产物以 `machine.accept_input` 直接
//! 注入，等价于事件线程回传的产物。
//!
//! 驱动方式：全部经公共 API（`TaskStateMachine` / `Channels` /
//! `start_command_runtime`），`cargo test` 全平台可跑。

use std::sync::Arc;
use std::time::Duration;

use gloss_app::channel::{AcquireCommand, Channels, Command, Event, PlatformEvent};
use gloss_app::machine::{AppState, OverlayView, TaskStateMachine};
use gloss_app::pipeline::start_command_runtime;
use gloss_core::cache::MokaCache;
use gloss_core::config::{Config, ModelBinding};
use gloss_core::config_handle::ConfigHandle;
use gloss_core::engine::AiTaskService;
use gloss_core::engine::mock::MockEngine;
use gloss_core::model::GlossError;
use gloss_core::ports::AiEngine;
use gloss_core::ports::mocks::MemoryConfigStore;
use gloss_core::task::{TaskInput, TaskKind};

/// 接好 tokio 桥的完整管线；返回驱动所需的全部端点。
///
/// 测试辅助函数不在 #[test] 内，clippy 的 test 豁免探测不到——显式豁免
/// 并沿用「失败即 panic」的测试断言语义。
#[allow(clippy::expect_used, clippy::panic)]
fn pipeline(engine: &MockEngine) -> Pipeline {
    let service = Arc::new(AiTaskService::new(
        Arc::new(engine.clone()) as Arc<dyn AiEngine>,
        Arc::new(MokaCache::new()),
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
    let runtime =
        start_command_runtime(service, cmd_rx, ev_tx, || {}).expect("tokio bridge should start");
    Pipeline {
        machine: TaskStateMachine::new(),
        config: Arc::new(ConfigHandle::with_config(
            Arc::new(MemoryConfigStore::default()),
            Config::default(),
        )),
        _pe_tx: pe_tx,
        _ac_tx: ac_tx,
        commands_tx: cmd_tx,
        events_rx: ev_rx,
        _runtime: runtime,
    }
}

/// 管线驱动端点集合。
struct Pipeline {
    machine: TaskStateMachine,
    /// 运行时配置句柄：触发时取快照，测试可 `save` 模拟设置页保存。
    config: Arc<ConfigHandle>,
    _pe_tx: crossbeam_channel::Sender<PlatformEvent>,
    /// 通道②发送端：真实消费者在事件线程（L4 层），L1 不接。
    _ac_tx: crossbeam_channel::Sender<AcquireCommand>,
    commands_tx: tokio::sync::mpsc::UnboundedSender<Command>,
    events_rx: crossbeam_channel::Receiver<Event>,
    /// 保持 tokio 运行时存活；drop 即关停（消费循环随通道③关闭退出）。
    _runtime: gloss_app::pipeline::CommandRuntime,
}

impl Pipeline {
    /// 触发（划词手势）→ 模拟取材产物回传 → 下发通道③。
    #[allow(clippy::expect_used, clippy::panic)]
    fn trigger_and_feed(&mut self, text: &str) -> tokio_util::sync::CancellationToken {
        let command = self
            .machine
            .trigger(&PlatformEvent::SelectionGesture, &self.config.snapshot())
            .expect("selection gesture must acquire");
        let AcquireCommand::AcquireText { generation, .. } = &command else {
            panic!("acquire text expected");
        };
        let request = self
            .machine
            .accept_input(
                *generation,
                TaskInput::Text {
                    text: text.into(),
                    hint: None,
                },
            )
            .expect("input should be accepted while fetching");
        self.commands_tx
            .send(Command::RunTask {
                generation: request.generation,
                task: request.task,
                cancel: request.cancel.clone(),
            })
            .expect("command channel open");
        request.cancel
    }
}

/// 全链路时序：触发 → 流式 chunk 逐条回流 → TaskDone 定格 Show。
#[test]
fn full_flow_streams_and_settles() {
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

    // 回传事件逐条喂给状态机（等价于主线程 drain_events 的采纳路径）。
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

/// 验收标准：连续触发 A→B 时 A 的迟到 chunk 不串台，A 的令牌被取消，
/// B 正常完成。
#[test]
fn superseded_trigger_cancels_and_filters_late_events() {
    let engine = MockEngine::new()
        .with_chunk_delay(Duration::from_millis(120))
        .with_chunks(vec![Ok("A1".into()), Ok("A2".into())]);
    let mut pipe = pipeline(&engine);

    let token_a = pipe.trigger_and_feed("A 的原文");
    let gen_a = pipe.machine.generation();

    // B 在 A 完成前触发：A 立即取消，代数推进。
    let token_b = pipe.trigger_and_feed("B 的原文");
    let gen_b = pipe.machine.generation();
    assert!(token_a.is_cancelled(), "new trigger must cancel task A");
    assert!(!token_b.is_cancelled());
    assert_eq!(gen_b, gen_a + 1);

    // 取消在首 chunk 之前生效：A 不产生任何回传（取消赢下竞速）。
    // 状态机侧仍验证陈旧过滤：手喂 A 代数的 chunk 必须被拒。
    assert!(
        !pipe.machine.accept_chunk(gen_a, "A 的迟到 chunk".into()),
        "stale generation must be dropped by the machine"
    );

    // B 正常完成。
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

/// 验收标准：失败落 Error 态，再次触发即重试成功（一次性失败注入）。
#[test]
fn failure_lands_in_error_and_retry_succeeds() {
    let engine = MockEngine::new()
        .with_execute_failure_once(GlossError::EngineRateLimited)
        .with_chunks(vec![Ok("第二次的产物".into())]);
    let mut pipe = pipeline(&engine);

    // 第一次：失败回传落 Error。
    let _ = pipe.trigger_and_feed("第一次");
    let Event::TaskFailed { generation, error } = pipe.events_rx.recv().unwrap() else {
        panic!("task failed expected");
    };
    assert!(pipe.machine.accept_failed(generation, &error));
    assert_eq!(pipe.machine.state(), AppState::Error);

    // 第二次（重试）：先 chunk 后 done，照常完成。
    let _ = pipe.trigger_and_feed("第二次");
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

#[allow(clippy::panic)] // 测试辅助：失败即 panic 是断言语义
fn outcome_body(machine: &TaskStateMachine) -> &str {
    match machine.overlay_view() {
        Some(OverlayView::Outcome(outcome)) => &outcome.body,
        other => panic!("expected outcome view, got {other:?}"),
    }
}

/// 收事件直到本任务完成（缓存命中时没有 chunk，只有 TaskDone）。
#[allow(clippy::expect_used, clippy::panic)] // 测试辅助：失败即 panic 是断言语义
fn wait_done(pipe: &mut Pipeline) {
    loop {
        match pipe.events_rx.recv().expect("event expected") {
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

/// 验收标准（M4-T3）：运行时保存的配置贯穿到缓存 key——同一段文本，换
/// 模型后不再命中旧产物，重新请求引擎；不换则命中缓存不再请求。
///
/// 这一层能证伪的正是「配置只改在 UI、没进管道」：模型经快照解析进任务
/// 选项（machine），再由桥送进 `AiTaskService` 参与 key（pipeline）。
#[test]
fn model_change_invalidates_cache_for_the_next_task() {
    let engine = MockEngine::new().with_chunks(vec![Ok("结果".into())]);
    let mut pipe = pipeline(&engine);

    pipe.trigger_and_feed("同一段文本");
    wait_done(&mut pipe);
    assert_eq!(engine.call_count(), 1, "first run must reach the engine");

    // 配置未动：同文本同模型命中缓存，引擎不再被调用。
    pipe.trigger_and_feed("同一段文本");
    wait_done(&mut pipe);
    assert_eq!(
        engine.call_count(),
        1,
        "unchanged config must hit the cache"
    );

    // 设置页保存新模型（划词手势的 kind 即 TranslateWord）。
    pipe.config
        .save(Config {
            model_by_kind: vec![ModelBinding {
                kind: TaskKind::TranslateWord,
                model: "deepseek-chat".into(),
            }],
            ..Default::default()
        })
        .expect("save should succeed");

    pipe.trigger_and_feed("同一段文本");
    wait_done(&mut pipe);
    assert_eq!(
        engine.call_count(),
        2,
        "model switch must miss the old cache entry"
    );
    assert_eq!(outcome_body(&pipe.machine), "结果");
}
