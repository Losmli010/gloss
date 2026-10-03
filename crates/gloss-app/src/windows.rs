//! 窗口管理器：浮层（预创建复用，只显隐不反复销毁）+ 设置窗口
//!（普通带标题栏窗口，关闭即隐藏）。

use std::sync::Arc;

use gloss_core::log::{thread, warn};
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
/// 高度钳制的屏幕占比：浮层最大高度按显示器**逻辑高度的一半**为界（先
/// 取一半再扣 [`WORK_AREA_MARGIN`]），超出部分由内容侧滚动兜底。
const HEIGHT_CAP_FRACTION: f64 = 0.5;
/// 浮层尺寸变化小于该阈值不重设窗口（防 egui 布局与 winit resize 的
/// 帧迟滞来回抖动）。
const RESIZE_EPSILON: f64 = 0.5;
/// 流式期间的增高步长（逻辑点）：逐 chunk 的内容增长被量化成台阶，
/// 避免每个增量都触发一次窗口 resize。
const STREAM_HEIGHT_STEP: f64 = 48.0;
/// 拖动落点小于该阈值（逻辑点）不重设窗口：指针未动或贴边钳制的帧
/// 不做无谓的平台调用。
const DRAG_POSITION_EPSILON: f64 = 0.5;
/// 设置窗口尺寸：全部配置区块一屏放下的紧凑初值（可拖拽调整）。
const SETTINGS_WIDTH: f64 = 460.0;
const SETTINGS_HEIGHT: f64 = 640.0;

/// 流式防抖尺寸：宽度保持当前档（popup 的宽度滞回到完成态
/// 再一次应用），高度向上量化到 [`STREAM_HEIGHT_STEP`] 的倍数且相对
/// 当前值**只增不减**——内容回缩留给完成态的精确重排。
fn debounced_size(current: LogicalSize<f64>, requested: LogicalSize<f64>) -> LogicalSize<f64> {
    let steps = (requested.height / STREAM_HEIGHT_STEP).ceil();
    let stepped = (steps * STREAM_HEIGHT_STEP).max(current.height);
    LogicalSize::new(current.width, stepped)
}

/// 由显示器逻辑高度算浮层高度上限（纯逻辑，单测覆盖）：先取
/// [`HEIGHT_CAP_FRACTION`] 的一半，再扣 [`WORK_AREA_MARGIN`]，且不低于
/// [`OVERLAY_HEIGHT`]（矮屏兜底）；请求不超过上限时原样返回。
fn capped_height(screen_height: f64, requested_height: f64) -> f64 {
    let max_height = (screen_height * HEIGHT_CAP_FRACTION - WORK_AREA_MARGIN).max(OVERLAY_HEIGHT);
    requested_height.min(max_height)
}

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
/// 重新居中（显示入口只能按上一帧尺寸算位置）；定点者（跟随划词，或
/// 用户拖动后的落点）在原锚点上按新尺寸重新钳制，不被居中覆盖。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Placement {
    /// 居中显示（热键触发等无坐标场景）。
    Centered,
    /// 定点显示：锚点是期望的浮层左上位置（全局桌面坐标；写入方钳制
    /// 与否不定——跟随划词写原始释放点，拖动写钳制后的落点），读取侧
    /// 一律再钳制。
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
    ///
    /// `debounced` 为真时走流式防抖（[`debounced_size`]）：宽度
    /// 锁定当前档、高度按步长只增不减；为假（骨架/失败/完成态）按精确
    /// 尺寸重排——TaskDone 的定型也走这一支。
    pub fn set_overlay_size(&mut self, size: LogicalSize<f64>, debounced: bool) {
        // 顺序：先防抖量化、再按屏幕钳制——反过来的话，量化取整会把已
        // 钳到屏高的高度又推回去（800 → 816），每帧在边界上抖动。
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

    /// 页头拖动的逐帧落点：以浮层**当前实际**位置为基准，加上指针自按
    /// 压点起的累计位移（窗口内相对逻辑点，egui 口径），按命中的显示器
    /// 钳制后应用，并把摆放意图记为定点——锚点随拖动落点走，流式增高
    /// 的重定位因此不会把窗口拽回拖动前的旧锚点。
    ///
    /// 落点每帧从实际位置重算（无增量记账）：窗口中途被谁动过（流式
    /// 重定位、指针随窗口移动的反馈）都被下一帧的落点自然吸收；指针未
    /// 动时落点即当前位置，等于动量以下的位置变化不再下发（流式重绘
    /// 帧不做无谓的平台调用）。当前位置读不到（窗口已亡等）时带痕迹
    /// 降级：跳过本帧平移。
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

    /// 高度按浮层所在显示器钳制，上限为显示器逻辑高度的一半减去工作区
    /// 余量（见 [`capped_height`]；超出部分由内容侧滚动兜底）；拿不到
    /// 显示器时原样返回。
    fn cap_height_to_screen(&self, size: LogicalSize<f64>) -> LogicalSize<f64> {
        let Some(monitor) = self.overlay.current_monitor() else {
            return size;
        };
        let scale = monitor.scale_factor();
        let screen_height = monitor.size().to_logical::<f64>(scale).height;
        LogicalSize::new(size.width, capped_height(screen_height, size.height))
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

#[cfg(test)]
mod cap_height_tests {
    use super::{OVERLAY_HEIGHT, WORK_AREA_MARGIN, capped_height};

    #[test]
    fn height_caps_at_half_the_screen_minus_the_margin() {
        assert_eq!(
            capped_height(1080.0, 2000.0),
            1080.0 * 0.5 - WORK_AREA_MARGIN,
            "444 on a 1080-logical-point display"
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
    use super::{STREAM_HEIGHT_STEP, debounced_size};
    use winit::dpi::LogicalSize;

    const CURRENT: LogicalSize<f64> = LogicalSize::new(380.0, 200.0);

    #[test]
    fn width_is_locked_to_the_current_tier() {
        let grown = debounced_size(CURRENT, LogicalSize::new(480.0, 400.0));
        assert_eq!(grown.width, 380.0, "streaming must not switch width tiers");
        assert_eq!(
            grown.height, 432.0,
            "height quantizes up to a step multiple"
        );
    }

    #[test]
    fn height_steps_up_in_quantized_increments() {
        let grown = debounced_size(CURRENT, LogicalSize::new(380.0, 205.0));
        assert_eq!(
            grown.height, 240.0,
            "one step past the current height is a full step"
        );
        assert_eq!(STREAM_HEIGHT_STEP, 48.0);
    }

    #[test]
    fn height_never_shrinks_during_streaming() {
        let shrunk = debounced_size(CURRENT, LogicalSize::new(380.0, 80.0));
        assert_eq!(
            shrunk.height, 200.0,
            "content shrinkage waits for the final relayout"
        );
    }

    #[test]
    fn current_size_quantizes_up_to_the_step_boundary() {
        let same = debounced_size(CURRENT, CURRENT);
        assert_eq!(
            same.height, 240.0,
            "the first streaming frame lands on the step boundary above the skeleton"
        );
        assert_eq!(same.width, CURRENT.width);
    }
}
