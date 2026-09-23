//! 窗口管理器：浮层（预创建复用，只显隐不反复销毁）+ 设置窗口
//!（普通带标题栏窗口，关闭即隐藏）。

use std::sync::Arc;

use winit::dpi::{LogicalPosition, LogicalSize};
use winit::error::OsError;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId, WindowLevel};

/// 应用名：浮层标题与设置窗创建期的占位标题（品牌不进文案表，不翻译）。
const APP_NAME: &str = "Gloss";
/// 浮层默认宽度（默认 380px，长文本自适应上限 480px，上限由 popup 模块定）
const OVERLAY_WIDTH: f64 = 380.0;
/// 浮层默认高度：自检期只有渲染面板，按内容给一个紧凑初值
const OVERLAY_HEIGHT: f64 = 200.0;
/// 自适应高度的屏幕余量：winit 不提供工作区（work area），按显示器逻辑
/// 高减去该值近似（菜单栏/Dock 的保守估计）。
const WORK_AREA_MARGIN: f64 = 96.0;
/// 浮层尺寸变化小于该阈值不重设窗口（防 egui 布局与 winit resize 的
/// 帧迟滞来回抖动）。
const RESIZE_EPSILON: f64 = 0.5;
/// 设置窗口尺寸：全部配置区块一屏放下的紧凑初值（可拖拽调整）。
const SETTINGS_WIDTH: f64 = 460.0;
const SETTINGS_HEIGHT: f64 = 640.0;

/// 浮层在指定显示器上居中的全局桌面坐标（winit 的窗口定位口径是跨显示
/// 器的桌面坐标，主显示器左上为原点；显示器尺寸按各自缩放比例换算）。
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

/// 把期望位置钳制进「原点在 `origin`、逻辑尺寸 `screen` 的显示器」内，
/// 以浮层尺寸 `overlay` 为界（右/下越界向内收，显示器比浮层还小时贴
/// 原点）。坐标是全局桌面坐标，与 winit 窗口定位口径一致。
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

/// 浮层摆放意图——尺寸自适应变化时的重定位依据：居中者在尺寸变化后
/// 重新居中（显示入口只能按上一帧尺寸算位置）；定点者（跟随划词）在
/// 原锚点上按新尺寸重新钳制，不被居中覆盖。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Placement {
    /// 居中显示（热键触发等无坐标场景）。
    Centered,
    /// 定点显示：锚点是期望的浮层左上位置（全局桌面坐标，未钳制）。
    At(LogicalPosition<f64>),
}

/// 窗口管理器：持有各窗口的生存期，对上层只暴露「谁的窗口」「显示/隐藏」。
pub struct WindowManager {
    overlay: Arc<Window>,
    /// 浮层当前逻辑尺寸（内容自适应；`centered_position` 居中计算取它）。
    overlay_size: LogicalSize<f64>,
    /// 当前摆放意图（`set_placement` 更新，尺寸变化时按它重定位）。
    placement: Placement,
    settings: Arc<Window>,
}

impl WindowManager {
    /// 创建浮层与设置窗口。必须在 `resumed` 里调用——只有进入 resumed
    /// 才允许创建窗口与 surface。
    pub fn new(event_loop: &ActiveEventLoop) -> Result<Self, OsError> {
        let overlay = event_loop.create_window(
            Window::default_attributes()
                .with_title(APP_NAME)
                .with_inner_size(LogicalSize::new(OVERLAY_WIDTH, OVERLAY_HEIGHT))
                // 浮层是「内容即窗口」的卡片：无系统标题栏、置顶、尺寸由内容决定
                .with_decorations(false)
                .with_window_level(WindowLevel::AlwaysOnTop)
                .with_resizable(false)
                // 透明：卡片圆角之外要让桌面透出来，因此 surface 也得选带 alpha 的合成模式
                .with_transparent(true),
        )?;
        // 窗口创建即可见，预创建的浮层必须立刻压下去
        overlay.set_visible(false);

        let settings = event_loop.create_window(
            Window::default_attributes()
                // 创建即隐藏：可见前 [`Self::show_settings`] 会按当前界面语言
                // 写入标题，这里的占位只在窗口从未显示过时存在。
                .with_title(APP_NAME)
                .with_inner_size(LogicalSize::new(SETTINGS_WIDTH, SETTINGS_HEIGHT))
                // 与浮层同层：同层窗口按激活序排布，聚焦即浮于浮层之上。
                .with_window_level(WindowLevel::AlwaysOnTop)
                .with_resizable(true)
                .with_min_inner_size(LogicalSize::new(380.0, 420.0)),
        )?;
        settings.set_visible(false);

        Ok(Self {
            overlay: Arc::new(overlay),
            overlay_size: LogicalSize::new(OVERLAY_WIDTH, OVERLAY_HEIGHT),
            placement: Placement::Centered,
            settings: Arc::new(settings),
        })
    }

    /// 浮层窗口本体（事件处理用）。
    pub fn overlay(&self) -> &Window {
        &self.overlay
    }

    /// 浮层当前逻辑尺寸（内容自适应，随 [`Self::set_overlay_size`] 更新；
    /// `centered_position` 的居中计算取它）。
    pub fn logical_size(&self) -> LogicalSize<f64> {
        self.overlay_size
    }

    /// 记录浮层的摆放意图（每次显示前设置）；尺寸自适应变化时按它重定位
    /// ——居中者重新居中，定点者原锚点重新钳制。
    pub fn set_placement(&mut self, placement: Placement) {
        self.placement = placement;
    }

    /// 应用浮层内容的期望尺寸（内容自适应高度）：按显示器钳制高度后才
    /// 重设窗口，变化小于阈值时不动——渲染帧后逐帧调用也只在真实变化
    /// 时触发 resize；尺寸变化后按摆放意图重定位（显示入口只能按上一帧
    /// 尺寸算位置：居中者不重定位会向下溢出屏幕，定点者不重定位会被
    /// 旧尺寸的钳制结果挤离锚点）。
    pub fn set_overlay_size(&mut self, size: LogicalSize<f64>) {
        let capped = self.cap_height_to_screen(size);
        if (capped.width - self.overlay_size.width).abs() < RESIZE_EPSILON
            && (capped.height - self.overlay_size.height).abs() < RESIZE_EPSILON
        {
            return;
        }
        // 即时生效的平台返回实际物理尺寸（可能与请求有出入），换算回
        // 逻辑值记录；异步交付的平台（Wayland）返回 None，先记请求值，
        // 随后的 Resized 事件照常驱动 surface 重建。
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

    /// 浮层居中于显示器（逻辑坐标）：优先窗口当前所在的显示器，其次
    /// 主显示器。
    pub fn centered_position(&self, event_loop: &ActiveEventLoop) -> LogicalPosition<f64> {
        let monitor = self
            .overlay
            .current_monitor()
            .or_else(|| event_loop.primary_monitor());
        monitor.map_or(LogicalPosition::new(0.0, 0.0), |monitor| {
            centered_on_monitor(&monitor, self.overlay_size)
        })
    }

    /// 把浮层期望位置钳制在释放点命中的显示器范围内（跟随划词位置用）：
    /// 以浮层当前尺寸为界，右/下越界时向内收；找不到命中显示器时回落
    /// 浮层当前所在显示器，再拿不到就原样返回。坐标是全局桌面坐标
    /// （与 winit 窗口定位、CG 释放坐标同口径）。
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

    /// 释放点命中的显示器（按全局桌面坐标判界）；浮层当前显示器作兜底。
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

    /// 高度按浮层所在显示器钳制（超出部分由内容侧滚动兜底）；拿不到
    /// 显示器时原样返回。
    fn cap_height_to_screen(&self, size: LogicalSize<f64>) -> LogicalSize<f64> {
        let Some(monitor) = self.overlay.current_monitor() else {
            return size;
        };
        let scale = monitor.scale_factor();
        let screen_height = monitor.size().to_logical::<f64>(scale).height;
        let max_height = (screen_height - WORK_AREA_MARGIN).max(OVERLAY_HEIGHT);
        LogicalSize::new(size.width, size.height.min(max_height))
    }

    /// reposition + show：唯一显示入口（预创建复用只显隐）。
    pub fn show_at(&self, position: LogicalPosition<f64>) {
        self.overlay.set_outer_position(position);
        self.overlay.set_visible(true);
    }

    /// 隐藏但不销毁：窗口与 surface 原样保留，下次显示零重建成本。
    pub fn hide(&self) {
        self.overlay.set_visible(false);
    }

    /// 浮层当前是否可见（winit 返回 `Option<bool>`，窗口已消失时按不可见算）。
    pub fn is_visible(&self) -> bool {
        self.overlay.is_visible().unwrap_or(false)
    }

    /// 浮层句柄的引用计数：唯一窗口只显隐不重建，计数应恒定不变，
    /// 显隐自检用它验证无窗口泄漏。
    pub fn handle_refcount(&self) -> usize {
        Arc::strong_count(&self.overlay)
    }

    /// 浮层窗口的共享句柄（surface 要持有窗口到 'static）。
    pub fn overlay_handle(&self) -> Arc<Window> {
        Arc::clone(&self.overlay)
    }

    /// 设置窗口的共享句柄（surface 持有到 'static）。
    pub fn settings_handle(&self) -> Arc<Window> {
        Arc::clone(&self.settings)
    }

    /// 显示设置窗口并置前（已可见则只是聚焦）；标题由调用方按当前界面语言
    /// 给（`Text::app.settings_title`），草稿与文案表都由调用方管理。
    pub fn show_settings(&self, title: &str) {
        self.settings.set_title(title);
        self.settings.set_visible(true);
        self.settings.focus_window();
    }

    /// 隐藏设置窗口（关闭按钮/保存完成）；窗口与 surface 保留。
    pub fn hide_settings(&self) {
        self.settings.set_visible(false);
    }

    /// 设置窗口是否可见。
    pub fn is_settings_visible(&self) -> bool {
        self.settings.is_visible().unwrap_or(false)
    }

    /// 事件是否来自浮层窗口。
    pub fn matches_overlay(&self, id: WindowId) -> bool {
        self.overlay.id() == id
    }

    /// 事件是否来自设置窗口。
    pub fn matches_settings(&self, id: WindowId) -> bool {
        self.settings.id() == id
    }

    /// 请求重绘浮层。
    pub fn request_redraw(&self) {
        self.overlay.request_redraw();
    }

    /// 请求重绘设置窗口。
    pub fn request_redraw_settings(&self) {
        self.settings.request_redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::clamp_to_monitor;
    use winit::dpi::{LogicalPosition, LogicalSize};

    const SCREEN: LogicalSize<f64> = LogicalSize::new(1920.0, 1080.0);
    const OVERLAY: LogicalSize<f64> = LogicalSize::new(380.0, 200.0);

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
            LogicalPosition::new(1540.0, 300.0),
            "右越界收到 1920-380"
        );
        assert_eq!(
            clamp_to_monitor(LogicalPosition::new(500.0, 2000.0), origin, SCREEN, OVERLAY),
            LogicalPosition::new(500.0, 880.0),
            "下越界收到 1080-200"
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
            LogicalPosition::new(2980.0, 300.0),
            "副屏右缘按 1440+1920-380 收，不是主屏的 1540"
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
