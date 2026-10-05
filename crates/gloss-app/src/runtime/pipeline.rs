//! 通道③ → [`AiTaskService`] → 通道④ 的 tokio 消费桥（推理在
//! tokio 后台，主线程不 await）。
//!
//! 桥的职责面：**缓存站点 + 纯泵**。进桥的任务只有取材输入与触发时冻结
//! 的选项，没有 kind——分类是 LLM 层（`service.run`）内的事。执行序：
//! `cache_key(input, options)` 查 [`TaskCache`] → 命中直出（补发
//! `TaskClassified{outcome.kind}` 再 `TaskDone`，事件序与 miss 路径同形）
//! → miss 时接线钩子（`TaskClassified`/`TaskChunk` 逐条回传）调
//! `service.run` → 完成态经 [`complete`]（JSON 主路径 + 围栏 fallback）
//! 解析出 [`TaskOutcome`] → 写缓存 → `TaskDone`。引擎失败不写缓存。
//!
//! 选项单次快照冻结在 machine（探测时按配置快照解析），桥与 LLM 层都不
//! 回读配置；分类缓存已删除——同一输入在同一选项下判定的 kind 唯一，
//! 主缓存的 key 无需 kind 参与。
//!
//! 每条 `RunTask` 的取消令牌经 `select!` 与执行竞速——取消在缓存查询与
//! 流式读取的全部 await 点上即时生效，被取消的任务静默丢弃（App 已推
//! 进代数，任何迟到产物都会被判 stale）。任务 future 包在 `catch_unwind`
//! 里（后台 panic 被 tokio 捕获转为 TaskFailed）：引擎或编排层炸掉时用户
//! 拿到失败卡而不是永悬的「推理中」，消费循环继续存活。运行时由
//! [`start_command_runtime`] 创建并托管，进程退出时随通道③关闭自然收尾。
//!
//! machine 侧另有显示用的流式累积，与这里的完成态解析并存：**完成态以
//! TaskDone 的 `outcome.note` 为权威源**（accept_done 用它整卡覆盖）。

use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::Sender;
use futures::FutureExt;
use gloss_core::engine::AiTaskService;
use gloss_core::log::{Instrument, debug, info, thread, warn};
use gloss_core::model::GlossError;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::channel::{Command, Event, Traced};
use crate::runtime::cache::{TaskCache, cache_key};
use crate::runtime::finalize::complete;

/// 关停运行时的等待上限：正常退出路径里通道③已关闭、消费循环已在收尾，
/// 只兜底极端悬挂。
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

/// 托管 tokio 运行时与消费任务；drop 即关停（等待上限见
/// [`SHUTDOWN_TIMEOUT`]）。
pub struct CommandRuntime {
    rt: Option<tokio::runtime::Runtime>,
}

impl Drop for CommandRuntime {
    fn drop(&mut self) {
        // shutdown_timeout 消费 Runtime 所有权，Drop 里经 take 取出。
        if let Some(rt) = self.rt.take() {
            rt.shutdown_timeout(SHUTDOWN_TIMEOUT);
        }
    }
}

/// 创建 tokio 运行时并启动消费循环。`cache` 是任务产物缓存（缓存站点在
/// 本桥，见模块文档）；`wake` 在每条回传事件入队后调用，唤醒睡在主线程
/// 事件循环里的 UI。
pub fn start_command_runtime(
    service: Arc<AiTaskService>,
    cache: Arc<TaskCache>,
    commands: UnboundedReceiver<Traced<Command>>,
    events: Sender<Event>,
    wake: impl Fn() + Send + Sync + 'static,
) -> std::io::Result<CommandRuntime> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.spawn(consume_loop(service, cache, commands, events, wake));
    Ok(CommandRuntime { rt: Some(rt) })
}

/// 消费循环：抽干通道③，逐条执行并回传。通道③全部 Sender 关闭（App
/// 随事件循环结束 drop）后循环结束。
async fn consume_loop(
    service: Arc<AiTaskService>,
    cache: Arc<TaskCache>,
    mut commands: UnboundedReceiver<Traced<Command>>,
    events: Sender<Event>,
    wake: impl Fn() + Send + Sync,
) {
    while let Some(job) = commands.recv().await {
        // Command 当前只有 RunTask 一个变体，直接解构；span 来自触发点，
        // 进入它让缓存与引擎内部的日志自动带上代数。
        let Traced { payload, span } = job;
        let Command::RunTask {
            generation,
            input,
            options,
            cancel,
        } = payload;
        // 取消与执行竞速：取消即时生效，覆盖缓存查询与流式读取的全部
        // await 点。被取消的任务不发任何回传——App 取消时已 gen+1，迟到
        // 产物本就该被丢弃。
        //
        // 执行体包在 catch_unwind 里：后台 panic 转 TaskFailed（错误卡给
        // 用户「服务异常」而不是永悬的推理中），循环自身继续消费。
        async {
            tokio::select! {
                _ = cancel.cancelled() => {
                    debug!(thread = thread::TOKIO, "task cancelled, result dropped");
                }
            outcome = std::panic::AssertUnwindSafe(run_task(
                service.as_ref(),
                cache.as_ref(),
                generation,
                input,
                options,
                &events,
                &wake,
            ))
            .catch_unwind() => match outcome {
                Ok(Ok(())) => {}
                Ok(Err(error)) => send_event(&events, &wake, Event::TaskFailed { generation, error }),
                Err(payload) => {
                    let detail = panic_detail(&payload);
                    warn!(
                        thread = thread::TOKIO,
                        detail = %detail,
                        "background task panicked"
                    );
                    send_event(
                        &events,
                        &wake,
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
    debug!(
        thread = thread::TOKIO,
        "command channel closed, consumer exits"
    );
}

/// 单个任务的桥侧执行体（缓存站点 + 纯泵）：查缓存 → 命中直出（事件序
/// 与 miss 同形：`TaskClassified` → `TaskDone`）→ miss 则 `service.run`
/// 转发（增量上抛）→ 完成态解析 → 回填缓存 → `TaskDone`。`Err` 交给调用
/// 方转 TaskFailed——失败路径不写缓存，下次触发重新执行。
async fn run_task(
    service: &AiTaskService,
    cache: &TaskCache,
    generation: u64,
    input: gloss_core::task::TaskInput,
    options: gloss_core::task::TaskOptions,
    events: &Sender<Event>,
    wake: &(impl Fn() + Send + Sync),
) -> Result<(), GlossError> {
    let key = cache_key(&input, &options);
    if let Some(outcome) = cache.get(key) {
        info!(
            model = %options.model,
            "cache hit, engine call skipped"
        );
        send_event(
            events,
            wake,
            Event::TaskClassified {
                generation,
                kind: outcome.kind,
            },
        );
        send_event(
            events,
            wake,
            Event::TaskDone {
                generation,
                outcome,
            },
        );
        return Ok(());
    }
    debug!(
        model = %options.model,
        "cache miss, calling the engine"
    );

    // 钩子把 LLM 层的中间产物逐条泵回主线程：分类定型（LLM 判定或失败
    // 兜底 kind）与流式增量。原始回复的完成态累积归 LLM 层（run 的返回
    // 值），这里只泵不存——完成态解析以 output.raw 为唯一真相源。
    let output = service
        .run(
            &input,
            &options,
            |kind| {
                send_event(events, wake, Event::TaskClassified { generation, kind });
            },
            |delta| {
                send_event(events, wake, Event::TaskChunk { generation, delta });
            },
        )
        .await?;

    let outcome = complete(output.kind, &output.raw);
    cache.set(key, outcome.clone());
    send_event(
        events,
        wake,
        Event::TaskDone {
            generation,
            outcome,
        },
    );
    Ok(())
}

/// 从 panic payload 提取诊断文本（只认 `&str` / `String` 载体，其余没有
/// 稳定形状）；上限与 SSE 诊断一致——这段文本会进 UI 与日志。
fn panic_detail(payload: &Box<dyn std::any::Any + Send>) -> String {
    let detail = payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".into());
    detail.chars().take(200).collect()
}

/// 回传事件 + 唤醒主线程；接收端消失（应用退出）时静默丢弃。
fn send_event(events: &Sender<Event>, wake: &impl Fn(), event: Event) {
    if events.send(event).is_ok() {
        wake();
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

    use super::{CommandRuntime, Event, start_command_runtime};
    use crate::channel::{Command, Traced};
    use crate::runtime::cache::TaskCache;

    fn start(
        engine: &MockEngine,
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
        let runtime = start_command_runtime(
            service,
            Arc::new(TaskCache::new()),
            commands_rx,
            events_tx,
            || {},
        )
        .expect("runtime should start");
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
                kind: gloss_core::task::TaskKind::TranslateWord
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
    async fn cancel_takes_effect_mid_stream() {
        let engine = MockEngine::new()
            .with_chunk_delay(Duration::from_millis(150))
            .with_chunks(vec![Ok("一".into()), Ok("二".into()), Ok("三".into())]);
        let (commands, events, runtime) = start(&engine);

        // 分类先于任何增量（LLM 层恒发判定；脚本解析不出 kind 落兜底）。
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

        // 分类先失败（同引擎同错误）落兜底并回传判定，任务段再失败才是
        // TaskFailed——两条错误同变体，事件序是本断言的靶心。
        assert!(matches!(
            events.recv().unwrap(),
            Event::TaskClassified {
                generation: 3,
                kind: gloss_core::task::TaskKind::TranslateWord
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
                // 分类解析不出 kind 落兜底：判定先于正文回传。
                Event::TaskClassified { generation: 2, .. } => {}
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
}
