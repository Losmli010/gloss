//! 编排用例·动作段：执行浮层一帧上交的动作（错误卡的壳侧半边 + 头部
//! 动作区）；设置窗动作经编辑会话（`settings_session`）执行。

use gloss_core::log::{debug, info, thread};

use crate::app::GlossApp;
use crate::ui::popup::OverlayAction;

impl GlossApp {
    /// 执行浮层一帧上交的动作（错误映射的壳侧半边 + 头部动作区）。
    pub(crate) fn handle_overlay_action(&mut self, action: OverlayAction) {
        match action {
            OverlayAction::Retry => match self.machine.retry() {
                Some(request) => {
                    info!(
                        thread = thread::UI,
                        generation = request.generation,
                        "error card retry, task re-dispatched to tokio"
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
            // 失败卡的「打开设置」与头部齿轮、托盘走同一个入口。
            OverlayAction::OpenSettings => self.open_settings(),
            OverlayAction::Dismiss => self.dismiss_overlay("close button"),
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
    fn retry_action_redispatches_the_failed_task() {
        let (mut app, _config, _store, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(1, text_input("A")));
        let Command::RunTask { cancel, .. } = cmd_rx.try_recv().unwrap().payload;
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
        } = cmd_rx.try_recv().unwrap().payload;
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
        let Command::RunTask { .. } = cmd_rx.try_recv().unwrap().payload;
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
