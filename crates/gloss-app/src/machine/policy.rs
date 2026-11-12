//! 显示与触发策略（纯函数）：平台事件的去向判定、错误→动作映射、任务
//! 选项构造与浮层露面策略。状态迁移归状态机本体，这里只回答“要不要
//! 与去哪里”。

use std::sync::Arc;

use gloss_core::config::Config;
use gloss_core::guard::{self, SceneFacts, TriggerBlock};
use gloss_core::model::{GlossError, Locale};
use gloss_core::task::{TaskInput, TaskOptions};

use crate::channel::{Event, PlatformEvent};
use crate::machine::types::{ErrorAction, OverlayView, ProbeSource};

pub(super) fn error_action(error: &GlossError) -> Option<ErrorAction> {
    match error {
        GlossError::EngineNetwork | GlossError::EngineRateLimited => Some(ErrorAction::Retry),
        GlossError::EngineAuth
        | GlossError::UnsupportedModality
        | GlossError::JsonModeUnsupported
        | GlossError::Config(_) => Some(ErrorAction::OpenSettings),
        _ => None,
    }
}

pub(super) fn source_text(input: &TaskInput) -> String {
    match input {
        TaskInput::Text { text } => text.clone(),
        TaskInput::Image { .. } => String::new(),
    }
}

pub(super) fn streaming_view(input: &TaskInput) -> OverlayView {
    OverlayView::Streaming {
        source: source_text(input),
        raw: String::new(),
        classified: None,
        repairing: false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerDecision {
    Acquire,
    Blocked(TriggerBlock),
    SelfSuppressed,
    Unwired,
}

pub(super) fn trigger_decision_for(source: ProbeSource, scene: &SceneFacts) -> TriggerDecision {
    match source {
        ProbeSource::Selection => {
            if scene.front_app.as_ref().is_some_and(|app| app.is_self) {
                return TriggerDecision::SelfSuppressed;
            }
            match guard::trigger_block(scene) {
                Some(block) => TriggerDecision::Blocked(block),
                None => TriggerDecision::Acquire,
            }
        }
        ProbeSource::PasteboardImage => match guard::trigger_block(scene) {
            Some(block) => TriggerDecision::Blocked(block),
            None => TriggerDecision::Acquire,
        },
    }
}

pub fn trigger_decision(event: &PlatformEvent, scene: &SceneFacts) -> TriggerDecision {
    match event {
        PlatformEvent::SelectionGesture { .. } => {
            trigger_decision_for(ProbeSource::Selection, scene)
        }
        PlatformEvent::PasteboardImageObserved => {
            trigger_decision_for(ProbeSource::PasteboardImage, scene)
        }
        PlatformEvent::OpenSettingsRequested | PlatformEvent::AccessibilityGranted => {
            TriggerDecision::Unwired
        }
    }
}

pub(crate) fn task_options(config: &Config, system_locale: Locale) -> TaskOptions {
    TaskOptions {
        target_lang: Some(config.target_lang),
        model: Arc::from(config.model.as_str()),
        prompt_locale: Some(config.language.resolve(system_locale)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EventKind {
    InputReady,
    TaskClassified,
    TaskChunk,
    TaskRepairing,
    TaskDone,
    TaskFailed,
    SecretPrewarmed,
}

pub(crate) fn event_kind(event: &Event) -> EventKind {
    match event {
        Event::InputReady { .. } => EventKind::InputReady,
        Event::TaskClassified { .. } => EventKind::TaskClassified,
        Event::TaskChunk { .. } => EventKind::TaskChunk,
        Event::TaskRepairing { .. } => EventKind::TaskRepairing,
        Event::TaskDone { .. } => EventKind::TaskDone,
        Event::TaskFailed { .. } => EventKind::TaskFailed,
        Event::SecretPrewarmed { .. } => EventKind::SecretPrewarmed,
    }
}

pub(super) fn auto_show_for(kind: EventKind, accepted: bool) -> bool {
    match kind {
        EventKind::InputReady => false,
        EventKind::TaskFailed => accepted,
        EventKind::TaskClassified
        | EventKind::TaskRepairing
        | EventKind::TaskDone
        | EventKind::TaskChunk => false,
        EventKind::SecretPrewarmed => false,
    }
}

pub(crate) fn auto_show_after(batch: impl IntoIterator<Item = (EventKind, bool)>) -> bool {
    batch
        .into_iter()
        .any(|(kind, accepted)| auto_show_for(kind, accepted))
}

pub(crate) fn should_reveal(
    pending: bool,
    has_view: bool,
    batch: impl IntoIterator<Item = (EventKind, bool)>,
) -> bool {
    (pending && has_view) || auto_show_after(batch)
}
