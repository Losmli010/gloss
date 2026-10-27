//! 编排用例·动作段：执行浮层一帧上交的动作（错误卡的壳侧半边、待确认卡的
//! 确认与取消、完成卡的复制钮与头部动作区）；设置窗动作经编辑会话执行。

use gloss_core::log::{debug, info, thread, warn};

use gloss_platform::clipboard::copy_text;

use crate::app::GlossApp;
use crate::machine::OverlayView;
use crate::ui::popup::{OverlayAction, annotation_text};

impl GlossApp {
    pub(crate) fn handle_overlay_action(&mut self, action: OverlayAction) {
        match action {
            OverlayAction::Confirm => match self.machine.confirm_pending() {
                Some(request) => {
                    info!(
                        thread = thread::UI,
                        generation = request.generation,
                        "confirmation accepted, task dispatched to tokio"
                    );
                    self.send_run(request);
                }
                None => {
                    debug!(
                        thread = thread::UI,
                        generation = self.machine.generation(),
                        state = ?self.machine.state(),
                        "stale confirm click dropped"
                    );
                }
            },
            OverlayAction::CopyAnnotation => self.copy_annotation(),
            OverlayAction::Retry => match self.machine.retry() {
                Some(request) => {
                    info!(
                        thread = thread::UI,
                        generation = request.generation,
                        "retry accepted, task re-dispatched to tokio"
                    );
                    self.send_run(request);
                }
                None => {
                    debug!(
                        thread = thread::UI,
                        generation = self.machine.generation(),
                        state = ?self.machine.state(),
                        "stale retry click dropped"
                    );
                }
            },
            OverlayAction::OpenSettings => self.open_settings(),
            OverlayAction::Dismiss => self.dismiss_overlay("close button"),
        }
    }

    fn copy_annotation(&mut self) {
        let Some(text) = self.machine.overlay_view().and_then(|view| match view {
            OverlayView::Outcome { outcome, .. } => Some(annotation_text(outcome)),
            _ => None,
        }) else {
            debug!(
                thread = thread::UI,
                state = ?self.machine.state(),
                "stale copy click dropped"
            );
            return;
        };
        match copy_text(&text) {
            Ok(()) => info!(thread = thread::UI, "annotation copied to the pasteboard"),
            Err(err) => warn!(
                thread = thread::UI,
                error = %err,
                "pasteboard write failed, copy annotation degraded"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use gloss_core::model::GlossError;
    use gloss_core::task::TaskInput;

    use crate::app::test_support::{driven_app, plain_outcome, text_input, trigger_selection};
    use crate::channel::Command;
    use crate::machine::{AppState, ErrorAction, OverlayView};
    use crate::ui::popup::OverlayAction;

    #[test]
    fn confirm_action_dispatches_the_pending_card() {
        let (mut app, _config, _store, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(1, text_input("A")));
        assert_eq!(app.machine.state(), AppState::PendingConfirm);
        assert!(
            cmd_rx.try_recv().is_err(),
            "no channel 3 message may precede the confirmation click"
        );

        app.handle_overlay_action(OverlayAction::Confirm);
        assert_eq!(app.machine.state(), AppState::Translating);
        let Command::RunTask { generation, .. } = cmd_rx.try_recv().unwrap().payload else {
            panic!("expected a RunTask command, got another variant")
        };
        assert_eq!(generation, 1, "confirmation keeps the promoted generation");
    }

    #[test]
    fn stale_confirm_clicks_dispatch_nothing() {
        let (mut app, _config, _store, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();

        app.handle_overlay_action(OverlayAction::Confirm);
        assert!(cmd_rx.try_recv().is_err(), "no card, no dispatch");

        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(1, text_input("A")));
        app.handle_overlay_action(OverlayAction::Confirm);
        app.handle_overlay_action(OverlayAction::Confirm);
        let Command::RunTask { generation, .. } = cmd_rx.try_recv().unwrap().payload else {
            panic!("expected a RunTask command, got another variant")
        };
        assert_eq!(generation, 1);
        assert!(
            cmd_rx.try_recv().is_err(),
            "the second confirmation on a consumed card must be refused"
        );
    }

    #[test]
    fn retry_action_redispatches_the_failed_task() {
        let (mut app, _config, _store, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(1, text_input("A")));
        app.handle_overlay_action(OverlayAction::Confirm);
        let Command::RunTask { cancel, .. } = cmd_rx.try_recv().unwrap().payload else {
            panic!("expected a RunTask command, got another variant")
        };
        assert!(app.accept_failed(1, &GlossError::EngineNetwork));
        assert!(matches!(
            app.machine.overlay_view(),
            Some(OverlayView::Failed {
                action: Some(ErrorAction::Retry),
                ..
            })
        ));

        app.handle_overlay_action(OverlayAction::Retry);
        assert_eq!(app.machine.state(), AppState::Translating);
        let Command::RunTask {
            generation,
            input,
            options: _,
            cancel: retried,
        } = cmd_rx.try_recv().unwrap().payload
        else {
            panic!("expected a RunTask command, got another variant")
        };
        assert_eq!(generation, 1, "retry keeps the failed task's generation");
        assert!(matches!(
            input,
            TaskInput::Text { ref text, .. } if text == "A"
        ));
        assert!(!retried.is_cancelled());
        assert!(
            !cancel.is_cancelled(),
            "a failed task's token is dropped, not cancelled"
        );

        assert!(app.accept_done(1, plain_outcome("重试成功")));
        assert_eq!(app.machine.state(), AppState::Show);
    }

    #[test]
    fn open_settings_action_keeps_the_error_card() {
        let (mut app, _config, _store, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(1, text_input("A")));
        app.handle_overlay_action(OverlayAction::Confirm);
        let Command::RunTask { .. } = cmd_rx.try_recv().unwrap().payload else {
            panic!("expected a RunTask command, got another variant")
        };
        assert!(app.accept_failed(1, &GlossError::EngineAuth));
        assert!(matches!(
            app.machine.overlay_view(),
            Some(OverlayView::Failed {
                action: Some(ErrorAction::OpenSettings),
                ..
            })
        ));

        app.handle_overlay_action(OverlayAction::OpenSettings);
        assert_eq!(app.machine.state(), AppState::Error);
        assert!(cmd_rx.try_recv().is_err(), "no re-dispatch for settings");
        assert!(
            app.settings.is_some(),
            "the open-settings action must start the edit session"
        );
    }
}
