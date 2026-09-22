//! 帧渲染管线：建帧（egui 状态 + GPU surface）与 egui → wgpu 的一帧呈现，
//! 浮层与设置窗口共用。

use std::error::Error;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::ViewportId;
use gloss_core::model::Locale;
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

use crate::gpu::{GpuContext, GpuSurface, MAX_TEXTURE_DIMENSION};
use crate::i18n::Text;
use crate::machine::OverlayView;
use crate::ui;
use crate::windows::WindowManager;

/// 一帧渲染所需的全部状态；窗口建好之前为 `None`。
pub struct Frame {
    pub(crate) window: Arc<Window>,
    pub(super) egui_ctx: egui::Context,
    pub(super) egui: egui_winit::State,
    pub(super) surface: GpuSurface,
    /// 浮层的跨帧渲染状态（markdown 缓存与尺寸收敛状态）：Rc 让它经绘制
    /// 闭包进入 [`ui::popup::draw`]。仅浮层路径使用。
    popup_state: Rc<ui::popup::RenderState>,
}

impl Frame {
    /// GPU 适配器名，供自检的性能数据记录运行环境。
    pub fn adapter_name(&self) -> String {
        self.surface.adapter_name()
    }
}

/// 建窗口栈与两个窗口的首帧渲染状态（生产 App 与自检 handler 共用）：
/// 浮层帧在前、设置窗口帧在后。两个窗口共享同一份 `GpuContext`（设备与
/// 队列各一份），egui 上下文各自独立（互不共享 UI 状态）。
pub fn build_window_stack(
    event_loop: &ActiveEventLoop,
) -> Result<(WindowManager, Frame, Frame), Box<dyn Error>> {
    let windows = WindowManager::new(event_loop)?;

    let context = Arc::new(GpuContext::new()?);
    let frame = build_frame(windows.overlay_handle(), &context)?;
    let settings_frame = build_frame(windows.settings_handle(), &context)?;

    Ok((windows, frame, settings_frame))
}

/// 建单个窗口的 egui 渲染状态 + surface（浮层与设置窗口同构）。
fn build_frame(window: Arc<Window>, context: &Arc<GpuContext>) -> Result<Frame, Box<dyn Error>> {
    let surface = GpuSurface::new(context, Arc::clone(&window))?;

    let egui_ctx = egui::Context::default();
    // 内置字体不含 CJK 字形，画第一帧前把系统中文字体接进后备链
    ui::fonts::install(&egui_ctx);
    let egui = egui_winit::State::new(
        egui_ctx.clone(),
        ViewportId::ROOT,
        &*window,
        Some(window.scale_factor() as f32),
        window.theme(),
        Some(MAX_TEXTURE_DIMENSION as usize),
    );
    Ok(Frame {
        window,
        egui_ctx,
        egui,
        surface,
        popup_state: Rc::new(ui::popup::RenderState::default()),
    })
}

/// 渲染一帧：egui 出绘制数据 → wgpu 呈现，返回（egui 要求的下一帧时刻，
/// 绘制闭包的返回值）。浮层与设置窗口共用这套管线，只有绘制闭包不同。
pub fn render_frame_with<R>(
    frame: &mut Frame,
    draw: impl FnOnce(&mut egui::Ui) -> R,
) -> (Option<Instant>, Option<R>) {
    let input = frame.egui.take_egui_input(&frame.window);
    // run_ui 的闭包是 FnMut 而绘制闭包是 FnOnce：装进 Option 交出所有权，
    // 绘制结果经这个槽带出来（闭包恰好执行一次）。
    let mut draw = Some(draw);
    let mut result = None;
    let output = frame.egui_ctx.run_ui(input, |ui| {
        if let Some(draw) = draw.take() {
            result = Some(draw(ui));
        }
    });
    frame
        .egui
        .handle_platform_output(&frame.window, output.platform_output);

    let paint_jobs = frame
        .egui_ctx
        .tessellate(output.shapes, output.pixels_per_point);
    let repaint_at = output
        .viewport_output
        .get(&ViewportId::ROOT)
        .and_then(|viewport| repaint_at(viewport.repaint_delay, Instant::now()));
    frame
        .surface
        .render(output.textures_delta, &paint_jobs, output.pixels_per_point);
    (repaint_at, result)
}

/// 渲染一帧浮层（[`render_frame_with`] 的浮层特化，供 App 与自检 handler
/// 用）：`locale` 是界面语言（文案表选表依据，本层不做探测）；返回（egui
/// 要求的下一帧时刻，本帧被点击的浮层动作，浮层内容期望的窗口尺寸——由
/// 壳按显示器钳制后应用）。
pub fn render_frame(
    frame: &mut Frame,
    view: Option<&OverlayView>,
    locale: Locale,
) -> (
    Option<Instant>,
    Option<ui::popup::OverlayAction>,
    Option<ui::popup::OverlaySizing>,
) {
    let popup_state = Rc::clone(&frame.popup_state);
    let text = Text::get(locale);
    let (repaint_at, output) =
        render_frame_with(frame, |ui| ui::popup::draw(ui, view, &popup_state, text));
    // 内层 Option 是「闭包有没有跑」的外壳，动作本身才是浮层的返回值。
    (
        repaint_at,
        output.as_ref().and_then(|out| out.action),
        output.map(|out| out.sizing),
    )
}

/// egui 用 `Duration::MAX` 表示「不必重绘，等输入」；其余延迟换算成唤醒时刻。
fn repaint_at(delay: Duration, now: Instant) -> Option<Instant> {
    (delay != Duration::MAX).then(|| now + delay)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::repaint_at;

    #[test]
    fn repaint_delay_max_means_no_wakeup() {
        let now = Instant::now();
        assert_eq!(repaint_at(Duration::MAX, now), None);
    }

    #[test]
    fn repaint_delay_becomes_a_deadline() {
        let now = Instant::now();
        let delay = Duration::from_millis(250);
        assert_eq!(repaint_at(delay, now), Some(now + delay));
        assert_eq!(repaint_at(Duration::ZERO, now), Some(now));
    }
}
