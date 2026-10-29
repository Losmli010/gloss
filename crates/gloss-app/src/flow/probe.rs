//! 编排用例·探测段：消费通道 1（平台事件）→ 场景闸门 → 产出取材命令
//! 发往通道 2。状态机不动、不显形——产物到达才由任务段提交。

use gloss_core::guard::SceneFacts;
use gloss_core::log::{Span, debug, info, task_span, thread, warn};

use crate::channel::{AcquireCommand, PlatformEvent, Traced};
use crate::machine::{TriggerDecision, trigger_decision};

use crate::app::GlossApp;

impl GlossApp {
    pub(crate) fn drain_platform_events(&mut self) {
        let config = self.env.config.snapshot();
        let events: Vec<PlatformEvent> = self
            .endpoints
            .as_ref()
            .map_or(Vec::new(), |e| e.platform_events.try_iter().collect());
        for event in events {
            if matches!(event, PlatformEvent::OpenSettingsRequested) {
                info!(thread = thread::UI, "settings open requested");
                self.open_settings();
                continue;
            }
            if matches!(event, PlatformEvent::AccessibilityGranted) {
                self.on_accessibility_granted();
                continue;
            }
            let scene = self.env.scene.facts();
            let Some(command) = (match &event {
                PlatformEvent::SelectionGesture { .. } => self.machine.begin_selection_probe(
                    &event,
                    &config,
                    self.env.system_locale,
                    &scene,
                ),
                PlatformEvent::PasteboardImageObserved => self.machine.begin_pasteboard_probe(
                    &event,
                    &config,
                    self.env.system_locale,
                    &scene,
                ),
                _ => None,
            }) else {
                log_unwired(&event, &scene);
                continue;
            };
            self.dispatch_probe(command, event, scene);
        }
    }

    fn dispatch_probe(&mut self, command: AcquireCommand, event: PlatformEvent, scene: SceneFacts) {
        let generation = command.generation();
        let span = task_span(generation);
        self.session.task_span = Some((generation, span.clone()));
        let entered_span = span.clone();
        let _entered = entered_span.enter();
        match event {
            PlatformEvent::SelectionGesture { pos } => {
                info!(
                    thread = thread::UI,
                    "selection probe dispatched as acquire command"
                );
                self.session.selection_anchor = Some((generation, pos));
                self.session.probe_front_app = scene.front_app;
            }
            PlatformEvent::PasteboardImageObserved => {
                info!(
                    thread = thread::UI,
                    "pasteboard image probe dispatched as acquire command"
                );
            }
            _ => {}
        }
        if !self.send_acquire(command, span) {
            self.machine.drop_probe();
            self.session.probe_front_app = None;
        }
    }

    fn send_acquire(&mut self, command: AcquireCommand, span: Span) -> bool {
        let generation = command.generation();
        let Some(endpoints) = &self.endpoints else {
            return false;
        };
        let traced = Traced {
            payload: command,
            span,
        };
        if endpoints.acquire_commands.send(traced).is_err() {
            warn!(
                thread = thread::UI,
                generation, "acquire channel closed, command dropped"
            );
            return false;
        }
        true
    }
}

fn log_unwired(event: &PlatformEvent, scene: &SceneFacts) {
    match trigger_decision(event, scene) {
        TriggerDecision::Blocked(block) => warn!(
            thread = thread::UI,
            reason = %block,
            "trigger suppressed by the sensitive content guard"
        ),
        TriggerDecision::SelfSuppressed => debug!(
            thread = thread::UI,
            "selection gesture ignored: gloss itself is the frontmost app"
        ),
        _ => debug!(
            thread = thread::UI,
            event = ?event,
            "platform event ignored: not wired yet"
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gloss_core::config::Config;
    use gloss_core::guard::{FrontApp, SceneFacts};
    use gloss_core::log::{capture_global, info, thread};
    use gloss_core::model::Lang;
    use gloss_core::ports::SceneProbe;

    use crate::app::test_support::{driven_app, driven_app_with_scene, trigger_selection};
    use crate::channel::{AcquireCommand, PlatformEvent};
    use crate::machine::AppState;
    use crate::stubs::ports::{MemoryConfigStore, StubSceneProbe};

    #[test]
    fn dispatched_acquire_carries_the_task_span() {
        let logs = capture_global();
        let (mut app, _config, _store, pe_tx, ac_rx, _cmd_rx, _ev_tx) = driven_app();

        trigger_selection(&mut app, &pe_tx);
        let job = ac_rx.try_recv().expect("acquire command dispatched");
        let _entered = job.span.enter();
        info!(thread = thread::EVENT, "probe");

        let text = logs.text();
        let probe = text
            .lines()
            .find(|line| line.contains(r#""message":"probe""#))
            .unwrap_or_default();
        assert!(probe.contains("\"generation\":1"), "{text}");
    }

    #[test]
    fn a_disabled_default_kind_does_not_stop_the_selection_gesture() {
        let (mut app, config, _store, pe_tx, ac_rx, _cmd_rx, _ev_tx) = driven_app();
        config
            .save(Config {
                target_lang: Lang::Ja,
                ..Default::default()
            })
            .expect("save should succeed");

        trigger_selection(&mut app, &pe_tx);
        assert!(
            matches!(
                ac_rx.try_recv().unwrap().payload,
                AcquireCommand::AcquireText { generation: 1 }
            ),
            "the gesture carries no explicit intent: acquisition is kind-free"
        );
    }

    #[test]
    fn a_sensitive_scene_makes_the_selection_gesture_a_no_op() {
        let scene = Arc::new(StubSceneProbe::default());
        let (mut app, _config, _store, pe_tx, ac_rx, _cmd_rx, _ev_tx) = driven_app_with_scene(
            Arc::new(MemoryConfigStore::default()),
            Arc::clone(&scene) as Arc<dyn SceneProbe>,
        );

        scene.set_facts(SceneFacts {
            secure_input: true,
            front_app: None,
        });
        trigger_selection(&mut app, &pe_tx);
        assert!(
            ac_rx.try_recv().is_err(),
            "a focused password field must stop the command before the event thread"
        );

        scene.set_facts(SceneFacts {
            secure_input: false,
            front_app: Some(FrontApp {
                bundle_id: Some("com.1password.1password".into()),
                name: None,
                is_self: false,
            }),
        });
        trigger_selection(&mut app, &pe_tx);
        assert!(
            ac_rx.try_recv().is_err(),
            "a listed frontmost app must stop the command too"
        );
        assert_eq!(
            app.machine.generation(),
            0,
            "a suppressed trigger keeps no generation"
        );
        assert_eq!(app.machine.state(), AppState::Idle);

        scene.set_facts(SceneFacts::default());
        trigger_selection(&mut app, &pe_tx);
        assert!(matches!(
            ac_rx.try_recv().unwrap().payload,
            AcquireCommand::AcquireText { generation: 1, .. }
        ));
    }

    #[test]
    fn a_pasteboard_observation_dispatches_the_clipboard_acquire_command() {
        let (mut app, _config, _store, pe_tx, ac_rx, _cmd_rx, _ev_tx) = driven_app();

        pe_tx
            .send(PlatformEvent::PasteboardImageObserved)
            .expect("platform channel open");
        app.drain_platform_events();

        assert_eq!(
            app.machine.state(),
            AppState::Idle,
            "the probe leaves the state machine alone until content arrives"
        );
        assert_eq!(app.machine.generation(), 0);
        assert_eq!(app.machine.probe_id(), Some(1));
        assert!(
            matches!(
                ac_rx
                    .try_recv()
                    .expect("acquire command dispatched")
                    .payload,
                AcquireCommand::AcquireClipboardImage { generation: 1 }
            ),
            "the observation acquires the clipboard image"
        );
        assert!(
            app.session.selection_anchor.is_none(),
            "a pasteboard probe records no anchor: the reveal falls back to centering"
        );
    }

    #[test]
    fn a_sensitive_scene_silences_the_pasteboard_observation() {
        let scene = Arc::new(StubSceneProbe::default());
        let (mut app, _config, _store, pe_tx, ac_rx, _cmd_rx, _ev_tx) = driven_app_with_scene(
            Arc::new(MemoryConfigStore::default()),
            Arc::clone(&scene) as Arc<dyn SceneProbe>,
        );

        scene.set_facts(SceneFacts {
            secure_input: true,
            front_app: None,
        });
        pe_tx
            .send(PlatformEvent::PasteboardImageObserved)
            .expect("platform channel open");
        app.drain_platform_events();

        assert!(
            ac_rx.try_recv().is_err(),
            "a focused password field must stop the clipboard probe"
        );
        assert_eq!(app.machine.generation(), 0);
        assert_eq!(app.machine.probe_id(), None);
        assert_eq!(app.machine.state(), AppState::Idle);

        scene.set_facts(SceneFacts {
            secure_input: false,
            front_app: Some(FrontApp {
                bundle_id: Some("com.1password.1password".into()),
                name: None,
                is_self: false,
            }),
        });
        pe_tx
            .send(PlatformEvent::PasteboardImageObserved)
            .expect("platform channel open");
        app.drain_platform_events();
        assert!(
            ac_rx.try_recv().is_err(),
            "a listed frontmost app must stop the clipboard probe too"
        );

        scene.set_facts(SceneFacts::default());
        pe_tx
            .send(PlatformEvent::PasteboardImageObserved)
            .expect("platform channel open");
        app.drain_platform_events();
        assert!(matches!(
            ac_rx.try_recv().unwrap().payload,
            AcquireCommand::AcquireClipboardImage { generation: 1 }
        ));
    }
}
