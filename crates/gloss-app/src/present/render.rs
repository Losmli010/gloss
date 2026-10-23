//! 帧渲染管线：建帧（`egui` 状态 + `GPU surface`）与 `egui` → `wgpu` 的一帧呈现，
//! 浮层与设置窗口共用。

use std::error::Error;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::ViewportId;
use gloss_core::config::Theme;
use gloss_core::log::milestone;
use gloss_core::model::Locale;
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

use crate::machine::OverlayView;
use crate::present::gpu::{FONT_ATLAS_MAX_SIDE, GpuContext, GpuSurface};
use crate::present::windows::WindowManager;
use crate::ui;
use crate::ui::i18n::Text;

pub struct Frame {
    pub(crate) window: Arc<Window>,
    pub(crate) egui_ctx: egui::Context,
    pub(crate) egui: egui_winit::State,
    pub(crate) surface: GpuSurface,
    popup_state: Rc<ui::popup::RenderState>,
}

impl Frame {
    pub fn adapter_name(&self) -> String {
        self.surface.adapter_name()
    }

    pub(crate) fn reset_overlay_drag(&self) {
        self.popup_state.reset_drag_state();
    }

    pub fn reset_appear_animation(&self) {
        ui::popup::reset_appear_animation(&self.egui_ctx);
    }
}

pub fn build_window_stack(
    event_loop: &ActiveEventLoop,
    theme: Theme,
) -> Result<(WindowManager, Arc<GpuContext>, Frame), Box<dyn Error>> {
    let windows = WindowManager::new(event_loop)?;
    milestone("06a_windows");

    let context = Arc::new(GpuContext::new()?);
    milestone("06b_gpu");
    let frame = build_frame(windows.overlay_handle(), &context, theme)?;
    milestone("06c_frames");

    Ok((windows, context, frame))
}

pub(crate) fn build_frame(
    window: Arc<Window>,
    context: &Arc<GpuContext>,
    theme: Theme,
) -> Result<Frame, Box<dyn Error>> {
    let surface = GpuSurface::new(context, Arc::clone(&window))?;

    let egui_ctx = ui::context::new_context(theme);
    let egui = egui_winit::State::new(
        egui_ctx.clone(),
        ViewportId::ROOT,
        &*window,
        Some(window.scale_factor() as f32),
        window.theme(),
        Some(FONT_ATLAS_MAX_SIDE),
    );
    Ok(Frame {
        window,
        egui_ctx,
        egui,
        surface,
        popup_state: Rc::new(ui::popup::RenderState::default()),
    })
}

pub fn render_frame_with<R>(
    frame: &mut Frame,
    draw: impl FnOnce(&mut egui::Ui) -> R,
) -> (Option<Instant>, Option<R>) {
    let input = frame.egui.take_egui_input(&frame.window);
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

#[derive(Default)]
pub struct OverlayFrameOutput {
    pub action: Option<ui::popup::OverlayAction>,
    pub sizing: Option<ui::popup::OverlaySizing>,
    pub drag: Option<egui::Vec2>,
}

pub fn render_frame(
    frame: &mut Frame,
    view: Option<&OverlayView>,
    attached: Option<&Arc<[u8]>>,
    locale: Locale,
) -> (Option<Instant>, OverlayFrameOutput) {
    let popup_state = Rc::clone(&frame.popup_state);
    let text = Text::get(locale);
    let (repaint_at, output) = render_frame_with(frame, |ui| {
        ui::popup::draw(ui, view, attached, &popup_state, text)
    });
    let output = output.map_or(OverlayFrameOutput::default(), |out| OverlayFrameOutput {
        action: out.action,
        sizing: Some(out.sizing),
        drag: out.drag,
    });
    (repaint_at, output)
}

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
