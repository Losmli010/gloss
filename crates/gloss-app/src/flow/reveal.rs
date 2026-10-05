//! 编排用例·显隐段：浮层的显示入口与收起出口，以及「在哪显示」的定位
//! 决策。露面「要不要」的策略（挂起请求 + 失败即弹）在
//! `machine::should_reveal`；本模块执行「怎么显示」。

use gloss_core::log::{info, thread};
use gloss_core::model::ScreenPoint;
use winit::dpi::LogicalPosition;
use winit::event_loop::ActiveEventLoop;

use crate::app::GlossApp;
use crate::present::windows::{Placement, WindowManager};

impl GlossApp {
    /// 统一显示入口：显示并重置出现动画起点，让本次显示从淡入开始；
    /// 同时清上一轮的拖动状态，跨显示残留的拖动所有权不得带进本轮
    /// （隐藏期丢失的鼠标释放会让 egui 侧拖动所有权残留，按压点清掉后
    /// 热区保持惰性，见 `ui::popup::RenderState::reset_drag_state`）。
    /// 浮层常驻——收起只认 Esc、关闭按钮与新触发的内容替换。
    pub(crate) fn show_overlay(&mut self, position: LogicalPosition<f64>) {
        let Some(windows) = &self.windows else {
            return;
        };
        if let Some(frame) = self.frame.as_ref() {
            crate::ui::popup::reset_appear_animation(&frame.egui_ctx);
            frame.reset_overlay_drag();
        }
        windows.show_at(position);
    }

    /// 收起浮层的统一出口（Esc / 关闭按钮）：隐藏窗口、清
    /// 渲染截止时刻与拖动状态防空转防残留，状态机放弃在途任务回 `Idle`
    /// （迟到产物经代数或状态守卫丢弃——为一个不可见的浮层继续推理与
    /// 渲染纯属空转）。
    pub(crate) fn dismiss_overlay(&mut self, reason: &'static str) {
        info!(
            thread = thread::UI,
            generation = self.machine.generation(),
            reason,
            "overlay dismissed"
        );
        self.overlay_repaint = None;
        if let Some(frame) = self.frame.as_ref() {
            frame.reset_overlay_drag();
        }
        if let Some(windows) = &self.windows {
            windows.hide();
        }
        self.machine.hide_overlay();
    }
}

/// 浮层显示位置与摆放意图的决策：当前代数携带划词释放坐标时在选区
/// 附近露面（右下偏移一点，避免浮层压在光标/选区上）并记为定点摆放
/// ——后续内容撑高窗口时原锚点重新钳制而非被居中覆盖；否则居中
/// （陈旧代数同样回落居中）。
pub(super) fn show_position(
    anchor: Option<(u64, ScreenPoint)>,
    generation: u64,
    centered: LogicalPosition<f64>,
) -> (LogicalPosition<f64>, Placement) {
    match anchor.filter(|(anchored_gen, _)| *anchored_gen == generation) {
        Some((_, pos)) => {
            let position = LogicalPosition::new(
                f64::from(pos.x) + SELECTION_OFFSET_X,
                f64::from(pos.y) + SELECTION_OFFSET_Y,
            );
            (position, Placement::At(position))
        }
        None => (centered, Placement::Centered),
    }
}

/// 浮层跟随划词位置时的偏移（逻辑点）：让卡片略偏右下，不压住光标。
const SELECTION_OFFSET_X: f64 = 16.0;
const SELECTION_OFFSET_Y: f64 = 20.0;

/// 浮层居中于显示器（逻辑坐标）：优先窗口当前所在的显示器，其次主显示器。
/// 定位与尺寸的算法在窗口管理器（`WindowManager::centered_position`），
/// 这里是给壳与自检 handler 的稳定入口。
pub fn centered_position(
    event_loop: &ActiveEventLoop,
    windows: &WindowManager,
) -> LogicalPosition<f64> {
    windows.centered_position(event_loop)
}

#[cfg(test)]
mod tests {
    use gloss_core::model::ScreenPoint;
    use winit::dpi::LogicalPosition;

    use crate::app::test_support::{driven_app, plain_outcome, text_input, trigger_selection};
    use crate::channel::{Command, PlatformEvent};
    use crate::machine::AppState;
    use crate::present::windows::Placement;

    use super::show_position;

    #[test]
    fn show_position_follows_selection_only_for_the_current_generation() {
        let anchor = Some((1, ScreenPoint::new(100, 200)));
        let centered = LogicalPosition::new(500.0, 500.0);

        let (position, placement) = show_position(anchor, 1, centered);
        assert_eq!(
            position,
            LogicalPosition::new(116.0, 220.0),
            "当前代数的划词触发跟随选区（右下偏移）"
        );
        assert_eq!(
            placement,
            Placement::At(position),
            "跟随触发必须记为定点摆放"
        );

        let (position, placement) = show_position(anchor, 2, centered);
        assert_eq!(position, centered, "代数对不上（后续触发）回落居中");
        assert_eq!(placement, Placement::Centered);

        let (position, placement) = show_position(None, 1, centered);
        assert_eq!(position, centered, "无划词锚点回落居中");
        assert_eq!(placement, Placement::Centered);
    }

    #[test]
    fn selection_trigger_records_its_anchor_per_generation() {
        let (mut app, _config, _store, pe_tx, _ac_rx, _cmd_rx, _ev_tx) = driven_app();

        pe_tx
            .send(PlatformEvent::SelectionGesture {
                pos: ScreenPoint::new(123, 45),
            })
            .unwrap();
        app.drain_platform_events();
        assert_eq!(
            app.selection_anchor,
            Some((1, ScreenPoint::new(123, 45))),
            "划词触发记录释放坐标随代数"
        );

        pe_tx
            .send(PlatformEvent::SelectionGesture {
                pos: ScreenPoint::new(9, 9),
            })
            .unwrap();
        app.drain_platform_events();
        assert_eq!(
            app.selection_anchor,
            Some((2, ScreenPoint::new(9, 9))),
            "新触发推进代数并刷新锚点"
        );
    }

    #[test]
    fn dismiss_abandons_the_inflight_task_and_returns_to_idle() {
        let (mut app, _config, _store, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(1, text_input("A")));
        let Command::RunTask { cancel, .. } = cmd_rx.try_recv().unwrap().payload;

        app.dismiss_overlay("test");
        assert_eq!(app.machine.state(), AppState::Idle);
        assert!(
            cancel.is_cancelled(),
            "dismiss must cancel the in-flight task"
        );
        assert!(app.machine.overlay_view().is_none());
        assert!(
            !app.accept_done(1, plain_outcome("迟到结果")),
            "a late outcome after dismissal must be dropped"
        );
    }
}
