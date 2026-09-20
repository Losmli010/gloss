//! 浮层显隐：显示入口、自动隐藏计时与失败卡动作出口，附 auto_show
//! 露面策略的纯函数。

use std::time::{Duration, Instant};

use gloss_core::log::{debug, info, thread};
use winit::dpi::LogicalPosition;
use winit::event_loop::ActiveEventLoop;

use crate::channel::Event;
use crate::machine::{AppState, ErrorAction};
use crate::windows::WindowManager;

use super::GlossApp;

/// 浮层显示后的自动隐藏时长（超时回 Idle；失焦路径走 Focused 事件）
pub(super) const AUTO_HIDE_AFTER: Duration = Duration::from_secs(10);

impl GlossApp {
    /// 统一显示入口：显示并启动自动隐藏计时。
    pub(super) fn show_overlay(&mut self, position: LogicalPosition<f64>) {
        let Some(windows) = &self.windows else {
            return;
        };
        windows.show_at(position);
        self.auto_hide = Some(Instant::now() + AUTO_HIDE_AFTER);
    }

    /// 自动隐藏到点：收起浮层并回落 Idle；推理中（Translating）不收起，
    /// 只把计时顺延一个周期，等 TaskDone/TaskFailed 落地后正常收起。Esc/
    /// 点击外部等显式隐藏不走此路径，仍立即收起。
    pub(super) fn on_auto_hide(&mut self, _event_loop: &ActiveEventLoop) {
        if self.machine.state() == AppState::Translating {
            self.auto_hide = Some(Instant::now() + AUTO_HIDE_AFTER);
            return;
        }
        self.auto_hide = None;
        if let Some(windows) = &self.windows {
            windows.hide();
        }
        self.machine.hide_overlay();
    }

    /// 执行失败卡的动作出口（错误映射的壳侧半边）。
    pub(super) fn handle_error_action(&mut self, action: ErrorAction) {
        match action {
            ErrorAction::Retry => match self.machine.retry() {
                Some(request) => {
                    info!(
                        thread = thread::UI,
                        generation = request.generation,
                        "error card retry, task re-dispatched to tokio"
                    );
                    self.send_run(request);
                    // 重锚隐藏计时（与 accept_done 的重锚同一理由）。
                    if self.windows.is_some() {
                        self.auto_hide = Some(Instant::now() + AUTO_HIDE_AFTER);
                    }
                }
                None => {
                    debug!(
                        thread = thread::UI,
                        state = ?self.machine.state(),
                        "stale retry click dropped"
                    );
                }
            },
            // 失败卡的「打开设置」与托盘/热键走同一个入口。
            ErrorAction::OpenSettings => self.open_settings(),
        }
    }
}

/// 浮层居中于显示器（逻辑坐标）：优先窗口当前所在的显示器，其次主显示器。
/// 定位与尺寸的算法在窗口管理器（`WindowManager::centered_position`），
/// 这里是给壳与自检 handler 的稳定入口。
pub fn centered_position(
    event_loop: &ActiveEventLoop,
    windows: &WindowManager,
) -> LogicalPosition<f64> {
    windows.centered_position(event_loop)
}

/// 回传事件的类别标签（`auto_show` 策略只关心类别，不关心代数与载荷）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EventKind {
    InputReady,
    TaskChunk,
    TaskDone,
    TaskFailed,
}

/// 取回传事件的类别标签。
pub(super) fn event_kind(event: &Event) -> EventKind {
    match event {
        Event::InputReady { .. } => EventKind::InputReady,
        Event::TaskChunk { .. } => EventKind::TaskChunk,
        Event::TaskDone { .. } => EventKind::TaskDone,
        Event::TaskFailed { .. } => EventKind::TaskFailed,
    }
}

/// 单个回传事件后浮层要不要自动露面（`auto_show` 策略）。
///
/// `accepted` 是状态机是否采纳了该事件：陈旧事件不触发显示。
fn auto_show_for(kind: EventKind, auto_show: bool, accepted: bool) -> bool {
    match kind {
        EventKind::InputReady => accepted && auto_show,
        EventKind::TaskDone => accepted && !auto_show,
        EventKind::TaskChunk => false,
        EventKind::TaskFailed => accepted,
    }
}

/// 一批回传之后浮层要不要自动露面：**任一**事件判为要显示就显示。
pub(super) fn auto_show_after(
    batch: impl IntoIterator<Item = (EventKind, bool)>,
    auto_show: bool,
) -> bool {
    batch
        .into_iter()
        .any(|(kind, accepted)| auto_show_for(kind, auto_show, accepted))
}

#[cfg(test)]
mod tests {
    use gloss_core::task::TaskInput;

    use crate::app::test_support::{driven_app, plain_outcome, text_input, trigger_selection};
    use crate::channel::Command;
    use crate::machine::{AppState, ErrorAction, OverlayView};

    use super::{EventKind, auto_show_after, auto_show_for};

    #[test]
    fn retry_action_redispatches_the_failed_task() {
        let (mut app, _config, _store, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(1, text_input("A")));
        let Command::RunTask { cancel, .. } = cmd_rx.try_recv().unwrap();
        assert!(app.accept_failed(1, &gloss_core::model::GlossError::EngineNetwork));
        assert!(matches!(
            app.machine.overlay_view(),
            Some(OverlayView::Failed {
                action: Some(ErrorAction::Retry),
                ..
            })
        ));

        app.handle_error_action(ErrorAction::Retry);
        assert_eq!(app.machine.state(), AppState::Translating);
        let Command::RunTask {
            generation,
            task,
            cancel: retried,
        } = cmd_rx.try_recv().unwrap();
        assert_eq!(generation, 1, "retry keeps the failed task's generation");
        assert!(matches!(
            task.input,
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
        let Command::RunTask { .. } = cmd_rx.try_recv().unwrap();
        assert!(app.accept_failed(1, &gloss_core::model::GlossError::EngineAuth));
        assert!(matches!(
            app.machine.overlay_view(),
            Some(OverlayView::Failed {
                action: Some(ErrorAction::OpenSettings),
                ..
            })
        ));

        app.handle_error_action(ErrorAction::OpenSettings);
        assert_eq!(app.machine.state(), AppState::Error);
        assert!(cmd_rx.try_recv().is_err(), "no re-dispatch for settings");
        assert!(
            app.settings.is_some(),
            "the open-settings action must start the edit session"
        );
    }

    #[test]
    fn auto_show_policy_decides_when_the_overlay_pops() {
        use EventKind::{InputReady, TaskChunk, TaskDone, TaskFailed};

        assert!(auto_show_for(InputReady, true, true));
        assert!(
            !auto_show_for(TaskDone, true, true),
            "the overlay is already up since input ready"
        );

        assert!(!auto_show_for(InputReady, false, true), "取材阶段不打扰");
        assert!(auto_show_for(TaskDone, false, true), "完成才露面");
        assert!(
            auto_show_for(TaskFailed, false, true),
            "关掉开关也不该把错误吞掉"
        );
        assert!(auto_show_for(TaskFailed, true, true));

        for kind in [InputReady, TaskChunk, TaskDone, TaskFailed] {
            assert!(
                !auto_show_for(kind, true, false) && !auto_show_for(kind, false, false),
                "未被采纳的 {kind:?} 不得触发显示"
            );
        }
        assert!(!auto_show_for(TaskChunk, true, true));
        assert!(!auto_show_for(TaskChunk, false, true));
    }

    #[test]
    fn auto_show_survives_a_mixed_batch() {
        use EventKind::{InputReady, TaskChunk, TaskDone, TaskFailed};

        assert!(
            auto_show_after([(TaskChunk, true), (TaskDone, true)], false),
            "关掉开关时，同一批里的完成那一条仍要把浮层带出来"
        );
        assert!(
            auto_show_after([(TaskDone, false), (TaskFailed, true)], false),
            "陈旧的成功不得抵消一条被采纳的失败"
        );
        assert!(
            auto_show_after([(TaskFailed, true), (InputReady, true)], true),
            "失败与取材同批到达照常露面"
        );
        assert!(
            !auto_show_after([(TaskDone, false), (InputReady, false)], true),
            "整批都没被采纳（陈旧）→ 不显示：迟到的产物不得把浮层弹回来"
        );
        assert!(!auto_show_after([], true), "空批不显示");
    }
}
