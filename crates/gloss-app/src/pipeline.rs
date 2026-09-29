//! 通道③ → [`AiTaskService`] → 通道④ 的 tokio 消费桥（推理在
//! tokio 后台，主线程不 await）。
//!
//! 桥的职责面：**缓存编排 + body 累积 + 产物组装**。core 服务只渲染与
//! 转发——桥在派发前查主缓存（key = `cache_key`，命中直出 TaskDone、
//! 不调引擎），在转发回调里累积原始 body 并逐条上抛 TaskChunk，流走完后
//! 经 `finalize_outcome` 组装产物回填缓存再发 TaskDone；引擎失败不写缓存。
//! machine 侧另有显示用的流式累积，与这里的完成态累积并存：**完成态以
//! TaskDone 的 `outcome.body` 为权威源**（accept_done 用它整卡覆盖）。
//!
//! 每条 `RunTask` 的取消令牌经 `select!` 与执行竞速——取消在流式读取的
//! 多个 await 点上即时生效，被取消的任务静默丢弃（App 已推进代数，任何
//! 迟到产物都会被判 stale）。任务 future 包在 `catch_unwind` 里（后台
//! panic 被 tokio 捕获转为 TaskFailed）：引擎或编排层炸掉时用户拿到失败
//! 卡而不是永悬的「推理中」，消费循环继续存活。运行时由
//! [`start_command_runtime`] 创建并托管，进程退出时随通道③关闭自然收尾。

use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::Sender;
use futures::FutureExt;
use gloss_core::cache::cache_key;
use gloss_core::config::DEFAULT_TEXT_MODEL;
use gloss_core::engine::{AiTaskService, finalize_outcome};
use gloss_core::log::{Instrument, debug, info, thread, warn};
use gloss_core::model::GlossError;
use gloss_core::ports::Cache;
use gloss_core::task::Task;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::channel::{Command, Event, Traced};

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

/// 创建 tokio 运行时并启动消费循环。`cache` 是主产物缓存（编排在本桥，
/// 见模块文档）；`wake` 在每条回传事件入队后调用，唤醒睡在主线程事件
/// 循环里的 UI。
pub fn start_command_runtime(
    service: Arc<AiTaskService>,
    cache: Arc<dyn Cache>,
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
    cache: Arc<dyn Cache>,
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
            task,
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
                &task,
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

/// 单个任务的桥侧执行体：模型解析 → 主缓存查询（命中直出）→ 服务渲染
/// 转发（增量上抛 + body 累积）→ `finalize_outcome` 组装 → 回填缓存 →
/// TaskDone。`Err` 交给调用方转 TaskFailed——失败路径不写缓存，下次触发
/// 重新执行。
async fn run_task(
    service: &AiTaskService,
    cache: &dyn Cache,
    generation: u64,
    task: &Task,
    events: &Sender<Event>,
    wake: &(impl Fn() + Send + Sync),
) -> Result<(), GlossError> {
    let model = model_for(task)?;
    let key = cache_key(task, model);
    if let Some(outcome) = cache.get(key) {
        info!(
            kind = ?task.kind,
            model = %model,
            "cache hit, engine call skipped"
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
        kind = ?task.kind,
        model = %model,
        "cache miss, calling the engine"
    );

    let mut body = String::new();
    service
        .execute(task, model, |delta| {
            body.push_str(&delta);
            send_event(events, wake, Event::TaskChunk { generation, delta });
        })
        .await?;

    let outcome = finalize_outcome(task.kind, &body);
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

/// 模型 id：任务自带（App 在触发时按 `Config::resolved_model` 解析）。
///
/// 留空表示该 kind 没有可用模型——图像类未配视觉模型时就是这种情形（
/// 接线后由它配视觉模型）。这里**明确失败**而不是退回文本模型：拿 `deepseek-chat`
/// 去接图像任务，用户看到的是服务端 400，与「去设置页配模型」的引导完全相反
/// （与 `Config::resolved_model` 的取舍一致）。文本 kind 的兜底只服务直接构造
/// 任务的测试，App 路径下恒有值。
///
/// 模型参与缓存 key：同任务换模型不命中旧产物。
fn model_for(task: &Task) -> Result<&str, GlossError> {
    task.options
        .model_override
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .or_else(|| task.kind.accepts_text().then_some(DEFAULT_TEXT_MODEL))
        .ok_or_else(|| GlossError::Config("no model configured for this task kind".into()))
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
    use gloss_core::cache::MokaCache;
    use gloss_core::engine::AiTaskService;
    use gloss_core::model::GlossError;
    use gloss_core::task::{Task, TaskInput, TaskKind, TaskOptions};
    use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

    use super::{CommandRuntime, Event, start_command_runtime};
    use crate::channel::{Command, Traced};

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
            Arc::new(MokaCache::new()),
            commands_rx,
            events_tx,
            || {},
        )
        .expect("runtime should start");
        (commands_tx, events_rx, runtime)
    }

    fn text_task(text: &str) -> Task {
        Task {
            kind: TaskKind::TranslateSentence,
            input: TaskInput::Text {
                text: text.into(),
                hint: None,
            },
            options: TaskOptions::default(),
        }
    }

    fn run(
        commands: &UnboundedSender<Traced<Command>>,
        generation: u64,
        task: Task,
    ) -> tokio_util::sync::CancellationToken {
        let cancel = tokio_util::sync::CancellationToken::new();
        commands
            .send(Traced::untraced(Command::RunTask {
                generation,
                task,
                cancel: cancel.clone(),
            }))
            .expect("command channel should accept");
        cancel
    }

    #[tokio::test]
    async fn chunks_and_done_flow_back_in_order() {
        let engine = MockEngine::new().with_chunks(vec![
            Ok("你".into()),
            Ok("好".into()),
            Ok("\n```gloss\n{\"title\":\"问候\"}\n```".into()),
        ]);
        let (commands, events, runtime) = start(&engine);

        run(&commands, 7, text_task("hello"));

        assert!(matches!(
            events.recv().unwrap(),
            Event::TaskChunk { generation: 7, ref delta } if delta == "你"
        ));
        assert!(matches!(
            events.recv().unwrap(),
            Event::TaskChunk { generation: 7, ref delta } if delta == "好"
        ));
        assert!(matches!(
            events.recv().unwrap(),
            Event::TaskChunk { generation: 7, ref delta } if delta.contains("gloss")
        ));
        match events.recv().unwrap() {
            Event::TaskDone {
                generation,
                outcome,
            } => {
                assert_eq!(generation, 7);
                assert_eq!(outcome.body, "你好");
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

        let cancel = run(&commands, 1, text_task("slow"));
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

        run(&commands, 3, text_task("boom"));

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
    async fn image_task_without_a_model_fails_before_the_engine() {
        let engine = MockEngine::new();
        let (commands, events, runtime) = start(&engine);

        let task = Task {
            kind: TaskKind::ImageOcr,
            input: TaskInput::Text {
                text: "irrelevant".into(),
                hint: None,
            },
            options: TaskOptions::default(),
        };
        run(&commands, 1, task);

        assert!(
            matches!(
                events.recv().unwrap(),
                Event::TaskFailed {
                    generation: 1,
                    error: GlossError::Config(_)
                }
            ),
            "missing model must be reported as a config failure"
        );
        assert_eq!(engine.call_count(), 0, "engine must not be called");
        drop(commands);
        tokio::task::spawn_blocking(move || drop(runtime))
            .await
            .expect("shutdown");
    }

    #[tokio::test]
    async fn background_panic_becomes_task_failed_and_the_loop_survives() {
        let engine = MockEngine::new().with_execute_panic();
        let (commands, events, runtime) = start(&engine);

        run(&commands, 1, text_task("boom"));
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
        run(&commands, 2, text_task("again"));
        loop {
            match events.recv().unwrap() {
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
