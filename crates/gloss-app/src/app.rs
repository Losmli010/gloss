//! winit 事件循环：主线程的窗口生命周期与渲染驱动。

use std::error::Error;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::ViewportId;
use gloss_core::log::{error, thread};
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

use crate::gpu::{GpuContext, GpuSurface, MAX_TEXTURE_DIMENSION};
use crate::ui;
use crate::windows::WindowManager;

/// 投递给主线程的自定义事件。
#[derive(Clone, Copy, Debug)]
pub enum UserEvent {
    /// 有跨线程消息待处理
    Wake,
}

/// 唤醒主线程的句柄：事件线程与 tokio 各持一份 clone。
///
/// 用 `EventLoopProxy` 而不是让主线程 `try_recv` 轮询（08 §7.3）：轮询只能在
/// winit 因别的事件醒来时顺带取消息，空闲时最坏延迟一帧，且要求主线程周期性
/// 空转；代理唤醒由发送方立即触发，无消息时主线程可以一直睡。
#[derive(Clone, Debug)]
pub struct Waker(EventLoopProxy<UserEvent>);

impl Waker {
    /// 唤醒主线程；返回 `false` 表示事件循环已退出。
    pub fn wake(&self) -> bool {
        self.0.send_event(UserEvent::Wake).is_ok()
    }
}

/// 启动事件循环，直到退出才返回。
///
/// `on_waker` 拿到唤醒句柄——`main.rs` 是唯一组装点，句柄要由它分发给
/// 平台事件线程与 tokio，库这边不替上层决定跨线程拓扑。
pub fn run(on_waker: impl FnOnce(Waker)) -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let waker = Waker(event_loop.create_proxy());
    on_waker(waker);
    let mut app = GlossApp::default();
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// 一帧渲染所需的全部状态；窗口建好之前为 `None`。
struct Frame {
    window: Arc<Window>,
    egui_ctx: egui::Context,
    egui: egui_winit::State,
    surface: GpuSurface,
}

#[derive(Default)]
struct GlossApp {
    windows: Option<WindowManager>,
    frame: Option<Frame>,
    /// egui 要求的下一帧时间点；`None` 表示等到有事件再画
    next_repaint: Option<Instant>,
}

impl GlossApp {
    /// 建窗口 → 建 GPU 上下文 → 建 surface → 建 egui 桥接，一次做完。
    fn init(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Box<dyn Error>> {
        let windows = WindowManager::new(event_loop)?;
        let window = windows.overlay_handle();

        let context = Arc::new(GpuContext::new()?);
        let surface = GpuSurface::new(&context, Arc::clone(&window))?;

        let egui_ctx = egui::Context::default();
        let egui = egui_winit::State::new(
            egui_ctx.clone(),
            ViewportId::ROOT,
            &*window,
            Some(window.scale_factor() as f32),
            window.theme(),
            Some(MAX_TEXTURE_DIMENSION as usize),
        );

        self.frame = Some(Frame {
            window,
            egui_ctx,
            egui,
            surface,
        });
        self.windows = Some(windows);
        Ok(())
    }

    /// 画一帧：egui 出绘制数据 → wgpu 呈现，并把 egui 要求的下一帧记下来。
    fn draw(&mut self) {
        let Some(frame) = self.frame.as_mut() else {
            return;
        };

        let input = frame.egui.take_egui_input(&frame.window);
        let output = frame.egui_ctx.run_ui(input, ui::popup::draw);
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

        self.next_repaint = repaint_at;
    }

    fn request_redraw(&self) {
        if let Some(windows) = &self.windows {
            windows.request_redraw();
        }
    }
}

/// egui 用 `Duration::MAX` 表示「不必重绘，等输入」；其余延迟换算成唤醒时刻。
fn repaint_at(delay: Duration, now: Instant) -> Option<Instant> {
    (delay != Duration::MAX).then(|| now + delay)
}

impl ApplicationHandler<UserEvent> for GlossApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // resumed 可能连续投递，渲染栈只起一次
        if self.windows.is_some() {
            return;
        }
        if let Err(err) = self.init(event_loop) {
            // 没有窗口与渲染栈就没有可做的事，带病进主循环只会静默空转
            error!(thread = thread::UI, error = %err, "failed to start window and render stack");
            event_loop.exit();
            return;
        }
        self.request_redraw();
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: UserEvent) {
        self.request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if !self.windows.as_ref().is_some_and(|w| w.matches(window_id)) {
            return;
        }

        if matches!(event, WindowEvent::RedrawRequested) {
            self.draw();
            return;
        }

        // 其余事件先喂给 egui，它决定是否消化掉以及要不要重绘
        let repaint = {
            let Some(frame) = self.frame.as_mut() else {
                return;
            };
            frame.egui.on_window_event(&frame.window, &event).repaint
        };
        if repaint {
            self.request_redraw();
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(frame) = self.frame.as_mut() {
                    frame.surface.resize(size);
                }
            }
            _ => {}
        }
    }

    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        // WaitUntil 到点：egui 要的下一帧
        if matches!(cause, StartCause::ResumeTimeReached { .. }) {
            self.request_redraw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // 没有待画的帧就彻底睡下，等窗口事件或唤醒句柄把自己叫醒
        event_loop.set_control_flow(
            self.next_repaint
                .map_or(ControlFlow::Wait, ControlFlow::WaitUntil),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
