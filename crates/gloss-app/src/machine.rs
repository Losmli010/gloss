//! 任务状态机与浮层视图（函数式内核）：纯状态转移，不做输入输出（IO）。
//! 通道②③由壳发送——去向分别是平台事件线程与后台运行时（tokio）；通道①④由壳
//! 消费——来源分别是平台事件与回传事件；浮层显隐与日志亦在壳侧。两段式「探测—提交」：
//! 探测只领编号、冻结任务选项，不改状态也不取消在途任务；产物到达才提交——编号提升为代数、
//! 旧任务取消让位、视图换流式卡。代数只在提交路径赋值，
//! 回传按它过滤陈旧。敏感闸门两处：场景闸门在触发决策（触发前），内容闸门在
//! 提交的文本分支（下发前，命中保留当前显示，图像没有可扫描文本故不设闸门）。

mod policy;
mod types;

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use gloss_core::config::Config;
use gloss_core::guard::{self, SceneFacts};
use gloss_core::model::GlossError;
use gloss_core::model::Locale;
use gloss_core::task::{TaskInput, TaskKind, TaskOptions, TaskOutcome};

use crate::channel::{AcquireCommand, PlatformEvent};
use policy::{error_action, streaming_view, task_options};
use types::{PendingOptions, ProbeSource, ProbeTask};

pub use policy::{TriggerDecision, trigger_decision};
pub(crate) use policy::{event_kind, should_reveal};
pub use types::{
    AppState, ErrorAction, FailureCause, FailureOutcome, InputOutcome, OverlayView, RunRequest,
};

#[derive(Debug, Default)]
pub struct TaskStateMachine {
    next_id: u64,
    generation: u64,
    probe: Option<ProbeTask>,
    current_cancel: Option<CancellationToken>,
    active_request: Option<(TaskInput, TaskOptions)>,
    attached_image: Option<Arc<[u8]>>,
    overlay_view: Option<OverlayView>,
}

impl TaskStateMachine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn probe_id(&self) -> Option<u64> {
        self.probe.as_ref().map(|probe| probe.id)
    }

    pub fn state(&self) -> AppState {
        match &self.overlay_view {
            None => AppState::Idle,
            Some(OverlayView::Streaming { .. }) => AppState::Translating,
            Some(OverlayView::Outcome { .. }) => AppState::Show,
            Some(OverlayView::Failed { .. }) => AppState::Error,
        }
    }

    pub fn overlay_view(&self) -> Option<&OverlayView> {
        self.overlay_view.as_ref()
    }

    pub fn attached_image(&self) -> Option<&Arc<[u8]>> {
        self.attached_image.as_ref()
    }

    #[cfg(test)]
    pub fn current_cancel(&self) -> Option<&CancellationToken> {
        self.current_cancel.as_ref()
    }

    pub fn begin_selection_probe(
        &mut self,
        event: &PlatformEvent,
        config: &Config,
        system_locale: Locale,
        scene: &SceneFacts,
    ) -> Option<AcquireCommand> {
        if !matches!(event, PlatformEvent::SelectionGesture { .. }) {
            return None;
        }
        match trigger_decision(event, scene) {
            TriggerDecision::Acquire => {}
            TriggerDecision::Blocked(_)
            | TriggerDecision::SelfSuppressed
            | TriggerDecision::Unwired => return None,
        }
        let id = self.next_id + 1;
        self.next_id += 1;
        self.probe = Some(ProbeTask {
            id,
            pending: PendingOptions(task_options(config, system_locale)),
            source: ProbeSource::Selection,
        });
        Some(AcquireCommand::AcquireText { generation: id })
    }

    pub fn begin_pasteboard_probe(
        &mut self,
        event: &PlatformEvent,
        config: &Config,
        system_locale: Locale,
        scene: &SceneFacts,
    ) -> Option<AcquireCommand> {
        if !matches!(event, PlatformEvent::PasteboardImageObserved) {
            return None;
        }
        match trigger_decision(event, scene) {
            TriggerDecision::Acquire => {}
            TriggerDecision::Blocked(_)
            | TriggerDecision::SelfSuppressed
            | TriggerDecision::Unwired => return None,
        }
        let id = self.next_id + 1;
        self.next_id += 1;
        self.probe = Some(ProbeTask {
            id,
            pending: PendingOptions(task_options(config, system_locale)),
            source: ProbeSource::PasteboardImage,
        });
        Some(AcquireCommand::AcquireClipboardImage { generation: id })
    }

    pub fn commit_probe(&mut self, probe_id: u64, input: TaskInput) -> InputOutcome {
        let source = match self.probe.as_ref() {
            Some(probe) if probe.id == probe_id => probe.source,
            _ => return InputOutcome::Ignored,
        };
        let attached = match (&input, source) {
            (TaskInput::Text { text }, ProbeSource::Selection) => {
                if let Some(reason) = guard::detect_sensitive(text) {
                    self.probe = None;
                    return InputOutcome::Blocked(reason);
                }
                None
            }
            (TaskInput::Image { png, .. }, ProbeSource::PasteboardImage) => Some(Arc::clone(png)),
            _ => return InputOutcome::Ignored,
        };
        let Some(ProbeTask {
            id,
            pending: PendingOptions(options),
            ..
        }) = self.probe.take()
        else {
            return InputOutcome::Ignored;
        };
        if let Some(cancel) = self.current_cancel.take() {
            cancel.cancel();
        }
        self.active_request = None;
        self.generation = id;
        self.attached_image = attached;
        InputOutcome::Dispatch(self.begin_run(input, options))
    }

    pub fn commit_probe_failed(&mut self, probe_id: u64, error: &GlossError) -> FailureOutcome {
        let source = match self.probe.as_ref() {
            Some(probe) if probe.id == probe_id => probe.source,
            _ => return FailureOutcome::Ignored,
        };
        self.probe = None;
        let silent = match source {
            ProbeSource::Selection => matches!(
                error,
                GlossError::SelectionUnavailable | GlossError::SelectionEmpty
            ),
            ProbeSource::PasteboardImage => false,
        };
        if silent {
            return FailureOutcome::SilentlyDropped;
        }
        self.land_error(error);
        FailureOutcome::Shown
    }

    pub fn drop_probe(&mut self) {
        self.probe = None;
    }

    fn land_error(&mut self, error: &GlossError) {
        if let Some(cancel) = self.current_cancel.take() {
            cancel.cancel();
        }
        let mut action = error_action(error);
        if action == Some(ErrorAction::Retry) {
            if self.active_request.is_none() {
                action = None;
            }
        } else {
            self.active_request = None;
        }
        self.overlay_view = Some(OverlayView::Failed {
            cause: FailureCause::Task(error.clone()),
            action,
        });
    }

    pub fn accept_chunk(&mut self, generation: u64, delta: String) -> bool {
        if generation != self.generation {
            return false;
        }
        match &mut self.overlay_view {
            Some(OverlayView::Streaming { raw, .. }) => {
                raw.push_str(&delta);
                true
            }
            _ => false,
        }
    }

    pub fn accept_done(&mut self, generation: u64, outcome: TaskOutcome) -> bool {
        if generation != self.generation {
            return false;
        }
        let source = match &mut self.overlay_view {
            Some(OverlayView::Streaming { source, .. }) => std::mem::take(source),
            _ => return false,
        };
        self.current_cancel = None;
        if !outcome.degraded {
            self.active_request = None;
        }
        self.overlay_view = Some(OverlayView::Outcome { source, outcome });
        true
    }

    pub fn accept_classified(&mut self, generation: u64, kind: TaskKind) -> bool {
        if generation != self.generation {
            return false;
        }
        match &mut self.overlay_view {
            Some(OverlayView::Streaming { classified, .. }) => {
                *classified = Some(kind);
                true
            }
            _ => false,
        }
    }

    pub fn accept_repairing(&mut self, generation: u64) -> bool {
        if generation != self.generation {
            return false;
        }
        match &mut self.overlay_view {
            Some(OverlayView::Streaming { raw, repairing, .. }) => {
                raw.clear();
                *repairing = true;
                true
            }
            _ => false,
        }
    }

    pub fn accept_failed(&mut self, generation: u64, error: &GlossError) -> FailureOutcome {
        if generation != self.generation || self.state() != AppState::Translating {
            return FailureOutcome::Ignored;
        }
        self.current_cancel = None;
        self.land_error(error);
        FailureOutcome::Shown
    }

    pub fn retry(&mut self) -> Option<RunRequest> {
        let retryable = matches!(
            self.overlay_view,
            Some(OverlayView::Failed { .. })
                | Some(OverlayView::Outcome {
                    outcome: TaskOutcome { degraded: true, .. },
                    ..
                })
        );
        if !retryable {
            return None;
        }
        let (input, options) = self.active_request.take()?;
        Some(self.begin_run(input, options))
    }

    fn begin_run(&mut self, input: TaskInput, options: TaskOptions) -> RunRequest {
        let cancel = CancellationToken::new();
        self.current_cancel = Some(cancel.clone());
        self.overlay_view = Some(streaming_view(&input));
        self.active_request = Some((input.clone(), options.clone()));
        RunRequest {
            generation: self.generation,
            input,
            options,
            cancel,
        }
    }

    pub fn hide_overlay(&mut self) {
        if let Some(cancel) = self.current_cancel.take() {
            cancel.cancel();
        }
        self.active_request = None;
        self.probe = None;
        self.attached_image = None;
        self.overlay_view = None;
    }

    pub fn fail_transport(&mut self, generation: u64) {
        if generation != self.generation || self.state() != AppState::Translating {
            return;
        }
        self.current_cancel = None;
        self.active_request = None;
        self.overlay_view = Some(OverlayView::Failed {
            cause: FailureCause::TransportChannel,
            action: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gloss_core::config::Language;
    use gloss_core::guard::{FrontApp, SceneFacts, SensitiveKind, TriggerBlock};
    use gloss_core::model::{GlossError, Lang, ScreenPoint};
    use gloss_core::task::OutcomeStructured;

    use super::*;

    fn selection_gesture() -> PlatformEvent {
        PlatformEvent::SelectionGesture {
            pos: ScreenPoint::new(0, 0),
        }
    }

    fn plain_outcome(note: &str) -> TaskOutcome {
        TaskOutcome {
            kind: TaskKind::TranslateWord,
            note: note.into(),
            structured: OutcomeStructured::Plain {
                examples: Vec::new(),
            },
            degraded: false,
        }
    }

    fn degraded_outcome() -> TaskOutcome {
        TaskOutcome {
            kind: TaskKind::ExplainCode,
            note: String::new(),
            structured: OutcomeStructured::Code {
                examples: Vec::new(),
                code_language: None,
            },
            degraded: true,
        }
    }

    fn text_input(text: &str) -> TaskInput {
        TaskInput::Text { text: text.into() }
    }

    fn suspicious_text() -> String {
        format!("key sk-{}", "aB3".repeat(8))
    }

    fn probe(machine: &mut TaskStateMachine, config: &Config) -> u64 {
        probe_using(machine, config, Locale::Zh)
    }

    fn probe_using(machine: &mut TaskStateMachine, config: &Config, system_locale: Locale) -> u64 {
        let command = machine
            .begin_selection_probe(
                &selection_gesture(),
                config,
                system_locale,
                &SceneFacts::default(),
            )
            .expect("selection gesture must probe");
        let AcquireCommand::AcquireText { generation } = command else {
            panic!("acquire text expected");
        };
        generation
    }

    fn pasteboard_probe(machine: &mut TaskStateMachine, config: &Config) -> u64 {
        let command = machine
            .begin_pasteboard_probe(
                &PlatformEvent::PasteboardImageObserved,
                config,
                Locale::Zh,
                &SceneFacts::default(),
            )
            .expect("pasteboard observation must probe");
        let AcquireCommand::AcquireClipboardImage { generation } = command else {
            panic!("acquire clipboard image expected");
        };
        generation
    }

    fn image_input(png: &[u8]) -> TaskInput {
        TaskInput::Image {
            png: Arc::from(png),
        }
    }

    fn dispatched(outcome: InputOutcome) -> RunRequest {
        match outcome {
            InputOutcome::Dispatch(request) => request,
            other => panic!("expected a dispatch, got {other:?}"),
        }
    }

    fn blocked_by_app() -> SceneFacts {
        SceneFacts {
            secure_input: false,
            front_app: Some(FrontApp {
                bundle_id: Some("com.1password.1password".into()),
                name: None,
                is_self: false,
            }),
        }
    }

    fn frontmost_is_self() -> SceneFacts {
        SceneFacts {
            secure_input: false,
            front_app: Some(FrontApp {
                bundle_id: Some("com.example.gloss".into()),
                name: None,
                is_self: true,
            }),
        }
    }

    #[test]
    fn probe_mapping_covers_wired_events_only() {
        let mut machine = TaskStateMachine::new();
        let probe_id = probe(&mut machine, &Config::default());
        assert_eq!(
            probe_id, 1,
            "the selection probe takes the first id from the counter"
        );
        assert_eq!(
            machine.generation(),
            0,
            "a probe takes a probe id, not a generation: the visible session is untouched"
        );

        for event in [
            PlatformEvent::PasteboardImageObserved,
            PlatformEvent::OpenSettingsRequested,
        ] {
            assert!(
                machine
                    .begin_selection_probe(
                        &event,
                        &Config::default(),
                        Locale::Zh,
                        &SceneFacts::default()
                    )
                    .is_none(),
                "unwired events must not acquire"
            );
        }
        assert_eq!(
            machine.generation(),
            0,
            "unwired events must not consume a generation"
        );
        assert_eq!(
            machine.probe_id(),
            Some(1),
            "unwired events must not replace the outstanding probe"
        );
    }

    #[test]
    fn trigger_decision_separates_blocked_and_unwired_events() {
        let open = SceneFacts::default();
        assert_eq!(
            trigger_decision(&selection_gesture(), &open),
            TriggerDecision::Acquire,
            "a selection carries no explicit intent: the kind is the LLM layer's call"
        );
        assert_eq!(
            trigger_decision(&PlatformEvent::PasteboardImageObserved, &open),
            TriggerDecision::Acquire,
            "a pasteboard image is an explicit image-explain intent"
        );
        assert_eq!(
            trigger_decision(&PlatformEvent::PasteboardImageObserved, &blocked_by_app()),
            TriggerDecision::Blocked(TriggerBlock::BlockedApp("com.1password.1password")),
            "a pasteboard observation in a sensitive app is blocked by the scene gate alone"
        );
        assert_eq!(
            trigger_decision(
                &PlatformEvent::PasteboardImageObserved,
                &frontmost_is_self()
            ),
            TriggerDecision::Acquire,
            "the clipboard watch has no self-frontmost mistouch surface"
        );
        assert_eq!(
            trigger_decision(&PlatformEvent::OpenSettingsRequested, &blocked_by_app()),
            TriggerDecision::Unwired,
            "settings is an entry point, never a gated trigger"
        );
        assert_eq!(
            trigger_decision(&PlatformEvent::AccessibilityGranted, &open),
            TriggerDecision::Unwired,
            "the grant event is handled in the drain, never a gated trigger"
        );
        assert_eq!(
            trigger_decision(&selection_gesture(), &open),
            TriggerDecision::Acquire
        );
        assert_eq!(
            trigger_decision(&selection_gesture(), &blocked_by_app()),
            TriggerDecision::Blocked(TriggerBlock::BlockedApp("com.1password.1password")),
            "a selection in a sensitive app is blocked"
        );
    }

    #[test]
    fn scene_gate_stops_the_probe_before_acquisition() {
        let mut machine = TaskStateMachine::new();
        let secure = SceneFacts {
            secure_input: true,
            front_app: Some(FrontApp {
                bundle_id: Some("com.example.editor".into()),
                name: None,
                is_self: false,
            }),
        };
        assert!(
            begin_probe_on(&mut machine, &secure),
            "a focused password field stops the probe"
        );
        assert!(
            begin_probe_on(&mut machine, &blocked_by_app()),
            "a listed frontmost app stops the probe"
        );
        assert_eq!(
            machine.probe_id(),
            None,
            "a suppressed probe takes no probe id"
        );
        assert_eq!(
            machine.state(),
            AppState::Idle,
            "no overlay, no failure card"
        );

        assert_eq!(
            probe(&mut machine, &Config::default()),
            1,
            "the same gesture probes once the scene clears"
        );
        assert_eq!(
            machine.generation(),
            0,
            "the probe itself still leaves the generation alone"
        );
    }

    #[test]
    fn probe_in_the_self_frontmost_scene_is_suppressed_without_an_id() {
        assert_eq!(
            trigger_decision(&selection_gesture(), &frontmost_is_self()),
            TriggerDecision::SelfSuppressed,
            "a drag over gloss's own window is anti-mistouch territory, not a trigger"
        );

        let mut machine = TaskStateMachine::new();
        assert!(
            machine
                .begin_selection_probe(
                    &selection_gesture(),
                    &Config::default(),
                    Locale::Zh,
                    &frontmost_is_self()
                )
                .is_none(),
            "the suppressed gesture acquires nothing"
        );
        assert_eq!(machine.probe_id(), None);
        assert_eq!(machine.state(), AppState::Idle);
        assert!(machine.overlay_view().is_none());
    }

    #[test]
    fn selection_options_pair_with_one_snapshot_including_the_model() {
        let mut machine = TaskStateMachine::new();
        let config = Config {
            target_lang: Lang::Ja,
            model: "frozen-model".into(),
            ..Default::default()
        };

        let probe_id = probe(&mut machine, &config);
        assert_eq!(
            probe_id, 1,
            "the gesture dispatches no kind; the LLM layer classifies"
        );

        let request = dispatched(machine.commit_probe(1, text_input("fn main() {}")));
        assert_eq!(
            request.options.model.as_ref(),
            "frozen-model",
            "the model freezes from the probe-time snapshot"
        );
        assert_eq!(
            request.options.target_lang,
            Some(Lang::Ja),
            "target language still freezes from the probe-time snapshot"
        );
    }

    #[test]
    fn prompt_locale_follows_config_language_and_the_system() {
        let mut explicit = TaskStateMachine::new();
        let chosen = Config {
            language: Language::En,
            ..Default::default()
        };
        assert_eq!(probe(&mut explicit, &chosen), 1);
        let request = dispatched(explicit.commit_probe(1, text_input("hello")));
        assert_eq!(
            request.options.prompt_locale,
            Some(Locale::En),
            "an explicit choice ignores the injected system language"
        );

        let mut following = TaskStateMachine::new();
        let factory = Config::default();
        assert_eq!(factory.language, Language::System);
        assert_eq!(probe_using(&mut following, &factory, Locale::En), 1);
        let request = dispatched(following.commit_probe(1, text_input("hello")));
        assert_eq!(
            request.options.prompt_locale,
            Some(Locale::En),
            "follow-the-system takes the injected system language"
        );
    }

    #[test]
    fn options_freeze_at_probe_time() {
        let mut machine = TaskStateMachine::new();
        let before = Config {
            target_lang: Lang::Ja,
            model: "before-model".into(),
            language: Language::En,
            ..Default::default()
        };
        let after = Config {
            target_lang: Lang::Ko,
            model: "after-model".into(),
            language: Language::Zh,
            ..Default::default()
        };

        assert_eq!(probe(&mut machine, &before), 1);
        let request = dispatched(machine.commit_probe(1, text_input("hello")));
        assert_eq!(
            request.options.target_lang,
            Some(Lang::Ja),
            "in-flight task must keep the snapshot taken at probe time"
        );
        assert_eq!(
            request.options.model.as_ref(),
            "before-model",
            "the model is frozen with the rest of the options"
        );
        assert_eq!(
            request.options.prompt_locale,
            Some(Locale::En),
            "the prompt locale is frozen with the rest of the options"
        );
        assert_eq!(probe(&mut machine, &after), 2);
        let request = dispatched(machine.commit_probe(2, text_input("world")));
        assert_eq!(request.options.target_lang, Some(Lang::Ko));
        assert_eq!(request.options.model.as_ref(), "after-model");
        assert_eq!(request.options.prompt_locale, Some(Locale::Zh));
    }

    #[test]
    fn commit_probe_yields_run_request_and_supersedes_the_previous_session() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);

        let request = dispatched(machine.commit_probe(1, text_input("hello")));
        assert_eq!(request.generation, 1);
        assert_eq!(machine.generation(), 1, "the probe id is promoted");
        assert_eq!(machine.state(), AppState::Translating);
        assert!(machine.current_cancel().is_some());
        assert!(machine.probe_id().is_none(), "the probe is consumed");

        let old_cancel = request.cancel;
        assert_eq!(probe(&mut machine, &Config::default()), 2);
        assert_eq!(machine.state(), AppState::Translating, "探测不动当前会话");
        dispatched(machine.commit_probe(2, text_input("world")));
        assert_eq!(machine.generation(), 2);
        assert!(
            old_cancel.is_cancelled(),
            "commit must cancel the superseded in-flight task"
        );
        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Streaming { source, .. }) if source == "world"
        ));

        assert!(
            matches!(
                machine.commit_probe(2, text_input("again")),
                InputOutcome::Ignored
            ),
            "a consumed probe accepts no duplicate commit"
        );
    }

    #[test]
    fn a_probe_leaves_a_visible_session_completely_untouched() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        let request = dispatched(machine.commit_probe(1, text_input("hello")));
        machine.accept_chunk(1, "已到达的正文".into());
        let view_before = machine.overlay_view().cloned();
        let state_before = machine.state();
        let cancel_before = request.cancel.clone();

        assert_eq!(probe(&mut machine, &Config::default()), 2);
        assert_eq!(machine.state(), state_before);
        assert_eq!(machine.overlay_view(), view_before.as_ref());
        assert!(machine.current_cancel().is_some());
        assert!(
            !cancel_before.is_cancelled(),
            "probing must not cancel the visible session's task"
        );

        let outcome = machine.commit_probe_failed(2, &GlossError::SelectionEmpty);
        assert_eq!(outcome, FailureOutcome::SilentlyDropped);
        assert_eq!(machine.state(), state_before, "误滑连状态都不碰");
        assert_eq!(machine.overlay_view(), view_before.as_ref());
        assert!(!cancel_before.is_cancelled());
    }

    #[test]
    fn image_input_for_text_kind_is_rejected() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        assert!(matches!(
            machine.commit_probe(
                1,
                TaskInput::Image {
                    png: Arc::from(&b"png"[..])
                }
            ),
            InputOutcome::Ignored
        ));
    }

    #[test]
    fn classified_kind_updates_the_streaming_chip_only_once_current() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        dispatched(machine.commit_probe(1, text_input("hello")));

        assert!(
            !machine.accept_classified(0, TaskKind::TranslateWord),
            "a stale classification must be dropped"
        );
        assert!(machine.accept_classified(1, TaskKind::TranslateWord));
        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Streaming {
                classified: Some(TaskKind::TranslateWord),
                ..
            })
        ));

        machine.accept_done(1, plain_outcome("产物"));
        assert!(
            !machine.accept_classified(1, TaskKind::ExplainCode),
            "a late classification must not touch a settled outcome card"
        );
    }

    #[test]
    fn the_streaming_view_waits_unclassified_until_the_llm_layer_reports() {
        let mut machine = TaskStateMachine::new();
        let probe_id = probe(&mut machine, &Config::default());
        dispatched(machine.commit_probe(
            probe_id,
            TaskInput::Text {
                text: "hello".into(),
            },
        ));
        assert!(
            matches!(
                machine.overlay_view(),
                Some(OverlayView::Streaming {
                    classified: None,
                    ..
                })
            ),
            "the streaming view stays unclassified until the LLM layer reports"
        );
    }

    #[test]
    fn the_repair_marker_clears_the_stream_and_refuses_stale_rounds() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        dispatched(machine.commit_probe(1, text_input("hello")));
        machine.accept_chunk(1, "上一轮的违约正文".into());

        assert!(
            !machine.accept_repairing(0),
            "a stale repair marker must be dropped"
        );
        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Streaming { raw, repairing: false, .. }) if raw == "上一轮的违约正文"
        ));

        assert!(machine.accept_repairing(1));
        assert!(
            matches!(
                machine.overlay_view(),
                Some(OverlayView::Streaming { raw, repairing: true, .. }) if raw.is_empty()
            ),
            "the violated accumulation is cleared when the repair round starts"
        );

        machine.accept_chunk(1, "新一轮的正文".into());
        assert!(
            matches!(
                machine.overlay_view(),
                Some(OverlayView::Streaming {
                    repairing: true,
                    ..
                })
            ),
            "chunks of the repair round keep the repairing footer on"
        );

        machine.accept_done(1, plain_outcome("产物"));
        assert!(
            !machine.accept_repairing(1),
            "a settled outcome card must not be cleared by a late repair marker"
        );

        assert_eq!(probe(&mut machine, &Config::default()), 2);
        dispatched(machine.commit_probe(2, text_input("新任务")));
        assert!(
            machine.accept_repairing(2),
            "a fresh session accepts its own repair markers"
        );
    }

    #[test]
    fn a_degraded_done_keeps_the_request_and_retry_redispatches_it() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        let original = dispatched(machine.commit_probe(1, text_input("hello")));
        machine.accept_chunk(1, "{\"note\":".into());
        assert!(machine.accept_done(1, degraded_outcome()));
        assert_eq!(machine.state(), AppState::Show);
        assert!(
            machine.current_cancel().is_none(),
            "a settled task drops its token: Some must imply Translating"
        );
        assert!(
            matches!(
                machine.overlay_view(),
                Some(OverlayView::Outcome { outcome, .. }) if outcome.degraded
            ),
            "the degraded outcome settles on the outcome card"
        );

        let retried = machine
            .retry()
            .expect("a degraded card keeps the request copy for the retry");
        assert_eq!(retried.generation, original.generation, "same generation");
        assert_eq!(retried.input, original.input, "same input is re-dispatched");
        assert_eq!(
            retried.options, original.options,
            "same options are re-dispatched (no config re-read)"
        );
        assert_eq!(machine.state(), AppState::Translating);
        assert!(
            machine.current_cancel().is_some(),
            "the retry re-enters Translating with a fresh token"
        );
        assert!(
            matches!(
                machine.overlay_view(),
                Some(OverlayView::Streaming { source, .. }) if source == "hello"
            ),
            "the retry returns to the streaming view"
        );

        assert_eq!(probe(&mut machine, &Config::default()), 2);
        dispatched(machine.commit_probe(2, text_input("world")));
        assert!(machine.accept_done(2, plain_outcome("正常产物")));
        assert_eq!(machine.state(), AppState::Show);
        assert!(
            machine.retry().is_none(),
            "a normal outcome has no retry exit"
        );
    }

    #[test]
    fn hide_abandons_inflight_and_drops_late_events() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        let request = dispatched(machine.commit_probe(1, text_input("hello")));
        let token = request.cancel;

        machine.hide_overlay();
        assert_eq!(machine.state(), AppState::Idle);
        assert!(machine.overlay_view().is_none());
        assert!(token.is_cancelled(), "hide must cancel the in-flight task");
        assert!(machine.current_cancel().is_none());

        assert!(
            !machine.accept_done(1, plain_outcome("迟到产物")),
            "hidden task's late outcome must be dropped"
        );
        assert_eq!(
            machine.accept_failed(1, &GlossError::EngineNetwork),
            FailureOutcome::Ignored,
            "hidden task's late failure must be dropped"
        );
        assert_eq!(machine.state(), AppState::Idle);
    }

    #[test]
    fn hide_overlay_drops_the_outstanding_probe() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        machine.hide_overlay();
        assert!(
            matches!(
                machine.commit_probe(1, text_input("迟到的产物")),
                InputOutcome::Ignored
            ),
            "a probe dropped by hide must not resurrect the overlay"
        );
        assert_eq!(
            machine.commit_probe_failed(1, &GlossError::SelectionEmpty),
            FailureOutcome::Ignored
        );

        assert_eq!(pasteboard_probe(&mut machine, &Config::default()), 2);
        machine.hide_overlay();
        assert!(
            matches!(
                machine.commit_probe(2, image_input(b"late product")),
                InputOutcome::Ignored
            ),
            "a pasteboard probe dropped by hide must not resurrect the overlay either"
        );
    }

    #[test]
    fn pasteboard_probe_takes_an_id_without_touching_the_session() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(pasteboard_probe(&mut machine, &Config::default()), 1);
        assert_eq!(
            machine.probe_id(),
            Some(1),
            "the observation parks an outstanding probe"
        );
        assert_eq!(
            machine.generation(),
            0,
            "a probe takes a probe id, not a generation"
        );
        assert_eq!(machine.state(), AppState::Idle);
        assert!(machine.overlay_view().is_none());
        assert!(machine.attached_image().is_none());
        machine.drop_probe();

        assert_eq!(probe(&mut machine, &Config::default()), 2);
        let request = dispatched(machine.commit_probe(2, text_input("hello")));
        machine.accept_chunk(2, "已到达的正文".into());
        assert_eq!(pasteboard_probe(&mut machine, &Config::default()), 3);
        assert_eq!(machine.state(), AppState::Translating);
        assert_eq!(
            machine.generation(),
            2,
            "the probe leaves the visible session alone"
        );
        assert!(
            !request.cancel.is_cancelled(),
            "probing must not cancel the visible session's task"
        );
    }

    #[test]
    fn the_scene_gate_stops_the_pasteboard_probe_too() {
        let mut machine = TaskStateMachine::new();
        assert!(
            machine
                .begin_pasteboard_probe(
                    &PlatformEvent::PasteboardImageObserved,
                    &Config::default(),
                    Locale::Zh,
                    &blocked_by_app()
                )
                .is_none(),
            "a listed frontmost app stops the observation"
        );
        let secure = SceneFacts {
            secure_input: true,
            front_app: None,
        };
        assert!(
            machine
                .begin_pasteboard_probe(
                    &PlatformEvent::PasteboardImageObserved,
                    &Config::default(),
                    Locale::Zh,
                    &secure
                )
                .is_none(),
            "a focused password field stops the observation"
        );
        assert_eq!(
            machine.probe_id(),
            None,
            "a suppressed observation takes no probe id"
        );
        assert_eq!(machine.state(), AppState::Idle);

        assert_eq!(
            pasteboard_probe(&mut machine, &Config::default()),
            1,
            "the observation probes once the scene clears"
        );
        assert_eq!(
            machine.generation(),
            0,
            "the probe itself still leaves the generation alone"
        );
    }

    #[test]
    fn pasteboard_commit_retains_the_image_and_dispatches() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(pasteboard_probe(&mut machine, &Config::default()), 1);
        let png: Arc<[u8]> = Arc::from(&b"png-bytes"[..]);
        let request = dispatched(machine.commit_probe(
            1,
            TaskInput::Image {
                png: Arc::clone(&png),
            },
        ));
        assert_eq!(request.generation, 1);
        assert_eq!(
            request.input,
            TaskInput::Image {
                png: Arc::clone(&png)
            },
            "the dispatch carries the image input"
        );
        assert_eq!(machine.generation(), 1, "the probe id is promoted");
        assert_eq!(machine.state(), AppState::Translating);
        assert!(machine.probe_id().is_none(), "the probe is consumed");
        assert!(
            machine
                .attached_image()
                .is_some_and(|attached| Arc::ptr_eq(attached, &png)),
            "the image is retained by Arc without copying bytes"
        );
        assert!(
            matches!(
                machine.overlay_view(),
                Some(OverlayView::Streaming { source, .. }) if source.is_empty()
            ),
            "the streaming card carries no source text: the image itself is the 经"
        );
    }

    #[test]
    fn probes_are_mutually_exclusive_across_triggers() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        assert_eq!(
            pasteboard_probe(&mut machine, &Config::default()),
            2,
            "a pasteboard probe replaces the outstanding selection probe"
        );
        assert!(matches!(
            machine.commit_probe(1, text_input("旧探测的产物")),
            InputOutcome::Ignored
        ));
        assert_eq!(
            machine.commit_probe_failed(1, &GlossError::ImageTooLarge),
            FailureOutcome::Ignored,
            "the superseded probe's late failure is stale too"
        );
        assert_eq!(
            dispatched(machine.commit_probe(2, image_input(b"png"))).generation,
            2
        );

        assert_eq!(
            probe(&mut machine, &Config::default()),
            3,
            "a selection probe replaces the outstanding pasteboard probe"
        );
        assert!(matches!(
            machine.commit_probe(2, image_input(b"png")),
            InputOutcome::Ignored
        ));
        assert_eq!(
            dispatched(machine.commit_probe(3, text_input("新选区"))).generation,
            3
        );
    }

    #[test]
    fn text_on_a_pasteboard_probe_is_ignored_without_consuming() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(pasteboard_probe(&mut machine, &Config::default()), 1);
        assert!(
            matches!(
                machine.commit_probe(1, text_input("剪贴板探测不收文本")),
                InputOutcome::Ignored
            ),
            "a pasteboard probe only accepts image products"
        );
        assert_eq!(
            dispatched(machine.commit_probe(1, image_input(b"png"))).generation,
            1,
            "the probe survives the mismatch and still commits"
        );
    }

    #[test]
    fn pasteboard_commit_supersedes_and_a_new_task_clears_the_image() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        let old = dispatched(machine.commit_probe(1, text_input("旧任务")));
        assert_eq!(pasteboard_probe(&mut machine, &Config::default()), 2);
        let png: Arc<[u8]> = Arc::from(&b"png-bytes"[..]);
        dispatched(machine.commit_probe(
            2,
            TaskInput::Image {
                png: Arc::clone(&png),
            },
        ));
        assert!(
            old.cancel.is_cancelled(),
            "the pasteboard commit supersedes the in-flight task"
        );
        assert!(machine.attached_image().is_some());

        assert_eq!(probe(&mut machine, &Config::default()), 3);
        dispatched(machine.commit_probe(3, text_input("回到文本")));
        assert!(
            machine.attached_image().is_none(),
            "a text task clears the attached image"
        );
    }

    #[test]
    fn hide_overlay_clears_the_attached_image() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(pasteboard_probe(&mut machine, &Config::default()), 1);
        dispatched(machine.commit_probe(1, image_input(b"png")));
        assert!(machine.attached_image().is_some());
        machine.hide_overlay();
        assert!(
            machine.attached_image().is_none(),
            "hide clears the attached image with the rest of the session"
        );
    }

    #[test]
    fn image_too_large_failure_lands_in_the_failure_card() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(pasteboard_probe(&mut machine, &Config::default()), 1);
        assert_eq!(
            machine.commit_probe_failed(1, &GlossError::ImageTooLarge),
            FailureOutcome::Shown,
            "an oversized image is a real failure, not a mis-slide"
        );
        assert_eq!(machine.state(), AppState::Error);
        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Failed { action: None, .. })
        ));
        assert!(
            machine.retry().is_none(),
            "an oversized image has no button-shaped exit"
        );
        assert!(machine.probe_id().is_none(), "the probe is consumed");
        assert!(
            machine.attached_image().is_none(),
            "nothing was ever committed"
        );
    }

    #[test]
    fn retrying_a_failed_image_task_keeps_the_attached_image() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(pasteboard_probe(&mut machine, &Config::default()), 1);
        let png: Arc<[u8]> = Arc::from(&b"png-bytes"[..]);
        dispatched(machine.commit_probe(
            1,
            TaskInput::Image {
                png: Arc::clone(&png),
            },
        ));
        machine.accept_chunk(1, "{\"note\":\"描述".into());
        assert_eq!(
            machine.accept_failed(1, &GlossError::EngineNetwork),
            FailureOutcome::Shown
        );

        let retried = machine.retry().expect("retry must be available");
        assert_eq!(
            retried.input,
            TaskInput::Image {
                png: Arc::clone(&png)
            },
            "the retry resends the same image input"
        );
        assert!(
            machine
                .attached_image()
                .is_some_and(|attached| Arc::ptr_eq(attached, &png)),
            "the image survives the retry for the streaming card"
        );
        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Streaming { source, .. }) if source.is_empty()
        ));
    }

    #[test]
    fn failed_guard_matches_translating_only() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        dispatched(machine.commit_probe(1, text_input("hello")));
        assert_eq!(
            machine.accept_failed(1, &GlossError::EngineNetwork),
            FailureOutcome::Shown,
            "an engine failure is not the no-selection family, the card still shows"
        );
        assert_eq!(machine.state(), AppState::Error);

        machine.hide_overlay();
        assert_eq!(
            machine.accept_failed(1, &GlossError::EngineNetwork),
            FailureOutcome::Ignored,
            "hidden error card must not be resurrected"
        );
    }

    #[test]
    fn probe_no_selection_failures_are_silently_dropped() {
        for error in [GlossError::SelectionUnavailable, GlossError::SelectionEmpty] {
            let mut machine = TaskStateMachine::new();
            assert_eq!(probe(&mut machine, &Config::default()), 1);
            assert_eq!(
                machine.commit_probe_failed(1, &error),
                FailureOutcome::SilentlyDropped,
                "{error:?} on a probe is a pure mis-drag: no card, no state change"
            );
            assert_eq!(machine.state(), AppState::Idle);
            assert!(machine.overlay_view().is_none());
            assert!(machine.probe_id().is_none(), "the probe is consumed");
        }
    }

    #[test]
    fn probe_permission_failures_still_raise_the_card() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        assert_eq!(
            machine.commit_probe_failed(1, &GlossError::AccessibilityDenied),
            FailureOutcome::Shown,
            "a permission failure is actionable feedback, not a mis-drag"
        );
        assert_eq!(machine.state(), AppState::Error);
        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Failed { .. })
        ));
    }

    #[test]
    fn selection_failures_outside_the_probe_still_raise_the_card() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        dispatched(machine.commit_probe(1, text_input("hello")));
        assert_eq!(
            machine.accept_failed(1, &GlossError::SelectionUnavailable),
            FailureOutcome::Shown,
            "the silent drop covers the probe leg only; an inference failure is a card"
        );
        assert_eq!(machine.state(), AppState::Error);
    }

    #[test]
    fn stale_probe_results_are_dropped() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        assert_eq!(
            probe(&mut machine, &Config::default()),
            2,
            "a newer probe replaces the older one"
        );
        assert!(
            matches!(
                machine.commit_probe(1, text_input("旧探测的产物")),
                InputOutcome::Ignored
            ),
            "a superseded probe must not commit"
        );
        assert_eq!(
            machine.commit_probe_failed(1, &GlossError::SelectionUnavailable),
            FailureOutcome::Ignored
        );
    }

    #[test]
    fn modality_mismatch_preserves_pending_options() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        assert!(matches!(
            machine.commit_probe(
                1,
                TaskInput::Image {
                    png: Arc::from(&b"png"[..])
                }
            ),
            InputOutcome::Ignored
        ));
        assert!(
            matches!(
                machine.commit_probe(1, text_input("第二次")),
                InputOutcome::Dispatch(_)
            ),
            "pending options must survive a modality mismatch"
        );
    }

    #[test]
    fn transport_failure_lands_in_error() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        let request = dispatched(machine.commit_probe(1, text_input("x")));
        machine.fail_transport(request.generation);
        assert_eq!(machine.state(), AppState::Error);
        assert!(machine.current_cancel().is_none());
        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Failed { action: None, .. })
        ));
        assert!(machine.retry().is_none());
    }

    #[test]
    fn retryable_failure_keeps_request_and_retry_redispatches_it() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        let original = dispatched(machine.commit_probe(1, text_input("hello")));
        assert_eq!(
            machine.accept_failed(1, &GlossError::EngineNetwork),
            FailureOutcome::Shown
        );

        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Failed {
                cause: FailureCause::Task(GlossError::EngineNetwork),
                action: Some(ErrorAction::Retry),
            })
        ));

        let retried = machine.retry().expect("retry must be available");
        assert_eq!(retried.generation, original.generation, "same generation");
        assert_eq!(retried.input, original.input, "same input is re-dispatched");
        assert_eq!(
            retried.options, original.options,
            "same options are re-dispatched (no config re-read)"
        );
        assert!(retried.cancel != original.cancel, "fresh cancel token");
        assert_eq!(machine.state(), AppState::Translating);
        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Streaming { source, .. }) if source == "hello"
        ));
    }

    fn begin_probe_on(machine: &mut TaskStateMachine, scene: &SceneFacts) -> bool {
        machine
            .begin_selection_probe(&selection_gesture(), &Config::default(), Locale::Zh, scene)
            .is_none()
    }

    fn fail_with(machine: &mut TaskStateMachine, generation: u64, error: &GlossError) {
        assert_eq!(probe(machine, &Config::default()), generation);
        dispatched(machine.commit_probe(generation, text_input("x")));
        assert_eq!(
            machine.accept_failed(generation, error),
            FailureOutcome::Shown
        );
    }

    fn assert_settings_card(machine: &TaskStateMachine) {
        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Failed {
                action: Some(ErrorAction::OpenSettings),
                ..
            })
        ));
    }

    #[test]
    fn error_actions_follow_the_mapping_table() {
        let mut machine = TaskStateMachine::new();
        fail_with(&mut machine, 1, &GlossError::EngineRateLimited);
        assert!(machine.retry().is_some());

        fail_with(&mut machine, 2, &GlossError::EngineAuth);
        assert_settings_card(&machine);
        assert!(
            machine.retry().is_none(),
            "auth failure must not offer a retry button"
        );

        fail_with(&mut machine, 3, &GlossError::UnsupportedModality);
        assert_settings_card(&machine);

        fail_with(
            &mut machine,
            4,
            &GlossError::Config("empty model id".into()),
        );
        assert_settings_card(&machine);

        fail_with(&mut machine, 5, &GlossError::JsonModeUnsupported);
        assert!(matches!(
            machine.overlay_view(),
            Some(OverlayView::Failed {
                cause: FailureCause::Task(GlossError::JsonModeUnsupported),
                action: Some(ErrorAction::OpenSettings),
            })
        ));
        assert!(
            machine.retry().is_none(),
            "an unsupported endpoint cannot be fixed by resending"
        );
    }

    #[test]
    fn new_commit_and_hide_supersede_the_retry_request() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        dispatched(machine.commit_probe(1, text_input("x")));
        assert_eq!(
            machine.accept_failed(1, &GlossError::EngineNetwork),
            FailureOutcome::Shown
        );

        assert_eq!(
            probe(&mut machine, &Config::default()),
            2,
            "probing alone leaves the retry offer alone"
        );
        assert!(
            machine.retry().is_some(),
            "a mis-slide must not kill the retry"
        );
        dispatched(machine.commit_probe(2, text_input("y")));
        assert!(
            machine.retry().is_none(),
            "the committed selection supersedes the retry offer"
        );

        assert_eq!(
            machine.accept_failed(2, &GlossError::EngineNetwork),
            FailureOutcome::Shown
        );
        machine.hide_overlay();
        assert_eq!(machine.state(), AppState::Idle);
        assert!(machine.retry().is_none(), "hide drops the retry request");
    }

    #[test]
    fn suspicious_input_is_dropped_while_the_visible_session_survives() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        let settled = dispatched(machine.commit_probe(1, text_input("上一次的普通文本")));
        assert!(machine.accept_done(1, plain_outcome("上一次的产物")));
        let view_before = machine.overlay_view().cloned();

        assert_eq!(probe(&mut machine, &Config::default()), 2);
        let outcome = machine.commit_probe(2, text_input(&suspicious_text()));
        assert!(
            matches!(outcome, InputOutcome::Blocked(SensitiveKind::Token)),
            "a selection that looks like a token must be refused, got {outcome:?}"
        );
        assert_eq!(
            machine.state(),
            AppState::Show,
            "the refused probe leaves the visible session alone"
        );
        assert_eq!(
            machine.overlay_view(),
            view_before.as_ref(),
            "the previous card is preserved — a refusal is not a dismissal"
        );
        assert!(machine.probe_id().is_none(), "the probe is consumed");
        assert!(
            machine.active_request.is_none(),
            "no request copy is left behind for a later retry to pick up"
        );
        assert!(
            !settled.cancel.is_cancelled(),
            "nothing new was dispatched, so the finished session's token is untouched"
        );
    }

    #[test]
    fn blocked_input_is_not_redispatched_by_any_later_path() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);
        machine.commit_probe(1, text_input("card 4111 1111 1111 1111"));

        assert!(
            machine.retry().is_none(),
            "a refused probe is not a retryable failure"
        );
        assert!(matches!(
            machine.commit_probe(1, text_input("second arrival")),
            InputOutcome::Ignored
        ));
        assert!(
            !machine.accept_chunk(1, "迟到的正文".into())
                && !machine.accept_done(1, plain_outcome("迟到的产物"))
                && machine.accept_failed(1, &GlossError::EngineNetwork) == FailureOutcome::Ignored,
            "no product of a refused probe may be adopted either"
        );
        machine.fail_transport(1);
        assert_eq!(
            machine.generation(),
            0,
            "the refusal never touches the machine: no session to disturb"
        );

        assert_eq!(
            probe(&mut machine, &Config::default()),
            2,
            "the next probe starts over"
        );
        assert!(matches!(
            machine.commit_probe(2, text_input(&suspicious_text())),
            InputOutcome::Blocked(_)
        ));
        assert!(machine.probe_id().is_none());
    }

    #[test]
    fn ordinary_input_still_passes_the_content_gate() {
        let mut machine = TaskStateMachine::new();
        assert_eq!(probe(&mut machine, &Config::default()), 1);

        assert!(
            matches!(
                machine.commit_probe(1, text_input("今天下午三点开会")),
                InputOutcome::Dispatch(_)
            ),
            "the gate only fires on the high-confidence patterns"
        );
        assert_eq!(machine.state(), AppState::Translating);
    }
}

#[cfg(test)]
mod reveal_policy_tests {
    use super::policy::{EventKind, auto_show_after, auto_show_for};
    use super::should_reveal;

    #[test]
    fn auto_show_policy_decides_when_the_overlay_pops() {
        use EventKind::{InputReady, TaskChunk, TaskDone, TaskFailed};

        assert!(
            !auto_show_for(InputReady, true),
            "显形走挂起请求，取材成功不负责露面"
        );
        assert!(!auto_show_for(TaskDone, true), "完成时浮层早已可见");
        assert!(!auto_show_for(TaskChunk, true), "chunk 只追加不露面");
        assert!(auto_show_for(TaskFailed, true), "错误不该被吞掉");

        for kind in [InputReady, TaskChunk, TaskDone, TaskFailed] {
            assert!(
                !auto_show_for(kind, false),
                "未被采纳的 {kind:?} 不得触发显示"
            );
        }
    }

    #[test]
    fn auto_show_survives_a_mixed_batch() {
        use EventKind::{InputReady, TaskChunk, TaskDone, TaskFailed};

        assert!(
            auto_show_after([(TaskChunk, true), (TaskFailed, true)]),
            "任一事件要显示就显示"
        );
        assert!(
            auto_show_after([(TaskDone, false), (TaskFailed, true)]),
            "陈旧的成功不得抵消一条被采纳的失败"
        );
        assert!(
            !auto_show_after([(TaskDone, false), (InputReady, false)]),
            "整批都没被采纳（陈旧）→ 不显示：迟到的产物不得把浮层弹回来"
        );
        assert!(!auto_show_after([]), "空批不显示");
    }

    #[test]
    fn pending_reveal_shows_only_over_a_live_view() {
        use EventKind::TaskFailed;

        assert!(
            should_reveal(true, true, []),
            "划词提交：挂起请求只在活视图在场时显形"
        );
        assert!(
            !should_reveal(true, false, []),
            "视图为空时弹出的会是渲染自检卡，守卫必须拦下挂起显形"
        );
        assert!(
            !should_reveal(false, false, [(TaskFailed, false)]),
            "无挂起且批次里没有失败即弹时不显示"
        );
    }

    #[test]
    fn reveal_decision_combines_the_pending_request_with_the_batch() {
        use EventKind::{InputReady, TaskChunk, TaskDone, TaskFailed};

        assert!(
            should_reveal(true, true, [(TaskDone, false), (InputReady, false)]),
            "挂起显形不依赖回传批次：整批陈旧也拦不下它"
        );
        assert!(
            should_reveal(false, false, [(TaskFailed, true)]),
            "失败即弹独立成立：无挂起也照常显示"
        );
        assert!(
            !should_reveal(false, true, [(InputReady, true), (TaskChunk, true)]),
            "取材成功与流式增量不负责露面：批次再新鲜也不显形"
        );
    }
}
