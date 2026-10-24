//! 状态机的状态词汇表：稳定态、浮层视图、失败语义与在途探测的载荷类型。
//! 决策逻辑归状态机本体与策略模块，这里只有数据形状。

use tokio_util::sync::CancellationToken;

use gloss_core::guard::SensitiveKind;
use gloss_core::model::GlossError;
use gloss_core::task::{TaskInput, TaskKind, TaskOptions, TaskOutcome};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorAction {
    Retry,
    OpenSettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppState {
    #[default]
    Idle,
    Translating,
    Show,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OverlayView {
    Streaming {
        source: String,
        raw: String,
        classified: Option<TaskKind>,
        repairing: bool,
    },
    Outcome {
        source: String,
        outcome: TaskOutcome,
    },
    Failed {
        cause: FailureCause,
        action: Option<ErrorAction>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum FailureCause {
    Task(GlossError),
    TransportChannel,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputOutcome {
    Dispatch(RunRequest),
    Blocked(SensitiveKind),
    Ignored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureOutcome {
    Shown,
    SilentlyDropped,
    Ignored,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunRequest {
    pub generation: u64,
    pub input: TaskInput,
    pub options: TaskOptions,
    pub cancel: CancellationToken,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct PendingOptions(pub(super) TaskOptions);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProbeSource {
    Selection,
    PasteboardImage,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ProbeTask {
    pub(super) id: u64,
    pub(super) pending: PendingOptions,
    pub(super) source: ProbeSource,
}
