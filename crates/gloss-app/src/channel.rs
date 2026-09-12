//! 线程间消息与通道（架构文档 §4.3）：四条通道 + 一个取消信号。
//!
//! 请求代数（字段名 `generation`，文档记作 gen——`gen` 是 Rust 2024 保留字）只在
//! App 一处赋值：`PlatformEvent` 不含它，其后所有消息携带同一个值，主线程对
//! 不匹配的回传事件静默丢弃。取消不走通道——`CancellationToken` 随 `RunTask`
//! 下发 clone，取消立即生效且覆盖多个 await 点。

use crossbeam_channel::{Receiver, Sender, unbounded};
use gloss_core::model::{GlossError, ScreenRect};
use gloss_core::task::{HotkeyBinding, Task, TaskInput, TaskKind, TaskOutcome};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio_util::sync::CancellationToken;

/// ① 平台事件源 → 主线程。专用事件线程产生；不含请求代数——由 App 收到后统一赋值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlatformEvent {
    /// 热键触发；绑定 = 任务类型 + 输入源。
    HotkeyTriggered {
        /// 触发的热键绑定。
        binding: HotkeyBinding,
    },
    /// 划词手势（文本任务）。
    SelectionGesture,
    /// 框选手势（图像任务）。
    RegionGesture {
        /// 框选区域的屏幕坐标。
        rect: ScreenRect,
    },
    /// 托盘/热键请求打开设置。
    OpenSettingsRequested,
    /// 托盘/热键请求退出应用。
    QuitRequested,
}

/// ② 主线程 → 平台事件线程：触发取材。
///
/// 取材有线程亲和约束（选区/截图是同步系统调用，必须发生在事件线程），
/// 不能与通道③合并发去 tokio；与系统事件同线程顺序消费，天然串行无锁。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcquireCommand {
    /// 读前台应用选区文本。
    AcquireText {
        /// 请求代数，由 App 统一赋值（见模块文档）。
        generation: u64,
        /// 本次文本任务的任务类型。
        kind: TaskKind,
    },
    /// 截取屏幕区域为图像。
    CaptureRegion {
        /// 请求代数，由 App 统一赋值（见模块文档）。
        generation: u64,
        /// 框选区域的屏幕坐标。
        rect: ScreenRect,
        /// 本次图像任务的任务类型。
        kind: TaskKind,
    },
}

/// ③ 主线程 → tokio 后台：推理任务（携带完整数据与取消令牌）。
///
/// 用 tokio unbounded mpsc：消费端 async `recv().await`；主线程侧 `send`
/// 永不阻塞、也不需要定容量策略——命令是轻量枚举，产量受用户手势限制。
#[derive(Debug, Clone)]
pub enum Command {
    /// 执行一条完整任务。
    RunTask {
        /// 请求代数，由 App 统一赋值（见模块文档）。
        generation: u64,
        /// 任务全量数据（类型 + 输入 + 选项）。
        task: Task,
        /// 取消令牌：App 为每次任务创建并 clone 下发；取消 = 调 `cancel()`。
        cancel: CancellationToken,
    },
}

/// ④ 平台事件线程 / tokio → 主线程：取材与推理的回传。
///
/// crossbeam MPMC——事件线程与 tokio 各持一个 Sender；主线程在渲染循环里
/// `try_recv` 非阻塞消费。
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// 取材产物，触发→取材→推理链路的枢纽。
    InputReady {
        /// 请求代数，与触发它的命令同值。
        generation: u64,
        /// 取材产物。
        input: TaskInput,
    },
    /// 流式增量（markdown 正文片段）。
    TaskChunk {
        /// 请求代数，与触发它的任务同值。
        generation: u64,
        /// markdown 正文增量片段。
        delta: String,
    },
    /// 任务完成。
    TaskDone {
        /// 请求代数，与触发它的任务同值。
        generation: u64,
        /// 任务产物。
        outcome: TaskOutcome,
    },
    /// 任务失败。
    TaskFailed {
        /// 请求代数，与触发它的任务同值。
        generation: u64,
        /// 失败原因，状态机据此分支重试策略。
        error: GlossError,
    },
}

/// crossbeam 无界通道对：①②④ 共用这一选型（MPMC + 主线程 `try_recv` 非阻塞）。
pub struct CrossbeamPair<T> {
    /// 发送端，可克隆（MPMC）。
    pub tx: Sender<T>,
    /// 接收端，主线程用 `try_recv` 非阻塞消费。
    pub rx: Receiver<T>,
}

impl<T> CrossbeamPair<T> {
    /// 创建一对无界通道。
    pub fn new() -> Self {
        let (tx, rx) = unbounded();
        Self { tx, rx }
    }
}

impl<T> Default for CrossbeamPair<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// 通道③的 tokio half：Receiver 移交 tokio 消费任务，Sender 留在主线程。
pub struct CommandChannel {
    /// 发送端，留在主线程。
    pub tx: UnboundedSender<Command>,
    /// 接收端，移交 tokio 消费任务。
    pub rx: UnboundedReceiver<Command>,
}

impl CommandChannel {
    /// 创建一对 tokio 无界 mpsc 通道。
    pub fn new() -> Self {
        let (tx, rx) = unbounded_channel();
        Self { tx, rx }
    }
}

impl Default for CommandChannel {
    fn default() -> Self {
        Self::new()
    }
}

/// 四条通道的组装点：入口 `main.rs` 创建一次并分发——① 的 Sender 交给平台
/// 事件源；② 的 Sender 留在 App；③ 的 Receiver 移交 tokio；④ 的 Sender
/// 克隆给事件线程与 tokio 各一。
pub struct Channels {
    /// 通道①：平台事件源 → 主线程。
    pub platform_events: CrossbeamPair<PlatformEvent>,
    /// 通道②：主线程 → 平台事件线程（取材命令）。
    pub acquire_commands: CrossbeamPair<AcquireCommand>,
    /// 通道③：主线程 → tokio（推理任务）。
    pub commands: CommandChannel,
    /// 通道④：事件线程 / tokio → 主线程（取材与推理回传）。
    pub events: CrossbeamPair<Event>,
}

impl Channels {
    /// 创建四条通道。
    pub fn new() -> Self {
        Self {
            platform_events: CrossbeamPair::new(),
            acquire_commands: CrossbeamPair::new(),
            commands: CommandChannel::new(),
            events: CrossbeamPair::new(),
        }
    }
}

impl Default for Channels {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use gloss_core::task::{InputHint, InputSource, OutcomeStructured, Sense, TaskOptions};

    fn selection_binding() -> HotkeyBinding {
        HotkeyBinding {
            trigger: "Cmd+Shift+1".into(),
            kind: TaskKind::TranslateWord,
            source: InputSource::Selection,
        }
    }

    fn sample_task() -> Task {
        Task {
            kind: TaskKind::TranslateWord,
            input: TaskInput::Text {
                text: "gloss".into(),
                hint: Some(InputHint::CodeLanguage("rust".into())),
            },
            options: TaskOptions::default(),
        }
    }

    #[test]
    fn platform_events_round_trip_through_crossbeam() {
        let ch = CrossbeamPair::<PlatformEvent>::new();
        let events = vec![
            PlatformEvent::HotkeyTriggered {
                binding: selection_binding(),
            },
            PlatformEvent::SelectionGesture,
            PlatformEvent::RegionGesture {
                rect: ScreenRect {
                    x: 10,
                    y: 20,
                    width: 300,
                    height: 200,
                },
            },
            PlatformEvent::OpenSettingsRequested,
            PlatformEvent::QuitRequested,
        ];
        for e in &events {
            ch.tx.send(e.clone()).unwrap();
        }
        for expected in events {
            assert_eq!(ch.rx.recv().unwrap(), expected);
        }
    }

    /// PlatformEvent 不含请求代数：generation 由 App 在消费时赋值（架构文档 §4.2）。
    #[test]
    fn acquire_commands_carry_app_assigned_gen() {
        let ch = CrossbeamPair::<AcquireCommand>::new();
        let commands = vec![
            AcquireCommand::AcquireText {
                generation: 1,
                kind: TaskKind::TranslateSentence,
            },
            AcquireCommand::CaptureRegion {
                generation: 2,
                rect: ScreenRect {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
                kind: TaskKind::ImageOcr,
            },
        ];
        for c in &commands {
            ch.tx.send(c.clone()).unwrap();
        }
        for expected in commands {
            assert_eq!(ch.rx.recv().unwrap(), expected);
        }
    }

    /// 通道③走 tokio mpsc：取消令牌随任务下发，App 侧 cancel 对接收侧立即可见。
    #[test]
    fn run_task_command_delivers_cancellable_task() {
        let mut ch = CommandChannel::new();
        let cancel = CancellationToken::new();
        ch.tx
            .send(Command::RunTask {
                generation: 7,
                task: sample_task(),
                cancel: cancel.clone(),
            })
            .unwrap();

        let Command::RunTask {
            generation,
            task,
            cancel: received,
        } = ch.rx.blocking_recv().unwrap();
        assert_eq!(generation, 7);
        assert_eq!(task, sample_task());
        assert!(!received.is_cancelled());
        cancel.cancel();
        assert!(received.is_cancelled());
    }

    /// 通道④ MPMC：事件线程与 tokio 各持一个 Sender，主线程按到达顺序消费。
    #[test]
    fn event_channel_supports_dual_senders() {
        let ch = CrossbeamPair::<Event>::new();
        let tokio_side = ch.tx.clone();

        ch.tx
            .send(Event::InputReady {
                generation: 1,
                input: TaskInput::Text {
                    text: "hello".into(),
                    hint: None,
                },
            })
            .unwrap();
        tokio_side
            .send(Event::TaskDone {
                generation: 1,
                outcome: TaskOutcome {
                    kind: TaskKind::TranslateWord,
                    body: "# gloss".into(),
                    structured: OutcomeStructured::WordCard {
                        word: "gloss".into(),
                        phonetic: Some("/ɡlɒs/".into()),
                        senses: vec![Sense {
                            pos: Some("n.".into()),
                            meaning: "光泽；注释".into(),
                            examples: vec![],
                        }],
                    },
                },
            })
            .unwrap();

        assert!(matches!(
            ch.rx.recv().unwrap(),
            Event::InputReady { generation: 1, .. }
        ));
        assert!(matches!(
            ch.rx.recv().unwrap(),
            Event::TaskDone { generation: 1, .. }
        ));
    }

    #[test]
    fn event_channel_carries_stream_chunks_and_failures() {
        let ch = CrossbeamPair::<Event>::new();
        ch.tx
            .send(Event::TaskChunk {
                generation: 3,
                delta: "光泽".into(),
            })
            .unwrap();
        ch.tx
            .send(Event::TaskFailed {
                generation: 3,
                error: GlossError::EngineRateLimited,
            })
            .unwrap();
        assert_eq!(
            ch.rx.recv().unwrap(),
            Event::TaskChunk {
                generation: 3,
                delta: "光泽".into()
            }
        );
        assert_eq!(
            ch.rx.recv().unwrap(),
            Event::TaskFailed {
                generation: 3,
                error: GlossError::EngineRateLimited
            }
        );
    }

    /// 主线程渲染循环用 try_recv 非阻塞拉取：空队列返回 Empty 而不是挂起。
    #[test]
    fn try_recv_on_empty_queue_returns_empty() {
        let ch: CrossbeamPair<Event> = CrossbeamPair::new();
        assert!(matches!(
            ch.rx.try_recv(),
            Err(crossbeam_channel::TryRecvError::Empty)
        ));
    }

    /// 通道销毁语义：全部 Sender drop 后 try_recv 报 Disconnected，主线程据此可安全收尾。
    #[test]
    fn try_recv_after_senders_dropped_reports_disconnected() {
        let ch: CrossbeamPair<Event> = CrossbeamPair::new();
        ch.tx
            .send(Event::TaskChunk {
                generation: 1,
                delta: "x".into(),
            })
            .unwrap();
        drop(ch.tx);
        assert_eq!(
            ch.rx.recv().unwrap(),
            Event::TaskChunk {
                generation: 1,
                delta: "x".into()
            }
        );
        assert!(matches!(
            ch.rx.try_recv(),
            Err(crossbeam_channel::TryRecvError::Disconnected)
        ));
    }

    /// 组装点一次性建齐四条通道；跨通道互不串扰（各收各的）。
    #[test]
    fn channels_bundles_all_four() {
        let mut channels = Channels::new();
        channels
            .platform_events
            .tx
            .send(PlatformEvent::SelectionGesture)
            .unwrap();
        channels
            .acquire_commands
            .tx
            .send(AcquireCommand::AcquireText {
                generation: 1,
                kind: TaskKind::ExplainCode,
            })
            .unwrap();
        channels
            .commands
            .tx
            .send(Command::RunTask {
                generation: 1,
                task: sample_task(),
                cancel: CancellationToken::new(),
            })
            .unwrap();
        channels
            .events
            .tx
            .send(Event::TaskFailed {
                generation: 1,
                error: GlossError::SelectionUnavailable,
            })
            .unwrap();

        assert_eq!(
            channels.platform_events.rx.recv().unwrap(),
            PlatformEvent::SelectionGesture
        );
        assert_eq!(
            channels.acquire_commands.rx.recv().unwrap(),
            AcquireCommand::AcquireText {
                generation: 1,
                kind: TaskKind::ExplainCode
            }
        );
        assert!(matches!(
            channels.commands.rx.blocking_recv().unwrap(),
            Command::RunTask { generation: 1, .. }
        ));
        assert_eq!(
            channels.events.rx.recv().unwrap(),
            Event::TaskFailed {
                generation: 1,
                error: GlossError::SelectionUnavailable
            }
        );
    }

    /// ④ 的 Sender 是 MPMC：克隆出的发送端与原型等价（tokio 侧持有一份）。
    #[test]
    fn event_sender_clone_is_independent() {
        let ch = CrossbeamPair::<Event>::new();
        let cloned = ch.tx.clone();
        drop(ch.tx);
        cloned
            .send(Event::InputReady {
                generation: 9,
                input: TaskInput::Image {
                    png: Arc::from(&b"png"[..]),
                    region: ScreenRect {
                        x: 0,
                        y: 0,
                        width: 1,
                        height: 1,
                    },
                },
            })
            .unwrap();
        assert!(matches!(
            ch.rx.recv().unwrap(),
            Event::InputReady { generation: 9, .. }
        ));
    }
}
