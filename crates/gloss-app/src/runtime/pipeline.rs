//! 任务消费桥：通道3 → 任务服务 → 通道4，跑在后台运行时（tokio）上
//! （推理在后台进行，主线程不驻留等待）。
//!
//! 桥的职责面：“缓存站点与纯泵”。进桥的任务只有取材输入与触发时冻结
//! 的选项，没有类别——分类是大语言模型（LLM）层内服务执行的事。执行序：
//! 按缓存键查任务缓存 → 命中直出（补发
//! 分类事件与任务完成事件，事件序与未命中路径同形）
//! → 未命中时接线钩子（分类与增量块事件逐条回传）调
//! 服务执行 → 完成态经契约校验两分支
//! 解析出产物 → “非降级才写缓存”（降级产物不投毒）→
//! 任务完成事件。引擎失败不写缓存。
//! 另有密钥预热一条旁路：读一次密钥进进程内缓存（启动期预热），
//! 与任务代数无关。
//!
//! 选项单次快照冻结在状态机（探测时按配置快照解析），桥与大语言模型层都不
//! 回读配置；分类缓存已删除——同一输入在同一选项下判定的类别唯一，
//! 主缓存的键无需类别参与。
//!
//! 每条任务的取消令牌经多路等待与执行竞速——取消在缓存查询与
//! 流式读取的全部挂起点上即时生效，被取消的任务静默丢弃（应用已推
//! 进代数，任何迟到产物都会被判过期）。任务异步体包在崩溃屏障（panic）
//! 里：展开构建下后台崩溃被后台运行时捕获、转为任务失败事件，引擎或编排
//! 层炸掉时用户拿到失败卡而不是永悬的“推理中”，消费循环继续存活；
//! 发布构建改为直接中止时屏障不介入，崩溃直接中止进程。
//! 运行时由启动入口创建并托管，进程退出时随通道3关闭
//! 自然收尾。
//!
//! 状态机侧另有显示用的流式累积，与这里的完成态解析并存：“完成态以
//! 任务完成事件的产物注字段为权威源”（接收端用它整卡覆盖）。

use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::Sender;
use futures::FutureExt;
use gloss_core::engine::AiTaskService;
use gloss_core::log::{Instrument, Span, debug, info, thread, warn};
use gloss_core::model::GlossError;
use gloss_core::ports::ConfigStore;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::channel::{Command, Event, Traced};
use crate::machine::RunRequest;
use crate::runtime::cache::{TaskCache, cache_key};
use crate::runtime::finalize::complete;

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

pub struct CommandRuntime {
    rt: Option<tokio::runtime::Runtime>,
}

impl Drop for CommandRuntime {
    fn drop(&mut self) {
        if let Some(rt) = self.rt.take() {
            rt.shutdown_timeout(SHUTDOWN_TIMEOUT);
        }
    }
}

pub struct TaskBridge {
    service: Arc<AiTaskService>,
    cache: Arc<TaskCache>,
    store: Arc<dyn ConfigStore>,
    events: Sender<Event>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl TaskBridge {
    pub fn new(
        service: Arc<AiTaskService>,
        cache: Arc<TaskCache>,
        store: Arc<dyn ConfigStore>,
        events: Sender<Event>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        Self {
            service,
            cache,
            store,
            events,
            wake: Arc::new(wake),
        }
    }
}

pub fn start_command_runtime(
    bridge: TaskBridge,
    commands: UnboundedReceiver<Traced<Command>>,
) -> std::io::Result<CommandRuntime> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    rt.spawn(consume_loop(bridge, commands));
    Ok(CommandRuntime { rt: Some(rt) })
}

async fn consume_loop(bridge: TaskBridge, mut commands: UnboundedReceiver<Traced<Command>>) {
    while let Some(job) = commands.recv().await {
        let Traced { payload, span } = job;
        match payload {
            Command::RunTask {
                generation,
                input,
                options,
                cancel,
            } => {
                let request = RunRequest {
                    generation,
                    input,
                    options,
                    cancel,
                };
                run_one(&bridge, request, span).await
            }
            Command::PrewarmSecret { keychain_id } => prewarm_secret(&bridge, keychain_id),
        }
    }
    debug!(
        thread = thread::TOKIO,
        "command channel closed, consumer exits"
    );
}

async fn run_one(bridge: &TaskBridge, request: RunRequest, span: Span) {
    let RunRequest {
        generation,
        input,
        options,
        cancel,
    } = request;
    async {
        tokio::select! {
            _ = cancel.cancelled() => {
                debug!(thread = thread::TOKIO, "task cancelled, result dropped");
            }
            outcome = std::panic::AssertUnwindSafe(run_task(bridge, generation, input, options))
                .catch_unwind() => match outcome {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        send_event(&bridge.events, &bridge.wake, Event::TaskFailed { generation, error })
                    }
                    Err(payload) => {
                        let detail = panic_detail(&payload);
                        warn!(
                            thread = thread::TOKIO,
                            detail = %detail,
                            "background task panicked"
                        );
                        send_event(
                            &bridge.events,
                            &bridge.wake,
                            Event::TaskFailed {
                                generation,
                                error: GlossError::EngineResponse(format!(
                                    "engine task panicked: {detail}"
                                )),
                            },
                        );
                    }
                },
        }
    }
    .instrument(span)
    .await;
}

fn prewarm_secret(bridge: &TaskBridge, keychain_id: String) {
    let store = Arc::clone(&bridge.store);
    let events = bridge.events.clone();
    let wake = Arc::clone(&bridge.wake);
    tokio::task::spawn_blocking(move || {
        let result = store.secret(&keychain_id).map(|_| ());
        match &result {
            Ok(()) => info!(
                thread = thread::TOKIO,
                "secret prewarmed into the in-process cache"
            ),
            Err(err) => warn!(
                thread = thread::TOKIO,
                error = %err,
                "secret prewarm failed; the first task will retry"
            ),
        }
        send_event(&events, &wake, Event::SecretPrewarmed { result });
    });
}

async fn run_task(
    bridge: &TaskBridge,
    generation: u64,
    input: gloss_core::task::TaskInput,
    options: gloss_core::task::TaskOptions,
) -> Result<(), GlossError> {
    let key = cache_key(&input, &options);
    if let Some(outcome) = bridge.cache.get(key) {
        deliver_cached(bridge, generation, &options.model, outcome);
        return Ok(());
    }
    debug!(
        model = %options.model,
        "cache miss, calling the engine"
    );

    let pump = |event: Event| send_event(&bridge.events, &bridge.wake, event);
    let output = bridge
        .service
        .run(
            input,
            options,
            |kind| pump(Event::TaskClassified { generation, kind }),
            |delta| pump(Event::TaskChunk { generation, delta }),
            || pump(Event::TaskRepairing { generation }),
        )
        .await?;

    let outcome = complete(output.kind, &output.raw);
    if !outcome.degraded {
        bridge.cache.set(key, outcome.clone());
    }
    pump(Event::TaskDone {
        generation,
        outcome,
    });
    Ok(())
}

fn deliver_cached(
    bridge: &TaskBridge,
    generation: u64,
    model: &str,
    outcome: gloss_core::task::TaskOutcome,
) {
    info!(
        model = %model,
        "cache hit, engine call skipped"
    );
    let pump = |event: Event| send_event(&bridge.events, &bridge.wake, event);
    pump(Event::TaskClassified {
        generation,
        kind: outcome.kind,
    });
    pump(Event::TaskDone {
        generation,
        outcome,
    });
}

fn panic_detail(payload: &Box<dyn std::any::Any + Send>) -> String {
    let detail = payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".into());
    detail.chars().take(200).collect()
}

fn send_event(events: &Sender<Event>, wake: &Arc<dyn Fn() + Send + Sync>, event: Event) {
    if events.send(event).is_ok() {
        wake.as_ref()();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use crate::stubs::engine::MockEngine;
    use crossbeam_channel::Receiver;
    use gloss_core::engine::AiTaskService;
    use gloss_core::model::GlossError;
    use gloss_core::task::{TaskInput, TaskOptions};
    use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

    use super::{CommandRuntime, Event, TaskBridge, start_command_runtime};
    use crate::channel::{Command, Traced};
    use crate::runtime::cache::TaskCache;

    fn start(
        engine: &MockEngine,
    ) -> (
        UnboundedSender<Traced<Command>>,
        Receiver<Event>,
        CommandRuntime,
    ) {
        start_with_store(
            engine,
            Arc::new(crate::stubs::ports::MemoryConfigStore::default()),
        )
    }

    fn start_with_store(
        engine: &MockEngine,
        store: Arc<dyn gloss_core::ports::ConfigStore>,
    ) -> (
        UnboundedSender<Traced<Command>>,
        Receiver<Event>,
        CommandRuntime,
    ) {
        let service = Arc::new(AiTaskService::new(
            Arc::new(engine.clone()) as Arc<dyn gloss_core::ports::AiEngine>
        ));
        let (commands_tx, commands_rx) = unbounded_channel();
        let (events_tx, events_rx) = crossbeam_channel::unbounded();
        let bridge = TaskBridge::new(service, Arc::new(TaskCache::new()), store, events_tx, || {});
        let runtime = start_command_runtime(bridge, commands_rx).expect("runtime should start");
        (commands_tx, events_rx, runtime)
    }

    fn text_options() -> TaskOptions {
        TaskOptions::default()
    }

    fn text_input(text: &str) -> TaskInput {
        TaskInput::Text { text: text.into() }
    }

    fn run(
        commands: &UnboundedSender<Traced<Command>>,
        generation: u64,
        input: TaskInput,
        options: TaskOptions,
    ) -> tokio_util::sync::CancellationToken {
        let cancel = tokio_util::sync::CancellationToken::new();
        commands
            .send(Traced::untraced(Command::RunTask {
                generation,
                input,
                options,
                cancel: cancel.clone(),
            }))
            .expect("command channel should accept");
        cancel
    }

    #[tokio::test]
    async fn classified_chunks_and_done_flow_back_in_order() {
        let engine =
            MockEngine::new().with_chunks(vec![Ok("{\"note\":\"你".into()), Ok("好\"}".into())]);
        let (commands, events, runtime) = start(&engine);

        run(&commands, 7, text_input("hello"), text_options());

        assert!(matches!(
            events.recv().unwrap(),
            Event::TaskClassified {
                generation: 7,
                kind: gloss_core::task::TaskKind::General
            }
        ));
        assert!(matches!(
            events.recv().unwrap(),
            Event::TaskChunk { generation: 7, ref delta } if delta == "{\"note\":\"你"
        ));
        assert!(matches!(
            events.recv().unwrap(),
            Event::TaskChunk { generation: 7, ref delta } if delta == "好\"}"
        ));
        match events.recv().unwrap() {
            Event::TaskDone {
                generation,
                outcome,
            } => {
                assert_eq!(generation, 7);
                assert_eq!(outcome.note, "你好");
            }
            other => panic!("expected task done, got {other:?}"),
        }
        drop(commands);
        tokio::task::spawn_blocking(move || drop(runtime))
            .await
            .expect("shutdown");
    }

    #[tokio::test]
    async fn a_degraded_done_never_enters_the_cache() {
        let engine = MockEngine::new().with_chunks(vec![Ok("整段正文，不是 JSON".into())]);
        let (commands, events, runtime) = start(&engine);

        run(&commands, 1, text_input("违约回复"), text_options());
        loop {
            match events.recv().unwrap() {
                Event::TaskClassified { generation: 1, .. } => {}
                Event::TaskChunk { generation: 1, .. } => {}
                Event::TaskRepairing { generation: 1 } => {}
                Event::TaskDone {
                    generation: 1,
                    outcome,
                } => {
                    assert!(outcome.degraded, "a non-JSON reply degrades");
                    assert_eq!(outcome.note, "");
                    break;
                }
                other => panic!("unexpected event: {other:?}"),
            }
        }
        assert_eq!(
            engine.call_count(),
            6,
            "classify 2 attempts + task 4 attempts: every violated round replays"
        );

        run(&commands, 2, text_input("违约回复"), text_options());
        loop {
            match events.recv().unwrap() {
                Event::TaskClassified { generation: 2, .. } => {}
                Event::TaskChunk { generation: 2, .. } => {}
                Event::TaskRepairing { generation: 2 } => {}
                Event::TaskDone { generation: 2, .. } => break,
                other => panic!("unexpected event: {other:?}"),
            }
        }
        assert_eq!(
            engine.call_count(),
            12,
            "the degraded outcome must not poison the cache: the retry reaches the engine again"
        );
        drop(commands);
        tokio::task::spawn_blocking(move || drop(runtime))
            .await
            .expect("shutdown");
    }

    #[tokio::test]
    async fn cancel_takes_effect_mid_stream() {
        let engine = MockEngine::new()
            .with_chunk_delay(Duration::from_millis(150))
            .with_chunks(vec![Ok("一".into()), Ok("二".into()), Ok("三".into())]);
        let (commands, events, runtime) = start(&engine);

        let cancel = run(&commands, 1, text_input("slow"), text_options());
        assert!(matches!(
            events.recv().unwrap(),
            Event::TaskClassified { generation: 1, .. }
        ));
        assert!(
            matches!(
                events.recv().unwrap(),
                Event::TaskChunk { generation: 1, ref delta } if delta == "一"
            ),
            "first chunk must arrive before cancellation"
        );
        cancel.cancel();

        assert!(
            events.recv_timeout(Duration::from_millis(400)).is_err(),
            "cancelled task must not deliver any further event"
        );
        drop(commands);
        tokio::task::spawn_blocking(move || drop(runtime))
            .await
            .expect("shutdown");
    }

    #[tokio::test]
    async fn engine_failure_becomes_task_failed() {
        let engine = MockEngine::new().with_execute_failure(GlossError::EngineRateLimited);
        let (commands, events, runtime) = start(&engine);

        run(&commands, 3, text_input("boom"), text_options());

        assert!(matches!(
            events.recv().unwrap(),
            Event::TaskClassified {
                generation: 3,
                kind: gloss_core::task::TaskKind::General
            }
        ));
        assert!(matches!(
            events.recv().unwrap(),
            Event::TaskFailed {
                generation: 3,
                error: GlossError::EngineRateLimited
            }
        ));
        drop(commands);
        tokio::task::spawn_blocking(move || drop(runtime))
            .await
            .expect("shutdown");
    }

    #[tokio::test]
    async fn background_panic_becomes_task_failed_and_the_loop_survives() {
        let engine = MockEngine::new().with_execute_panic();
        let (commands, events, runtime) = start(&engine);

        run(&commands, 1, text_input("boom"), text_options());
        assert!(
            matches!(
                events.recv().unwrap(),
                Event::TaskFailed {
                    generation: 1,
                    error: GlossError::EngineResponse(_)
                }
            ),
            "panic must surface as an engine response failure"
        );

        engine.clone().with_chunks(vec![Ok("劫后余生".into())]);
        run(&commands, 2, text_input("again"), text_options());
        loop {
            match events.recv().unwrap() {
                Event::TaskClassified { generation: 2, .. } => {}
                Event::TaskRepairing { generation: 2 } => {}
                Event::TaskChunk {
                    generation: 2,
                    delta,
                } => assert_eq!(delta, "劫后余生"),
                Event::TaskDone { generation: 2, .. } => break,
                other => panic!("unexpected event: {other:?}"),
            }
        }
        drop(commands);
        tokio::task::spawn_blocking(move || drop(runtime))
            .await
            .expect("shutdown");
    }

    #[tokio::test]
    async fn closing_commands_stops_the_consumer() {
        let engine = MockEngine::new();
        let (commands, _events, runtime) = start(&engine);
        drop(commands);
        tokio::task::spawn_blocking(move || drop(runtime))
            .await
            .expect("shutdown within timeout");
    }

    #[tokio::test]
    async fn prewarm_secret_reads_the_store_and_reports_success() {
        use gloss_core::ports::ConfigStore as _;

        let engine = MockEngine::new();
        let store = crate::stubs::ports::MemoryConfigStore::default();
        store
            .set_secret("gloss/deepseek", "sk-stocked")
            .expect("stub store accepts secret");
        let (commands, events, runtime) = start_with_store(
            &engine,
            Arc::new(store) as Arc<dyn gloss_core::ports::ConfigStore>,
        );

        commands
            .send(Traced::untraced(Command::PrewarmSecret {
                keychain_id: "gloss/deepseek".into(),
            }))
            .expect("command channel should accept");

        assert!(
            matches!(
                events.recv().unwrap(),
                Event::SecretPrewarmed { result: Ok(()) }
            ),
            "a stocked store prewarms successfully"
        );
        drop(commands);
        tokio::task::spawn_blocking(move || drop(runtime))
            .await
            .expect("shutdown");
    }

    #[tokio::test]
    async fn prewarm_failure_surfaces_as_err_without_the_secret() {
        let engine = MockEngine::new();
        let failing = crate::stubs::ports::MemoryConfigStore::default()
            .with_secret_failure(GlossError::Config("keychain locked".into()));
        let (commands, events, runtime) = start_with_store(
            &engine,
            Arc::new(failing) as Arc<dyn gloss_core::ports::ConfigStore>,
        );

        commands
            .send(Traced::untraced(Command::PrewarmSecret {
                keychain_id: "gloss/deepseek".into(),
            }))
            .expect("command channel should accept");

        match events.recv().unwrap() {
            Event::SecretPrewarmed {
                result: Err(GlossError::Config(detail)),
            } => assert_eq!(detail, "keychain locked"),
            other => panic!("expected a failed prewarm receipt, got {other:?}"),
        }
        drop(commands);
        tokio::task::spawn_blocking(move || drop(runtime))
            .await
            .expect("shutdown");
    }

    #[test]
    fn prewarm_receipt_never_carries_a_secret_value() {
        let receipt = Event::SecretPrewarmed { result: Ok(()) };
        assert!(
            !format!("{receipt:?}").contains("sk-"),
            "the receipt must be value-free: the secret never leaves the store"
        );
    }
}
