//! 通道③ → [`AiTaskService`] → 通道④ 的 tokio 消费桥（推理在
//! tokio 后台，主线程不 await）。
//!
//! 每条 `RunTask` 的取消令牌经 `select!` 与 execute 竞速——取消在流式
//! 读取的多个 await 点上即时生效，被取消的任务静默丢弃（App 已推进代
//! 数，任何迟到产物都会被判 stale）。任务 future 包在 `catch_unwind`
//! 里（后台 panic 被 tokio 捕获转为 TaskFailed）：引擎或编排
//! 层炸掉时用户拿到失败卡而不是永悬的「推理中」，消费循环继续存活。
//! 运行时由 [`start_command_runtime`] 创建并托管，进程退出时随通道③
//! 关闭自然收尾。

use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::Sender;
use futures::FutureExt;
use gloss_core::config::DEFAULT_TEXT_MODEL;
use gloss_core::engine::AiTaskService;
use gloss_core::log::{debug, thread, warn};
use gloss_core::model::GlossError;
use gloss_core::task::Task;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::channel::{Command, Event};

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

/// 创建 tokio 运行时并启动消费循环。`wake` 在每条回传事件入队后调用，
/// 唤醒睡在主线程事件循环里的 UI。
pub fn start_command_runtime(
    service: Arc<AiTaskService>,
    commands: UnboundedReceiver<Command>,
    events: Sender<Event>,
    wake: impl Fn() + Send + Sync + 'static,
) -> std::io::Result<CommandRuntime> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.spawn(consume_loop(service, commands, events, wake));
    Ok(CommandRuntime { rt: Some(rt) })
}

/// 消费循环：抽干通道③，逐条执行并回传。通道③全部 Sender 关闭（App
/// 随事件循环结束 drop）后循环结束。
async fn consume_loop(
    service: Arc<AiTaskService>,
    mut commands: UnboundedReceiver<Command>,
    events: Sender<Event>,
    wake: impl Fn() + Send + Sync,
) {
    while let Some(command) = commands.recv().await {
        // Command 当前只有 RunTask 一个变体，直接解构。
        let Command::RunTask {
            generation,
            task,
            cancel,
        } = command;
        let model = match model_for(&task) {
            Ok(model) => model,
            Err(error) => {
                // 没有可用模型（图像任务未配视觉模型）：明确失败，不拿文本模型
                // 去接图像任务——用户看到的是「去设置页配模型」，而不是服务端
                // 400 的转述。
                send_event(&events, &wake, Event::TaskFailed { generation, error });
                continue;
            }
        };
        // 取消与 execute 竞速：取消即时生效，覆盖 execute 内部的全部
        // await 点（渲染、缓存、流式读取）。被取消的任务不发任何回传——
        // App 取消时已 gen+1，迟到产物本就该被丢弃。
        //
        // execute 包在 catch_unwind 里：后台 panic 转 TaskFailed（错误
        // 卡给用户「服务异常」而不是永悬的推理中），循环自身继续消费。
        tokio::select! {
            _ = cancel.cancelled() => {
                debug!(
                    thread = thread::TOKIO,
                    generation = generation,
                    "task cancelled, result dropped"
                );
            }
            outcome = std::panic::AssertUnwindSafe(service.execute(&task, model, |delta| {
                send_event(&events, &wake, Event::TaskChunk { generation, delta });
            }))
            .catch_unwind() => match outcome {
                Ok(Ok(outcome)) => send_event(&events, &wake, Event::TaskDone { generation, outcome }),
                Ok(Err(error)) => send_event(&events, &wake, Event::TaskFailed { generation, error }),
                Err(payload) => {
                    let detail = panic_detail(&payload);
                    warn!(
                        thread = thread::TOKIO,
                        generation = generation,
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
    debug!(
        thread = thread::TOKIO,
        "command channel closed, consumer exits"
    );
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
/// 留空表示该 kind 没有可用模型——图像类未配视觉模型时就是这种情形（M5-T4
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

    use crossbeam_channel::Receiver;
    use gloss_core::cache::MokaCache;
    use gloss_core::engine::AiTaskService;
    use gloss_core::engine::mock::MockEngine;
    use gloss_core::model::GlossError;
    use gloss_core::task::{Task, TaskInput, TaskKind, TaskOptions};
    use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

    use super::{CommandRuntime, Event, start_command_runtime};
    use crate::channel::Command;

    fn start(engine: &MockEngine) -> (UnboundedSender<Command>, Receiver<Event>, CommandRuntime) {
        let service = Arc::new(AiTaskService::new(
            Arc::new(engine.clone()) as Arc<dyn gloss_core::ports::AiEngine>,
            Arc::new(MokaCache::new()),
        ));
        let (commands_tx, commands_rx) = unbounded_channel();
        let (events_tx, events_rx) = crossbeam_channel::unbounded();
        let runtime = start_command_runtime(service, commands_rx, events_tx, || {})
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
        commands: &UnboundedSender<Command>,
        generation: u64,
        task: Task,
    ) -> tokio_util::sync::CancellationToken {
        let cancel = tokio_util::sync::CancellationToken::new();
        commands
            .send(Command::RunTask {
                generation,
                task,
                cancel: cancel.clone(),
            })
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
