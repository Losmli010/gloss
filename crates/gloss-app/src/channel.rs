//! 线程间消息与通道：四条通道 + 一个取消信号。
//!
//! 请求代数（字段名 `generation`，文档记作 gen——`gen` 是 Rust 2024 保留字）只在
//! App 一处赋值：`PlatformEvent` 不含它，其后所有消息携带同一个值，主线程对
//! 不匹配的回传事件静默丢弃。下发方向（②③）的载荷还带着触发点的任务 span
//! （见 [`Traced`]），接收线程的日志因此自动带上代数。取消不走通道——`CancellationToken` 随 `RunTask`
//! 下发 clone，取消立即生效且覆盖多个 await 点。

use crossbeam_channel::{Receiver, Sender, unbounded};
use gloss_core::log::Span;
use gloss_core::model::{GlossError, ScreenPoint};
use gloss_core::task::{TaskInput, TaskKind, TaskOptions, TaskOutcome};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlatformEvent {
    SelectionGesture { pos: ScreenPoint },
    PasteboardImageObserved,
    OpenSettingsRequested,
    AccessibilityGranted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcquireCommand {
    AcquireText { generation: u64 },
    AcquireClipboardImage { generation: u64 },
}

impl AcquireCommand {
    pub fn generation(&self) -> u64 {
        match self {
            Self::AcquireText { generation } | Self::AcquireClipboardImage { generation } => {
                *generation
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum Command {
    RunTask {
        generation: u64,
        input: TaskInput,
        options: TaskOptions,
        cancel: CancellationToken,
    },
    PrewarmSecret {
        keychain_id: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    InputReady {
        generation: u64,
        input: TaskInput,
    },
    TaskClassified {
        generation: u64,
        kind: TaskKind,
    },
    TaskChunk {
        generation: u64,
        delta: String,
    },
    TaskRepairing {
        generation: u64,
    },
    TaskDone {
        generation: u64,
        outcome: TaskOutcome,
    },
    TaskFailed {
        generation: u64,
        error: GlossError,
    },
    SecretPrewarmed {
        result: Result<(), GlossError>,
    },
}

#[derive(Debug, Clone)]
pub struct Traced<T> {
    pub payload: T,
    pub span: Span,
}

impl<T> Traced<T> {
    pub fn untraced(payload: T) -> Self {
        Self {
            payload,
            span: Span::none(),
        }
    }
}

pub struct CrossbeamPair<T> {
    pub tx: Sender<T>,
    pub rx: Receiver<T>,
}

impl<T> CrossbeamPair<T> {
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

pub struct CommandChannel {
    pub tx: UnboundedSender<Traced<Command>>,
    pub rx: UnboundedReceiver<Traced<Command>>,
}

impl CommandChannel {
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

pub struct Channels {
    pub platform_events: CrossbeamPair<PlatformEvent>,
    pub acquire_commands: CrossbeamPair<Traced<AcquireCommand>>,
    pub commands: CommandChannel,
    pub events: CrossbeamPair<Event>,
}

impl Channels {
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

pub struct AppEndpoints {
    pub platform_events: Receiver<PlatformEvent>,
    pub acquire_commands: Sender<Traced<AcquireCommand>>,
    pub commands: UnboundedSender<Traced<Command>>,
    pub events: Receiver<Event>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use gloss_core::task::OutcomeStructured;

    fn sample_input() -> TaskInput {
        TaskInput::Text {
            text: "gloss".into(),
        }
    }

    fn sample_options() -> TaskOptions {
        TaskOptions {
            target_lang: Some(gloss_core::model::Lang::Zh),
            ..TaskOptions::default()
        }
    }

    #[test]
    fn platform_events_round_trip_through_crossbeam() {
        let ch = CrossbeamPair::<PlatformEvent>::new();
        let events = vec![
            PlatformEvent::SelectionGesture {
                pos: ScreenPoint::new(30, 40),
            },
            PlatformEvent::PasteboardImageObserved,
            PlatformEvent::OpenSettingsRequested,
            PlatformEvent::AccessibilityGranted,
        ];
        for e in &events {
            ch.tx.send(e.clone()).unwrap();
        }
        for expected in events {
            assert_eq!(ch.rx.recv().unwrap(), expected);
        }
    }

    #[test]
    fn acquire_commands_carry_app_assigned_gen() {
        let ch = CrossbeamPair::<Traced<AcquireCommand>>::new();
        let commands = vec![
            AcquireCommand::AcquireText { generation: 1 },
            AcquireCommand::AcquireClipboardImage { generation: 2 },
        ];
        for c in &commands {
            ch.tx.send(Traced::untraced(c.clone())).unwrap();
        }
        for expected in commands {
            assert_eq!(ch.rx.recv().unwrap().payload, expected);
        }
    }

    #[test]
    fn run_task_command_delivers_cancellable_task() {
        let mut ch = CommandChannel::new();
        let cancel = CancellationToken::new();
        ch.tx
            .send(Traced::untraced(Command::RunTask {
                generation: 7,
                input: sample_input(),
                options: sample_options(),
                cancel: cancel.clone(),
            }))
            .unwrap();

        let Command::RunTask {
            generation,
            input,
            options,
            cancel: received,
        } = ch.rx.blocking_recv().unwrap().payload
        else {
            panic!("expected a RunTask command, got another variant")
        };
        assert_eq!(generation, 7);
        assert_eq!(input, sample_input());
        assert_eq!(options, sample_options());
        assert!(!received.is_cancelled());
        cancel.cancel();
        assert!(received.is_cancelled());
    }

    #[test]
    fn prewarm_secret_command_carries_only_the_entry_id() {
        let mut ch = CommandChannel::new();
        ch.tx
            .send(Traced::untraced(Command::PrewarmSecret {
                keychain_id: "gloss/deepseek".into(),
            }))
            .unwrap();

        let received = ch.rx.blocking_recv().unwrap().payload;
        assert!(
            matches!(
                received,
                Command::PrewarmSecret { ref keychain_id } if keychain_id == "gloss/deepseek"
            ),
            "the prewarm command carries the entry id only, got {received:?}"
        );
    }

    #[test]
    fn secret_prewarmed_receipt_round_trips_without_a_value() {
        let ch = CrossbeamPair::<Event>::new();
        ch.tx
            .send(Event::SecretPrewarmed { result: Ok(()) })
            .unwrap();
        ch.tx
            .send(Event::SecretPrewarmed {
                result: Err(GlossError::EngineAuth),
            })
            .unwrap();
        assert!(matches!(
            ch.rx.recv().unwrap(),
            Event::SecretPrewarmed { result: Ok(()) }
        ));
        assert!(matches!(
            ch.rx.recv().unwrap(),
            Event::SecretPrewarmed {
                result: Err(GlossError::EngineAuth)
            }
        ));
    }

    #[test]
    fn event_channel_supports_dual_senders() {
        let ch = CrossbeamPair::<Event>::new();
        let tokio_side = ch.tx.clone();

        ch.tx
            .send(Event::InputReady {
                generation: 1,
                input: TaskInput::Text {
                    text: "hello".into(),
                },
            })
            .unwrap();
        tokio_side
            .send(Event::TaskDone {
                generation: 1,
                outcome: TaskOutcome {
                    kind: TaskKind::TranslateWord,
                    note: "# gloss".into(),
                    structured: OutcomeStructured::WordCard {
                        phonetic: Some("/ɡlɒs/".into()),
                        examples: vec![],
                    },
                    degraded: false,
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
        ch.tx.send(Event::TaskRepairing { generation: 3 }).unwrap();
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
            Event::TaskRepairing { generation: 3 },
            "the repair marker rides channel 4 without a round count"
        );
        assert_eq!(
            ch.rx.recv().unwrap(),
            Event::TaskFailed {
                generation: 3,
                error: GlossError::EngineRateLimited
            }
        );
    }

    #[test]
    fn try_recv_on_empty_queue_returns_empty() {
        let ch: CrossbeamPair<Event> = CrossbeamPair::new();
        assert!(matches!(
            ch.rx.try_recv(),
            Err(crossbeam_channel::TryRecvError::Empty)
        ));
    }

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

    #[test]
    fn channels_bundles_all_four() {
        let mut channels = Channels::new();
        send_one_of_each(&mut channels);

        assert_eq!(
            channels.platform_events.rx.recv().unwrap(),
            PlatformEvent::SelectionGesture {
                pos: ScreenPoint::new(30, 40)
            }
        );
        assert_eq!(
            channels.acquire_commands.rx.recv().unwrap().payload,
            AcquireCommand::AcquireText { generation: 1 }
        );
        assert!(matches!(
            channels.commands.rx.blocking_recv().unwrap().payload,
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

    fn send_one_of_each(channels: &mut Channels) {
        channels
            .platform_events
            .tx
            .send(PlatformEvent::SelectionGesture {
                pos: ScreenPoint::new(30, 40),
            })
            .unwrap();
        channels
            .acquire_commands
            .tx
            .send(Traced::untraced(AcquireCommand::AcquireText {
                generation: 1,
            }))
            .unwrap();
        channels
            .commands
            .tx
            .send(Traced::untraced(Command::RunTask {
                generation: 1,
                input: sample_input(),
                options: sample_options(),
                cancel: CancellationToken::new(),
            }))
            .unwrap();
        channels
            .events
            .tx
            .send(Event::TaskFailed {
                generation: 1,
                error: GlossError::SelectionUnavailable,
            })
            .unwrap();
    }

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
                },
            })
            .unwrap();
        assert!(matches!(
            ch.rx.recv().unwrap(),
            Event::InputReady { generation: 9, .. }
        ));
    }
}
