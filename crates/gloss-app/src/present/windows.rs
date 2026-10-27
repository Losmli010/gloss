//! 窗口管理器：浮层（预创建复用，只显隐不反复销毁）与设置窗口
//!（普通带标题栏窗口，关闭即隐藏）。

use std::sync::Arc;

use gloss_core::log::{thread, warn};
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::error::OsError;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId, WindowLevel};

use crate::ui::style::geometry::window;

const APP_NAME: &str = "Gloss";
const HEIGHT_CAP_FRACTION: f64 = 0.382;
const RESIZE_EPSILON: f64 = 0.5;
const DRAG_POSITION_EPSILON: f64 = 0.5;

fn debounced_size(current: LogicalSize<f64>, requested: LogicalSize<f64>) -> LogicalSize<f64> {
    let steps = (requested.height / window::STREAM_HEIGHT_STEP).ceil();
    let stepped = (steps * window::STREAM_HEIGHT_STEP).max(current.height);
    LogicalSize::new(current.width, stepped)
}

fn capped_height(screen_height: f64, requested_height: f64) -> f64 {
    let max_height = (screen_height * HEIGHT_CAP_FRACTION - window::WORK_AREA_MARGIN)
        .max(window::OVERLAY_HEIGHT);
    requested_height.min(max_height)
}

fn centered_on_monitor(
    monitor: &winit::monitor::MonitorHandle,
    size: LogicalSize<f64>,
) -> LogicalPosition<f64> {
    let scale = monitor.scale_factor();
    let monitor_size = monitor.size().to_logical::<f64>(scale);
    let origin = monitor.position();
    LogicalPosition::new(
        f64::from(origin.x) + (monitor_size.width - size.width) / 2.0,
        f64::from(origin.y) + (monitor_size.height - size.height) / 2.0,
    )
}

fn clamp_to_monitor(
    position: LogicalPosition<f64>,
    origin: LogicalPosition<f64>,
    screen: LogicalSize<f64>,
    overlay: LogicalSize<f64>,
) -> LogicalPosition<f64> {
    let max_x = origin.x + (screen.width - overlay.width).max(0.0);
    let max_y = origin.y + (screen.height - overlay.height).max(0.0);
    LogicalPosition::new(
        position.x.clamp(origin.x, max_x),
        position.y.clamp(origin.y, max_y),
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Placement {
    Centered,
    At(LogicalPosition<f64>),
}

pub struct WindowManager {
    overlay: Arc<Window>,
    overlay_size: LogicalSize<f64>,
    placement: Placement,
    settings: Arc<Window>,
}

impl WindowManager {
    pub fn new(event_loop: &ActiveEventLoop) -> Result<Self, OsError> {
        let overlay = event_loop.create_window(
            Window::default_attributes()
                .with_title(APP_NAME)
                .with_inner_size(LogicalSize::new(
                    window::OVERLAY_WIDTH,
                    window::OVERLAY_HEIGHT,
                ))
                .with_decorations(false)
                .with_window_level(WindowLevel::AlwaysOnTop)
                .with_resizable(false)
                .with_transparent(true),
        )?;
        overlay.set_visible(false);

        let settings = event_loop.create_window(
            Window::default_attributes()
                .with_title(APP_NAME)
                .with_inner_size(LogicalSize::new(
                    window::SETTINGS_WIDTH,
                    window::SETTINGS_HEIGHT,
                ))
                .with_window_level(WindowLevel::AlwaysOnTop)
                .with_resizable(true)
                .with_min_inner_size(LogicalSize::new(380.0, 420.0)),
        )?;
        settings.set_visible(false);

        Ok(Self {
            overlay: Arc::new(overlay),
            overlay_size: LogicalSize::new(window::OVERLAY_WIDTH, window::OVERLAY_HEIGHT),
            placement: Placement::Centered,
            settings: Arc::new(settings),
        })
    }

    pub fn overlay(&self) -> &Window {
        &self.overlay
    }

    pub fn set_placement(&mut self, placement: Placement) {
        self.placement = placement;
    }

    pub fn set_overlay_size(&mut self, size: LogicalSize<f64>, debounced: bool) {
        let sized = if debounced {
            debounced_size(self.overlay_size, size)
        } else {
            size
        };
        let capped = self.cap_height_to_screen(sized);
        if (capped.width - self.overlay_size.width).abs() < RESIZE_EPSILON
            && (capped.height - self.overlay_size.height).abs() < RESIZE_EPSILON
        {
            return;
        }
        if let Some(applied) = self.overlay.request_inner_size(capped) {
            let scale = self.overlay.scale_factor();
            self.overlay_size = applied.to_logical::<f64>(scale);
        } else {
            self.overlay_size = capped;
        }
        match self.placement {
            Placement::Centered => {
                if let Some(monitor) = self.overlay.current_monitor() {
                    let position = centered_on_monitor(&monitor, self.overlay_size);
                    self.overlay.set_outer_position(position);
                }
            }
            Placement::At(anchor) => {
                let position = self.clamp_position(anchor);
                self.overlay.set_outer_position(position);
            }
        }
    }

    pub fn apply_overlay_drag(&mut self, offset: (f64, f64)) {
        let physical = match self.overlay.outer_position() {
            Ok(physical) => physical,
            Err(error) => {
                warn!(
                    thread = thread::UI,
                    error = %error,
                    "overlay position unavailable, drag frame skipped"
                );
                return;
            }
        };
        let scale = self.overlay.scale_factor();
        let current = physical.to_logical::<f64>(scale);
        let target = LogicalPosition::new(current.x + offset.0, current.y + offset.1);
        let clamped = self.clamp_position(target);
        if (clamped.x - current.x).abs() < DRAG_POSITION_EPSILON
            && (clamped.y - current.y).abs() < DRAG_POSITION_EPSILON
        {
            return;
        }
        self.overlay.set_outer_position(clamped);
        self.placement = Placement::At(clamped);
    }

    pub fn centered_position(&self, event_loop: &ActiveEventLoop) -> LogicalPosition<f64> {
        let monitor = self
            .overlay
            .current_monitor()
            .or_else(|| event_loop.primary_monitor());
        monitor.map_or(LogicalPosition::new(0.0, 0.0), |monitor| {
            centered_on_monitor(&monitor, self.overlay_size)
        })
    }

    pub fn clamp_position(&self, position: LogicalPosition<f64>) -> LogicalPosition<f64> {
        let monitor = self
            .monitor_containing(position)
            .or_else(|| self.overlay.current_monitor());
        let Some(monitor) = monitor else {
            return position;
        };
        let scale = monitor.scale_factor();
        let screen = monitor.size().to_logical::<f64>(scale);
        let origin = monitor.position();
        clamp_to_monitor(
            position,
            LogicalPosition::new(f64::from(origin.x), f64::from(origin.y)),
            screen,
            self.overlay_size,
        )
    }

    fn monitor_containing(
        &self,
        position: LogicalPosition<f64>,
    ) -> Option<winit::monitor::MonitorHandle> {
        self.overlay
            .available_monitors()
            .find(|monitor| {
                let origin = monitor.position();
                let scale = monitor.scale_factor();
                let size = monitor.size().to_logical::<f64>(scale);
                let (x, y) = (f64::from(origin.x), f64::from(origin.y));
                position.x >= x
                    && position.x < x + size.width
                    && position.y >= y
                    && position.y < y + size.height
            })
            .or_else(|| self.overlay.current_monitor())
    }

    fn cap_height_to_screen(&self, size: LogicalSize<f64>) -> LogicalSize<f64> {
        let Some(monitor) = self.overlay.current_monitor() else {
            return size;
        };
        let scale = monitor.scale_factor();
        let screen_height = monitor.size().to_logical::<f64>(scale).height;
        LogicalSize::new(size.width, capped_height(screen_height, size.height))
    }

    pub fn show_at(&self, position: LogicalPosition<f64>) {
        self.overlay.set_outer_position(position);
        self.overlay.set_visible(true);
    }

    pub fn hide(&self) {
        self.overlay.set_visible(false);
    }

    pub fn is_visible(&self) -> bool {
        self.overlay.is_visible().unwrap_or(false)
    }

    pub fn handle_refcount(&self) -> usize {
        Arc::strong_count(&self.overlay)
    }

    pub fn overlay_handle(&self) -> Arc<Window> {
        Arc::clone(&self.overlay)
    }

    pub fn settings_handle(&self) -> Arc<Window> {
        Arc::clone(&self.settings)
    }

    pub fn show_settings(&self, title: &str) {
        self.settings.set_title(title);
        self.settings.set_visible(true);
        self.settings.focus_window();
    }

    pub fn hide_settings(&self) {
        self.settings.set_visible(false);
    }

    pub fn is_settings_visible(&self) -> bool {
        self.settings.is_visible().unwrap_or(false)
    }

    pub fn matches_overlay(&self, id: WindowId) -> bool {
        self.overlay.id() == id
    }

    pub fn matches_settings(&self, id: WindowId) -> bool {
        self.settings.id() == id
    }

    pub fn request_redraw(&self) {
        self.overlay.request_redraw();
    }

    pub fn request_redraw_settings(&self) {
        self.settings.request_redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::clamp_to_monitor;
    use winit::dpi::{LogicalPosition, LogicalSize};

    const SCREEN: LogicalSize<f64> = LogicalSize::new(1920.0, 1080.0);
    const OVERLAY: LogicalSize<f64> = LogicalSize::new(480.0, 296.0);

    #[test]
    fn position_inside_the_monitor_is_untouched() {
        let origin = LogicalPosition::new(0.0, 0.0);
        let pos = LogicalPosition::new(500.0, 300.0);
        assert_eq!(clamp_to_monitor(pos, origin, SCREEN, OVERLAY), pos);
    }

    #[test]
    fn position_past_the_right_or_bottom_edge_pulls_back() {
        let origin = LogicalPosition::new(0.0, 0.0);
        assert_eq!(
            clamp_to_monitor(LogicalPosition::new(1900.0, 300.0), origin, SCREEN, OVERLAY),
            LogicalPosition::new(1440.0, 300.0),
            "右越界收到 1920-480"
        );
        assert_eq!(
            clamp_to_monitor(LogicalPosition::new(500.0, 2000.0), origin, SCREEN, OVERLAY),
            LogicalPosition::new(500.0, 784.0),
            "下越界收到 1080-296"
        );
    }

    #[test]
    fn position_before_the_origin_clamps_to_it() {
        let origin = LogicalPosition::new(1440.0, 0.0);
        assert_eq!(
            clamp_to_monitor(LogicalPosition::new(-50.0, 300.0), origin, SCREEN, OVERLAY),
            LogicalPosition::new(1440.0, 300.0),
            "越出该屏左缘的全局坐标钳到屏原点，不跳到别的屏"
        );
    }

    #[test]
    fn clamping_respects_the_monitor_origin_on_secondary_displays() {
        let origin = LogicalPosition::new(1440.0, 0.0);
        assert_eq!(
            clamp_to_monitor(LogicalPosition::new(3200.0, 300.0), origin, SCREEN, OVERLAY),
            LogicalPosition::new(2880.0, 300.0),
            "副屏右缘按 1440+1920-480 收，不是主屏的 1440"
        );
    }

    #[test]
    fn monitor_smaller_than_the_overlay_pins_to_the_origin() {
        let origin = LogicalPosition::new(1440.0, 0.0);
        assert_eq!(
            clamp_to_monitor(
                LogicalPosition::new(1500.0, 300.0),
                origin,
                LogicalSize::new(300.0, 200.0),
                OVERLAY
            ),
            LogicalPosition::new(1440.0, 0.0),
            "显示器比浮层还小时贴原点（max 取 0）"
        );
    }
}

#[cfg(test)]
mod cap_height_tests {
    use super::capped_height;
    use crate::ui::style::geometry::window::{OVERLAY_HEIGHT, WORK_AREA_MARGIN};

    #[test]
    fn height_caps_at_the_golden_share_of_the_screen_minus_the_margin() {
        assert_eq!(
            capped_height(1080.0, 2000.0),
            1080.0 * 0.382 - WORK_AREA_MARGIN,
            "316.56 on a 1080-logical-point display"
        );
    }

    #[test]
    fn smaller_requests_pass_through_untouched() {
        assert_eq!(capped_height(1080.0, 300.0), 300.0);
    }

    #[test]
    fn short_screens_bottom_out_at_the_default_height() {
        assert_eq!(capped_height(300.0, 2000.0), OVERLAY_HEIGHT);
    }
}

#[cfg(test)]
mod debounce_tests {
    use crate::ui::style::geometry::window::{OVERLAY_HEIGHT, STREAM_HEIGHT_STEP};

    use super::debounced_size;
    use winit::dpi::LogicalSize;

    const CURRENT: LogicalSize<f64> = LogicalSize::new(480.0, OVERLAY_HEIGHT);

    #[test]
    fn width_never_changes_during_streaming() {
        let grown = debounced_size(CURRENT, LogicalSize::new(520.0, 400.0));
        assert_eq!(
            grown.width, 480.0,
            "the popup width is fixed: streaming must not widen or narrow it"
        );
        assert_eq!(
            grown.height, 432.0,
            "height quantizes up to a step multiple"
        );
    }

    #[test]
    fn height_steps_up_in_quantized_increments() {
        let grown = debounced_size(CURRENT, LogicalSize::new(480.0, 310.0));
        assert_eq!(
            grown.height, 336.0,
            "one step past the current height is a full step"
        );
        assert_eq!(STREAM_HEIGHT_STEP, 48.0);
    }

    #[test]
    fn height_never_shrinks_during_streaming() {
        let shrunk = debounced_size(CURRENT, LogicalSize::new(480.0, 80.0));
        assert_eq!(
            shrunk.height, OVERLAY_HEIGHT,
            "content shrinkage waits for the final relayout"
        );
    }

    #[test]
    fn current_size_quantizes_up_to_the_step_boundary() {
        let same = debounced_size(CURRENT, CURRENT);
        assert_eq!(
            same.height, 336.0,
            "the first streaming frame lands on the step boundary above the skeleton"
        );
        assert_eq!(same.width, CURRENT.width);
    }
}
