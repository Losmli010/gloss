//! winit 事件循环：主线程的窗口生命周期与渲染驱动。

use std::error::Error;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::ViewportId;
use gloss_core::log::{error, info, thread, warn};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalPosition;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

use crate::gpu::{GpuContext, GpuSurface, MAX_TEXTURE_DIMENSION};
use crate::ui;
use crate::windows::WindowManager;

/// 浮层显示后的自动隐藏时长（06 §6.1：超时回 Idle；失焦路径走 Focused 事件）
const AUTO_HIDE_AFTER: Duration = Duration::from_secs(10);
/// 显隐自检：每轮浮层停留时长与轮数（09 M1-T6 验收：反复显隐 100 次）
const SELFTEST_VISIBLE: Duration = Duration::from_millis(80);
const SELFTEST_ROUNDS: usize = 100;
/// 首次显示预算（09 M1-T6 验收：< 100ms，预创建生效）
const SHOW_BUDGET: Duration = Duration::from_millis(100);

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
/// `self_test` 为真时启动后跑浮层显隐自检（`--overlay-selftest`）：
/// 反复显隐 100 次后统计延迟并退出，用于验收预创建复用。
///
/// `on_waker` 拿到唤醒句柄——`main.rs` 是唯一组装点，句柄要由它分发给
/// 平台事件线程与 tokio，库这边不替上层决定跨线程拓扑。
pub fn run(self_test: bool, on_waker: impl FnOnce(Waker)) -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let waker = Waker(event_loop.create_proxy());
    on_waker(waker);
    let mut app = GlossApp {
        self_test: self_test.then(SelfTest::new),
        ..GlossApp::default()
    };
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
    /// 浮层自动隐藏时刻；仅浮层可见时为 `Some`
    auto_hide: Option<Instant>,
    /// 显隐自检；`None` 表示正常模式
    self_test: Option<SelfTest>,
}

/// 显隐自检的纯逻辑部分：轮次推进与首帧延迟统计，不碰窗口，可单测。
///
/// 每轮 = show_at → 首帧上屏（记延迟）→ 到点隐藏；跑满即输出统计退出。
struct SelfTest {
    /// 当前轮次（1-based）
    round: usize,
    /// 本轮 show_at 时刻；首帧记录后清空，保证每轮只计一次
    shown_at: Option<Instant>,
    latencies: Vec<Duration>,
}

impl SelfTest {
    fn new() -> Self {
        Self {
            round: 0,
            shown_at: None,
            latencies: Vec::new(),
        }
    }

    /// 进入新一轮，返回轮次。
    fn start_round(&mut self, now: Instant) -> usize {
        self.round += 1;
        self.shown_at = Some(now);
        self.round
    }

    /// 首帧上屏：记录本轮 show→paint 延迟；非首帧重绘返回 `None`。
    fn first_paint(&mut self, now: Instant) -> Option<Duration> {
        let latency = self.shown_at.take().map(|shown_at| now - shown_at);
        if let Some(latency) = latency {
            self.latencies.push(latency);
        }
        latency
    }

    /// 隐藏当前轮：返回是否已跑满计划轮数。
    fn is_complete(&self, max_rounds: usize) -> bool {
        self.round >= max_rounds
    }

    /// （首次显示延迟，全程最慢延迟）。
    fn summary(&self) -> Option<(Duration, Duration)> {
        let first = *self.latencies.first()?;
        let max = self.latencies.iter().max().copied()?;
        Some((first, max))
    }
}

impl GlossApp {
    /// 建窗口 → 建 GPU 上下文 → 建 surface → 建 egui 桥接，一次做完。
    fn init(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Box<dyn Error>> {
        let windows = WindowManager::new(event_loop)?;
        let window = windows.overlay_handle();

        let context = Arc::new(GpuContext::new()?);
        let surface = GpuSurface::new(&context, Arc::clone(&window))?;

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

        self.frame = Some(Frame {
            window,
            egui_ctx,
            egui,
            surface,
        });
        self.windows = Some(windows);
        self.draw();
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

        // 自检每轮只把 show→首帧 计一次（first_paint 内部已去重）
        if let Some(latency) = self
            .self_test
            .as_mut()
            .and_then(|st| st.first_paint(Instant::now()))
            && latency > SHOW_BUDGET
        {
            warn!(
                thread = thread::UI,
                latency_ms = latency.as_millis() as u64,
                "overlay show exceeded budget"
            );
        }
    }

    /// 统一显示入口：显示并启动自动隐藏计时（M2 事件源接上后触发端调这里）。
    fn show_overlay(&mut self, position: LogicalPosition<f64>) {
        let Some(windows) = &self.windows else {
            return;
        };
        windows.show_at(position);
        self.auto_hide = Some(Instant::now() + AUTO_HIDE_AFTER);
    }

    /// 自检的一轮：居中显示，停留 SELFTEST_VISIBLE 后由自动隐藏路径收回。
    fn begin_selftest_round(&mut self, event_loop: &ActiveEventLoop) {
        let Some(windows) = &self.windows else {
            return;
        };
        let position = centered_position(event_loop, windows);
        self.show_overlay(position);
        let now = Instant::now();
        // 自检要的是快闪，覆盖掉普通模式的 10s 计时
        self.auto_hide = Some(now + SELFTEST_VISIBLE);
        if let Some(st) = &mut self.self_test {
            st.start_round(now);
        }
        self.request_redraw();
    }

    /// 自动隐藏到点：收起浮层；自检模式则推进轮次或收尾退出。
    fn on_auto_hide(&mut self, event_loop: &ActiveEventLoop) {
        self.auto_hide = None;
        if let Some(windows) = &self.windows {
            windows.hide();
        }

        let Some(st) = &mut self.self_test else {
            return;
        };
        if !st.is_complete(SELFTEST_ROUNDS) {
            self.begin_selftest_round(event_loop);
            return;
        }

        let handles = self
            .windows
            .as_ref()
            .map_or(0, WindowManager::handle_refcount);
        match st.summary() {
            Some((first, max)) => {
                info!(
                    thread = thread::UI,
                    rounds = SELFTEST_ROUNDS,
                    first_show_ms = first.as_millis() as u64,
                    max_show_ms = max.as_millis() as u64,
                    window_handles = handles,
                    "overlay self-test passed"
                );
                if first > SHOW_BUDGET {
                    warn!(
                        thread = thread::UI,
                        first_show_ms = first.as_millis() as u64,
                        "first show exceeded budget"
                    );
                }
            }
            None => error!(thread = thread::UI, "overlay self-test recorded no frames"),
        }
        event_loop.exit();
    }

    fn request_redraw(&self) {
        if let Some(windows) = &self.windows {
            windows.request_redraw();
        }
    }
}

/// 浮层居中于显示器（逻辑坐标）：优先窗口当前所在的显示器，其次主显示器。
///
/// winit 0.30 没有全局光标位置读取接口，「跟随鼠标所在屏幕」需等 M2 的
/// 平台端口提供光标坐标后由调用方指定目标显示器。
fn centered_position(
    event_loop: &ActiveEventLoop,
    windows: &WindowManager,
) -> LogicalPosition<f64> {
    let monitor = windows
        .overlay_handle()
        .current_monitor()
        .or_else(|| event_loop.primary_monitor());
    let Some(monitor) = monitor else {
        return LogicalPosition::new(0.0, 0.0);
    };
    let scale = monitor.scale_factor();
    let monitor_size = monitor.size().to_logical::<f64>(scale);
    let overlay_size = windows.logical_size();
    LogicalPosition::new(
        (monitor_size.width - overlay_size.width) / 2.0,
        (monitor_size.height - overlay_size.height) / 2.0,
    )
}

/// 两个唤醒时刻里更早的那个；都没有则不用定时唤醒。
fn sooner(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
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
        // 浮层预创建即隐藏（Idle 态）；先在隐藏状态画一帧预热——egui 图集构建、
        // Metal 管线编译与纹理上传都发生在首帧，不预热的话首次显示会超 100ms
        // 预算（09 M1-T6）；自检模式随后从第一轮开始
        if self.self_test.is_some() {
            self.begin_selftest_round(event_loop);
        }
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
            WindowEvent::Focused(false) => {
                // 失焦回 Idle：只隐藏不销毁；自检轮次不受真实焦点影响
                if self.self_test.is_none()
                    && let Some(windows) = &self.windows
                {
                    windows.hide();
                    self.auto_hide = None;
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(frame) = self.frame.as_mut() {
                    frame.surface.resize(size);
                }
            }
            _ => {}
        }
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if !matches!(cause, StartCause::ResumeTimeReached { .. }) {
            return;
        }
        // 到点的可能是自动隐藏，也可能是 egui 要的下一帧，也可能两者都是
        let now = Instant::now();
        if self.auto_hide.is_some_and(|deadline| deadline <= now) {
            self.on_auto_hide(event_loop);
        }
        if self.next_repaint.is_some_and(|deadline| deadline <= now) {
            self.request_redraw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // 没有待处理的唤醒时刻就彻底睡下，等窗口事件或唤醒句柄把自己叫醒
        event_loop.set_control_flow(
            sooner(self.next_repaint, self.auto_hide)
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

    #[test]
    fn sooner_picks_the_earliest_deadline() {
        let now = Instant::now();
        let a = now + Duration::from_secs(1);
        let b = now + Duration::from_secs(2);
        assert_eq!(sooner(Some(a), Some(b)), Some(a));
        assert_eq!(sooner(Some(b), Some(a)), Some(a));
        assert_eq!(sooner(Some(a), None), Some(a));
        assert_eq!(sooner(None, Some(a)), Some(a));
        assert_eq!(sooner(None, None), None);
    }

    /// 验收（09 M1-T6）：显隐 100 次后收尾，每轮恰好记一次首帧延迟。
    #[test]
    fn selftest_completes_after_hundred_rounds() {
        let mut st = SelfTest::new();
        let mut t = Instant::now();

        for round in 1..=SELFTEST_ROUNDS {
            assert_eq!(st.start_round(t), round);
            // 首帧延迟 3ms；随后的重绘（shown_at 已清空）不再计入
            assert_eq!(
                st.first_paint(t + Duration::from_millis(3)),
                Some(Duration::from_millis(3))
            );
            assert_eq!(st.first_paint(t + Duration::from_millis(4)), None);
            assert!(!st.is_complete(SELFTEST_ROUNDS) || round == SELFTEST_ROUNDS);
            t += Duration::from_millis(83);
        }

        assert!(st.is_complete(SELFTEST_ROUNDS));
        let (first, max) = st.summary().expect("latencies recorded");
        assert_eq!(first, Duration::from_millis(3));
        assert_eq!(max, Duration::from_millis(3));
        assert_eq!(st.latencies.len(), SELFTEST_ROUNDS);
    }

    #[test]
    fn selftest_before_completion_keeps_going() {
        let mut st = SelfTest::new();
        for _ in 0..SELFTEST_ROUNDS - 1 {
            st.start_round(Instant::now());
            st.is_complete(SELFTEST_ROUNDS);
        }
        assert!(!st.is_complete(SELFTEST_ROUNDS));
    }
}
