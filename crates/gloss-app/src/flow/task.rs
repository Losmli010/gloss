//! 编排用例·任务段：消费通道4（取材产物与任务回传）→ 状态机转移 →
//! 通道3下发 → 批后露面决策与重绘。显形“要不要”的策略归状态机，
//! “怎么显示”在显隐段。

use gloss_core::log::{Span, debug, info, thread, warn};
use gloss_core::model::GlossError;
use gloss_core::task::TaskInput;
use winit::event_loop::ActiveEventLoop;

use crate::channel::{Command, Event, Traced};
use crate::machine::{FailureOutcome, InputOutcome, RunRequest, event_kind, should_reveal};

use super::reveal::{centered_position, show_position};
use crate::app::GlossApp;

impl GlossApp {
    pub(crate) fn drain_events(&mut self, event_loop: &ActiveEventLoop) {
        let events: Vec<Event> = self
            .endpoints
            .as_ref()
            .map_or(Vec::new(), |e| e.events.try_iter().collect());
        let mut batch = Vec::with_capacity(events.len());
        for event in events {
            let kind = event_kind(&event);
            let accepted = match event {
                Event::SecretPrewarmed { result } => {
                    self.on_secret_prewarmed(&result);
                    continue;
                }
                Event::InputReady { generation, input } => self.accept_input(generation, input),
                Event::TaskClassified { generation, kind } => {
                    self.accept_classified(generation, kind)
                }
                Event::TaskChunk { generation, delta } => self.accept_chunk(generation, delta),
                Event::TaskRepairing { generation } => self.accept_repairing(generation),
                Event::TaskDone {
                    generation,
                    outcome,
                } => self.accept_done(generation, outcome),
                Event::TaskFailed { generation, error } => self.accept_failed(generation, &error),
            };
            batch.push((kind, accepted));
        }
        let pending_reveal = std::mem::take(&mut self.session.pending_reveal);
        if should_reveal(pending_reveal, self.machine.overlay_view().is_some(), batch)
            && let Some(windows) = &mut self.workspace.windows
        {
            let centered = centered_position(event_loop, windows);
            let (position, placement) = show_position(
                self.session.selection_anchor,
                self.machine.generation(),
                centered,
            );
            windows.set_placement(placement);
            let position = windows.clamp_position(position);
            self.show_overlay(position);
        }
        self.request_redraw();
    }

    pub(crate) fn accept_input(&mut self, generation: u64, input: TaskInput) -> bool {
        if self.machine.probe_id() == Some(generation) {
            return self.commit_probe(generation, input);
        }
        debug!(
            thread = thread::UI,
            generation = generation,
            current = self.machine.generation(),
            "stale or unexpected input ready dropped"
        );
        false
    }

    fn commit_probe(&mut self, generation: u64, input: TaskInput) -> bool {
        let image_bytes = match &input {
            TaskInput::Image { png, .. } => Some(png.len()),
            _ => None,
        };
        match self.machine.commit_probe(generation, input) {
            InputOutcome::Pending => {
                match image_bytes {
                    Some(bytes) => info!(
                        thread = thread::UI,
                        generation, bytes, "pasteboard image committed, pending user confirmation"
                    ),
                    None => info!(
                        thread = thread::UI,
                        generation, "selection committed, pending user confirmation"
                    ),
                }
                self.session.probe_front_app = None;
                self.session.pending_reveal = true;
                true
            }
            InputOutcome::Blocked(reason) => {
                warn!(
                    thread = thread::UI,
                    generation = generation,
                    reason = ?reason,
                    "probed selection suppressed by the sensitive content guard, task not dispatched"
                );
                self.session.probe_front_app = None;
                false
            }
            InputOutcome::Ignored => {
                debug!(
                    thread = thread::UI,
                    generation = generation,
                    current = self.machine.generation(),
                    "stale probe result dropped"
                );
                false
            }
        }
    }

    pub(crate) fn send_run(&mut self, request: RunRequest) {
        let Some(endpoints) = &self.endpoints else {
            warn!(
                thread = thread::UI,
                generation = request.generation,
                "endpoints missing, task dropped"
            );
            self.machine.fail_transport(request.generation);
            return;
        };
        let traced = Traced {
            payload: Command::RunTask {
                generation: request.generation,
                input: request.input,
                options: request.options,
                cancel: request.cancel,
            },
            span: self.span_for(request.generation).unwrap_or_else(Span::none),
        };
        if endpoints.commands.send(traced).is_err() {
            warn!(
                thread = thread::UI,
                generation = request.generation,
                "command channel closed, task dropped"
            );
            self.machine.fail_transport(request.generation);
        }
    }

    fn accept_chunk(&mut self, generation: u64, delta: String) -> bool {
        let accepted = self.machine.accept_chunk(generation, delta);
        if !accepted {
            debug!(
                thread = thread::UI,
                generation = generation,
                current = self.machine.generation(),
                state = ?self.machine.state(),
                "superseded or hidden chunk dropped"
            );
        }
        accepted
    }

    fn accept_repairing(&mut self, generation: u64) -> bool {
        let accepted = self.machine.accept_repairing(generation);
        if !accepted {
            debug!(
                thread = thread::UI,
                generation = generation,
                current = self.machine.generation(),
                state = ?self.machine.state(),
                "superseded or hidden repair marker dropped"
            );
        }
        accepted
    }

    fn accept_classified(&mut self, generation: u64, kind: gloss_core::task::TaskKind) -> bool {
        let accepted = self.machine.accept_classified(generation, kind);
        if !accepted {
            debug!(
                thread = thread::UI,
                generation = generation,
                current = self.machine.generation(),
                state = ?self.machine.state(),
                "stale classification dropped"
            );
        }
        accepted
    }

    pub(crate) fn accept_done(
        &mut self,
        generation: u64,
        outcome: gloss_core::task::TaskOutcome,
    ) -> bool {
        let accepted = self.machine.accept_done(generation, outcome);
        if accepted {
            info!(
                thread = thread::UI,
                generation = generation,
                "task done, showing outcome"
            );
        } else {
            debug!(
                thread = thread::UI,
                generation = generation,
                current = self.machine.generation(),
                state = ?self.machine.state(),
                "superseded or hidden task done dropped"
            );
        }
        accepted
    }

    pub(crate) fn accept_failed(
        &mut self,
        generation: u64,
        error: &gloss_core::model::GlossError,
    ) -> bool {
        if self.machine.probe_id() == Some(generation) {
            return self.fail_probe(generation, error);
        }
        match self.machine.accept_failed(generation, error) {
            FailureOutcome::Shown => {
                warn!(
                    thread = thread::UI,
                    generation = generation,
                    error = %error,
                    "task failed"
                );
                true
            }
            FailureOutcome::SilentlyDropped => {
                debug!(
                    thread = thread::UI,
                    generation = generation,
                    error = %error,
                    "silent failure dropped outside the probe path"
                );
                false
            }
            FailureOutcome::Ignored => {
                info!(
                    thread = thread::UI,
                    generation = generation,
                    current = self.machine.generation(),
                    "stale task failed dropped, superseded by a newer gesture"
                );
                false
            }
        }
    }

    fn fail_probe(&mut self, generation: u64, error: &gloss_core::model::GlossError) -> bool {
        match self.machine.commit_probe_failed(generation, error) {
            FailureOutcome::SilentlyDropped => {
                let front_app = self
                    .session
                    .probe_front_app
                    .as_ref()
                    .map(|app| {
                        app.bundle_id
                            .as_deref()
                            .or(app.name.as_deref())
                            .unwrap_or("unknown")
                    })
                    .unwrap_or("unknown");
                info!(
                    thread = thread::UI,
                    generation = generation,
                    error = %error,
                    front_app,
                    "selection probe found nothing, treated as a mis-slide"
                );
                self.session.probe_front_app = None;
                false
            }
            FailureOutcome::Shown => {
                warn!(
                    thread = thread::UI,
                    generation = generation,
                    error = %error,
                    "probe failed"
                );
                self.session.probe_front_app = None;
                true
            }
            FailureOutcome::Ignored => {
                debug!(
                    thread = thread::UI,
                    generation = generation,
                    current = self.machine.generation(),
                    "stale probe failure dropped"
                );
                false
            }
        }
    }
}

impl GlossApp {
    pub(crate) fn on_accessibility_granted(&mut self) {
        if self.env.permissions_ready {
            debug!(
                thread = thread::UI,
                "accessibility grant event ignored, permissions already ready"
            );
            return;
        }
        self.env.permissions_ready = true;
        info!(
            thread = thread::UI,
            "accessibility granted mid-run, dispatching the deferred secret prewarm"
        );
        self.send_secret_prewarm();
    }

    pub(crate) fn send_secret_prewarm(&mut self) {
        if !self.env.permissions_ready {
            debug!(
                thread = thread::UI,
                "startup permissions not ready, secret prewarm deferred to the first task"
            );
            return;
        }
        let keychain_id = self
            .env
            .config
            .snapshot()
            .resolved_provider()
            .keychain_id
            .clone();
        let Some(endpoints) = &self.endpoints else {
            return;
        };
        let command = Traced::untraced(Command::PrewarmSecret { keychain_id });
        if let Err(err) = endpoints.commands.send(command) {
            debug!(
                thread = thread::UI,
                error = %err,
                "command channel closed, secret prewarm dropped"
            );
        }
    }

    pub(crate) fn on_secret_prewarmed(&mut self, result: &Result<(), GlossError>) {
        if let Err(err) = result {
            debug!(thread = thread::UI, error = %err, "secret prewarm reported failure");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gloss_core::config::{Config, DEFAULT_MODEL};
    use gloss_core::model::{GlossError, Lang};
    use gloss_core::task::TaskInput;

    use crate::app::test_support::{
        driven_app, driven_app_without_startup_permissions, outcome_note, plain_outcome,
        streaming_raw, text_input, trigger_selection,
    };
    use crate::channel::{AcquireCommand, Command, PlatformEvent, Traced};
    use crate::machine::{AppState, OverlayView};
    use crate::ui::popup::OverlayAction;

    use crate::app::GlossApp;

    fn confirm(app: &mut GlossApp) {
        app.handle_overlay_action(OverlayAction::Confirm);
    }

    fn pasteboard_probe(app: &mut GlossApp, pe_tx: &crossbeam_channel::Sender<PlatformEvent>) {
        pe_tx
            .send(PlatformEvent::PasteboardImageObserved)
            .expect("platform channel open");
        app.drain_platform_events();
    }

    fn image_input() -> TaskInput {
        TaskInput::Image {
            png: Arc::from(&b"png-bytes"[..]),
        }
    }

    fn expect_idle_probe(
        app: &mut GlossApp,
        pe_tx: &crossbeam_channel::Sender<PlatformEvent>,
        ac_rx: &crossbeam_channel::Receiver<Traced<AcquireCommand>>,
    ) {
        trigger_selection(app, pe_tx);
        assert_eq!(
            app.machine.state(),
            AppState::Idle,
            "the probe leaves the state machine alone until content arrives"
        );
        assert_eq!(app.machine.generation(), 0);
        assert!(matches!(
            ac_rx.try_recv().unwrap().payload,
            AcquireCommand::AcquireText { generation: 1, .. }
        ));
    }

    #[test]
    fn accessibility_grant_event_unblocks_and_dispatches_deferred_prewarm() {
        let (mut app, _config, _store, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) =
            driven_app_without_startup_permissions();

        pe_tx
            .send(PlatformEvent::AccessibilityGranted)
            .expect("platform channel open");
        app.drain_platform_events();

        let first = cmd_rx
            .try_recv()
            .expect("the grant must unblock the deferred prewarm");
        assert!(
            matches!(first.payload, Command::PrewarmSecret { .. }),
            "the dispatched command is the secret prewarm, got {:?}",
            first.payload
        );

        pe_tx
            .send(PlatformEvent::AccessibilityGranted)
            .expect("platform channel open");
        app.drain_platform_events();
        assert!(
            cmd_rx.try_recv().is_err(),
            "a repeat grant event must not re-dispatch: the gate stays unblocked"
        );
    }

    #[test]
    fn accessibility_grant_event_is_a_noop_when_already_ready() {
        let (mut app, _config, _store, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();

        pe_tx
            .send(PlatformEvent::AccessibilityGranted)
            .expect("platform channel open");
        app.drain_platform_events();
        assert!(
            cmd_rx.try_recv().is_err(),
            "a ready app gains nothing from the grant event"
        );
    }

    #[test]
    fn prewarm_dispatches_the_configured_keychain_entry_exactly_once() {
        let (mut app, _config, _store, _pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();

        app.send_secret_prewarm();
        app.send_secret_prewarm();

        let first = cmd_rx
            .try_recv()
            .expect("the first prewarm lands on channel 3");
        assert!(
            matches!(
                first.payload,
                Command::PrewarmSecret { ref keychain_id }
                    if *keychain_id == app.env.config.snapshot().resolved_provider().keychain_id
            ),
            "the entry id must come from the current snapshot, got {:?}",
            first.payload
        );
        let second = cmd_rx.try_recv().expect("each call sends its own command");
        assert!(matches!(second.payload, Command::PrewarmSecret { .. }));
        assert!(
            cmd_rx.try_recv().is_err(),
            "two calls send exactly two commands"
        );
    }

    #[test]
    fn prewarm_without_endpoints_is_a_silent_no_op() {
        let (mut app, _config, _store, _pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();
        app.endpoints = None;

        app.send_secret_prewarm();
        assert!(
            cmd_rx.try_recv().is_err(),
            "no endpoints means nothing is sent and nothing panics"
        );
    }

    #[test]
    fn prewarm_is_deferred_when_startup_permissions_are_not_ready() {
        let (mut app, _config, _store, _pe_tx, _ac_rx, mut cmd_rx, _ev_tx) =
            driven_app_without_startup_permissions();

        app.send_secret_prewarm();
        assert!(
            cmd_rx.try_recv().is_err(),
            "with permission guidance still in flight the keychain read must not be pulled \
             forward to startup"
        );
    }

    #[test]
    fn a_repair_marker_resets_the_stream_through_the_shell() {
        let (mut app, _config, _store, pe_tx, _ac_rx, _cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(1, text_input("hello")));
        confirm(&mut app);
        app.accept_chunk(1, "上一轮的违约正文".into());

        assert!(
            app.accept_repairing(1),
            "the shell adopts the current generation's repair marker"
        );
        assert!(
            matches!(
                app.machine.overlay_view(),
                Some(OverlayView::Streaming { raw, repairing: true, .. }) if raw.is_empty()
            ),
            "the shell clears the violated accumulation and flips the repairing footer"
        );

        assert!(
            !app.accept_repairing(9),
            "a stale repair marker is dropped without disturbing the live session"
        );
        assert!(matches!(
            app.machine.overlay_view(),
            Some(OverlayView::Streaming {
                repairing: true,
                ..
            })
        ));
    }

    #[test]
    fn late_events_of_superseded_trigger_do_not_bleed() {
        let (mut app, _config, _store, pe_tx, ac_rx, mut cmd_rx, _ev_tx) = driven_app();

        expect_idle_probe(&mut app, &pe_tx, &ac_rx);

        assert!(app.accept_input(1, text_input("A")));
        assert!(
            matches!(app.machine.state(), AppState::PendingConfirm),
            "the commit parks the pending card; channel 3 stays silent until the confirm"
        );
        assert!(cmd_rx.try_recv().is_err());
        confirm(&mut app);
        assert_eq!(app.machine.generation(), 1, "commit promotes the probe id");
        assert_eq!(app.machine.state(), AppState::Translating);
        let Command::RunTask {
            generation: 1,
            cancel: token_a,
            ..
        } = cmd_rx.try_recv().unwrap().payload
        else {
            panic!("expected a RunTask command, got another variant")
        };
        assert!(!token_a.is_cancelled());

        assert!(app.accept_chunk(1, "部分A".into()));
        assert!(streaming_raw(&app).contains("部分A"));

        trigger_selection(&mut app, &pe_tx);
        assert_eq!(
            app.machine.generation(),
            1,
            "the second gesture probes without superseding anything"
        );
        assert_eq!(
            app.machine.state(),
            AppState::Translating,
            "the visible session keeps running while the probe is out"
        );
        assert!(!token_a.is_cancelled(), "a probe must not cancel task A");
        assert!(
            app.accept_chunk(1, "续A".into()),
            "A's stream keeps flowing during the probe"
        );

        assert!(app.accept_input(2, text_input("B")));
        assert!(token_a.is_cancelled(), "the commit supersedes task A");
        confirm(&mut app);
        assert!(matches!(
            cmd_rx.try_recv().unwrap().payload,
            Command::RunTask { generation: 2, .. }
        ));
        assert!(!app.accept_chunk(1, "迟到A".into()));
        assert!(!app.accept_done(1, plain_outcome("迟到结果A")));
        assert!(app.accept_done(2, plain_outcome("结果B")));
        assert_eq!(app.machine.state(), AppState::Show);
        assert_eq!(outcome_note(&app), "结果B");
    }

    #[test]
    fn failed_task_lands_in_error_and_retry_works() {
        let (mut app, _config, _store, pe_tx, _ac_rx, _cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(1, text_input("A")));
        confirm(&mut app);

        assert!(!app.accept_failed(0, &GlossError::EngineNetwork));
        assert_eq!(app.machine.state(), AppState::Translating);

        assert!(app.accept_failed(1, &GlossError::EngineNetwork));
        assert_eq!(app.machine.state(), AppState::Error);
        assert!(app.machine.current_cancel().is_none());

        trigger_selection(&mut app, &pe_tx);
        assert_eq!(
            app.machine.state(),
            AppState::Error,
            "a probe never disturbs the visible failure card"
        );
        assert_eq!(app.machine.probe_id(), Some(2));
    }

    #[test]
    fn stale_input_ready_is_dropped_entirely() {
        let (mut app, _config, _store, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert!(!app.accept_input(42, text_input("来自未来")));
        assert_eq!(
            app.machine.state(),
            AppState::Idle,
            "stale input must not move state"
        );
        assert!(
            cmd_rx.try_recv().is_err(),
            "stale input must not reach tokio"
        );
    }

    #[test]
    fn saved_config_applies_to_the_next_trigger() {
        let (mut app, config, _store, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();

        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(1, text_input("A")));
        confirm(&mut app);
        let Command::RunTask { options, .. } = cmd_rx.try_recv().unwrap().payload else {
            panic!("expected a RunTask command, got another variant")
        };
        assert_eq!(options.target_lang, Some(Lang::Zh));
        assert_eq!(
            options.model.as_ref(),
            DEFAULT_MODEL,
            "the factory model freezes into the first task"
        );

        config
            .save(Config {
                target_lang: Lang::Ja,
                model: "deepseek-reasoner".into(),
                ..Default::default()
            })
            .expect("save should succeed");

        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(2, text_input("B")));
        confirm(&mut app);
        let Command::RunTask { options, .. } = cmd_rx.try_recv().unwrap().payload else {
            panic!("expected a RunTask command, got another variant")
        };
        assert_eq!(options.target_lang, Some(Lang::Ja));
        assert_eq!(
            options.model.as_ref(),
            "deepseek-reasoner",
            "the saved model freezes into the next task"
        );
    }

    #[test]
    fn saved_config_does_not_leak_into_the_inflight_task() {
        let (mut app, config, _store, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();

        trigger_selection(&mut app, &pe_tx);
        config
            .save(Config {
                target_lang: Lang::Ja,
                ..Default::default()
            })
            .expect("save should succeed");
        assert!(app.accept_input(1, text_input("A")));
        confirm(&mut app);

        let Command::RunTask { options, .. } = cmd_rx.try_recv().unwrap().payload else {
            panic!("expected a RunTask command, got another variant")
        };
        assert_eq!(
            options.target_lang,
            Some(Lang::Zh),
            "in-flight task must keep the snapshot taken at trigger"
        );

        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(2, text_input("B")));
        confirm(&mut app);
        let Command::RunTask { options, .. } = cmd_rx.try_recv().unwrap().payload else {
            panic!("expected a RunTask command, got another variant")
        };
        assert_eq!(options.target_lang, Some(Lang::Ja));
    }

    #[test]
    fn probe_empty_selection_is_silently_dropped() {
        let (mut app, _config, _store, pe_tx, _ac_rx, _cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert_eq!(
            app.machine.state(),
            AppState::Idle,
            "the probe never enters the state machine"
        );

        assert!(
            !app.accept_failed(1, &GlossError::SelectionUnavailable),
            "a pure mis-drag must not pop the overlay for a failure card"
        );
        assert_eq!(app.machine.state(), AppState::Idle);
        assert!(
            app.machine.overlay_view().is_none(),
            "nothing was ever shown, nothing needs withdrawing"
        );
        assert!(app.machine.probe_id().is_none(), "the probe is consumed");
    }

    #[test]
    fn a_mis_slide_over_a_visible_session_preserves_it_entirely() {
        let (mut app, _config, _store, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(1, text_input("正常选区")));
        confirm(&mut app);
        let Command::RunTask { cancel: token, .. } = cmd_rx.try_recv().unwrap().payload else {
            panic!("expected a RunTask command, got another variant")
        };
        assert!(
            app.session.pending_reveal,
            "the commit flagged the reveal for drain_events"
        );
        app.session.pending_reveal = false;
        assert!(app.accept_chunk(1, "流式正文".into()));
        let view_before = app.machine.overlay_view().cloned();
        let state_before = app.machine.state();

        trigger_selection(&mut app, &pe_tx);
        assert!(
            !token.is_cancelled(),
            "the probe must not cancel the stream"
        );
        assert!(
            !app.accept_failed(2, &GlossError::SelectionUnavailable),
            "a mis-slide over a visible session shows nothing new"
        );
        assert_eq!(app.machine.state(), state_before);
        assert_eq!(app.machine.overlay_view(), view_before.as_ref());
        assert!(!app.session.pending_reveal, "nothing new to reveal");
        assert!(app.machine.probe_id().is_none());

        assert!(app.accept_chunk(1, "、继续".into()));
        assert!(streaming_raw(&app).contains("继续"));
    }

    #[test]
    fn committing_the_probe_flags_the_reveal_for_the_same_frame() {
        let (mut app, _config, _store, pe_tx, _ac_rx, _cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert!(!app.session.pending_reveal, "probing never reveals");

        assert!(app.accept_input(1, text_input("内容到了")));
        assert!(
            app.session.pending_reveal,
            "the commit flags the reveal; drain_events consumes it the same frame"
        );
        assert_eq!(app.machine.state(), AppState::PendingConfirm);
    }

    #[test]
    fn the_pending_card_cancel_path_sends_no_channel3_message() {
        let (mut app, _config, _store, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(1, text_input("待确认的选区")));
        assert!(app.session.pending_reveal);
        app.session.pending_reveal = false;

        app.dismiss_overlay("cancel button");
        assert_eq!(app.machine.state(), AppState::Idle);
        assert!(
            cmd_rx.try_recv().is_err(),
            "Esc/×/取消 all collapse the pending card: tokio never hears about it"
        );
        assert!(
            !app.accept_done(1, plain_outcome("迟到的产物")),
            "a pending card holds no generation session to adopt late events"
        );
        confirm(&mut app);
        assert!(
            cmd_rx.try_recv().is_err(),
            "a stale confirmation after dismissal must not redispatch"
        );
    }

    #[test]
    fn suspicious_input_is_suppressed_and_shows_nothing() {
        let suspicious_text = format!("{{\"token\":\"sk-{}\"}}", "9f2b7c1d");
        let (mut app, _config, _store, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);

        assert!(
            !app.accept_input(1, text_input(&suspicious_text)),
            "a refused fetch must not ask for the overlay"
        );
        assert!(
            cmd_rx.try_recv().is_err(),
            "nothing may reach tokio — there is no confirmation path to wait for"
        );
        assert_eq!(app.machine.state(), AppState::Idle);
        assert!(
            app.machine.overlay_view().is_none(),
            "no card, no question: the guard is not a dialog"
        );

        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(2, text_input("普通的文本")));
        confirm(&mut app);
        assert!(matches!(
            cmd_rx.try_recv().unwrap().payload,
            Command::RunTask { generation: 2, .. }
        ));
    }

    #[test]
    fn pasteboard_commit_retains_the_image_and_flags_the_reveal() {
        let (mut app, _config, _store, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();
        pasteboard_probe(&mut app, &pe_tx);

        assert!(
            app.accept_input(1, image_input()),
            "the pasteboard product enters a task that needs the overlay"
        );
        assert_eq!(app.machine.state(), AppState::PendingConfirm);
        assert!(app.machine.attached_image().is_some());
        assert!(
            app.session.pending_reveal,
            "the commit flags the reveal; drain_events consumes it the same frame"
        );
        assert!(
            app.session
                .selection_anchor
                .is_none_or(|(generation, _)| generation != app.machine.generation()),
            "no anchor matches a pasteboard generation: the reveal centers"
        );
        assert!(
            cmd_rx.try_recv().is_err(),
            "no channel 3 message before the confirmation click"
        );
        confirm(&mut app);
        assert!(matches!(
            cmd_rx.try_recv().unwrap().payload,
            Command::RunTask {
                generation: 1,
                input: TaskInput::Image { .. },
                ..
            }
        ));
        assert!(matches!(
            app.machine.overlay_view(),
            Some(OverlayView::Streaming { .. })
        ));
    }

    #[test]
    fn a_pasteboard_product_without_a_matching_probe_is_dropped() {
        let (mut app, _config, _store, _pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();

        assert!(
            !app.accept_input(7, image_input()),
            "no outstanding probe: the product is stale"
        );
        assert_eq!(app.machine.state(), AppState::Idle);
        assert!(app.machine.attached_image().is_none());
        assert!(
            cmd_rx.try_recv().is_err(),
            "stale input must not reach tokio"
        );
    }

    #[test]
    fn pasteboard_oversized_failure_lands_in_the_error_card() {
        let (mut app, _config, _store, pe_tx, _ac_rx, _cmd_rx, _ev_tx) = driven_app();
        pasteboard_probe(&mut app, &pe_tx);

        assert!(
            app.accept_failed(1, &GlossError::ImageTooLarge),
            "an oversized image must be observable: the failure card pops"
        );
        assert_eq!(app.machine.state(), AppState::Error);
        assert!(
            app.machine.overlay_view().is_some(),
            "the card carries the oversized-image wording via the i18n mapping"
        );
        assert!(app.machine.probe_id().is_none());
    }
}
