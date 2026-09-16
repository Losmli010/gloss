//! winit 事件循环：主线程的窗口生命周期与渲染驱动。

use std::error::Error;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::ViewportId;
use gloss_core::config_handle::ConfigHandle;
use gloss_core::log::{debug, error, info, thread, warn};
use gloss_core::task::TaskInput;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalPosition;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

use crate::channel::{AcquireCommand, AppEndpoints, Command, Event, PlatformEvent};
use crate::gpu::{GpuContext, GpuSurface, MAX_TEXTURE_DIMENSION};
use crate::machine::{ErrorAction, OverlayView, RunRequest, TaskStateMachine};
use crate::ui;
use crate::windows::WindowManager;

/// 浮层显示后的自动隐藏时长（06 §6.1：超时回 Idle；失焦路径走 Focused 事件）
const AUTO_HIDE_AFTER: Duration = Duration::from_secs(10);

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
/// `endpoints` 是 App 侧通道端点（① 收平台事件、② 发取材命令、③ 发推理
/// 任务、④ 收回传事件），由组装点拆出移交；`config` 是运行时配置句柄
/// （M4-T3），在每一批平台事件的起手处取一份快照交给状态机（见
/// [`GlossApp::drain_platform_events`]）——配置热更新因此无需重启，也不必
/// 给 App 传配置存储。
///
/// `on_waker` 拿到唤醒句柄——`main.rs` 是唯一组装点，句柄要由它分发给
/// 平台事件线程与 tokio，库这边不替上层决定跨线程拓扑。
pub fn run(
    endpoints: AppEndpoints,
    config: Arc<ConfigHandle>,
    on_waker: impl FnOnce(Waker),
) -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let waker = Waker(event_loop.create_proxy());
    on_waker(waker);
    let mut app = GlossApp::new(endpoints, config);
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// 一帧渲染所需的全部状态；窗口建好之前为 `None`。
pub struct Frame {
    pub(crate) window: Arc<Window>,
    egui_ctx: egui::Context,
    egui: egui_winit::State,
    surface: GpuSurface,
}

/// 建窗口栈与首帧渲染所需的全部状态（生产 App 与自检 handler 共用）。
pub fn build_window_stack(
    event_loop: &ActiveEventLoop,
) -> Result<(WindowManager, Frame), Box<dyn Error>> {
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

    Ok((
        windows,
        Frame {
            window,
            egui_ctx,
            egui,
            surface,
        },
    ))
}

/// 渲染一帧：egui 出绘制数据 → wgpu 呈现，返回（egui 要求的下一帧时刻，
/// 本帧被点击的失败卡动作按钮）。
pub fn render_frame(
    frame: &mut Frame,
    view: Option<&OverlayView>,
) -> (Option<Instant>, Option<ErrorAction>) {
    let input = frame.egui.take_egui_input(&frame.window);
    // run_ui 的闭包没有返回值：浮层把点击动作写进这个局部变量带出来。
    let mut clicked = None;
    let output = frame.egui_ctx.run_ui(input, |ui| {
        clicked = ui::popup::draw(ui, view);
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
    (repaint_at, clicked)
}

struct GlossApp {
    windows: Option<WindowManager>,
    frame: Option<Frame>,
    /// egui 要求的下一帧时间点；`None` 表示等到有事件再画
    next_repaint: Option<Instant>,
    /// 浮层自动隐藏时刻；仅浮层可见时为 `Some`
    auto_hide: Option<Instant>,
    /// 组装点移交的通道端点（① 收、② 发、③ 发、④ 收）。
    endpoints: Option<AppEndpoints>,
    /// 任务状态机（functional core，见 machine.rs）：纯状态转移，壳只做
    /// 通道发送、浮层窗口操作与日志。
    machine: TaskStateMachine,
    /// 运行时配置句柄（M4-T3）：每批平台事件取一份快照交给状态机，
    /// 配置保存后无需重启即对下一次触发生效。
    config: Arc<ConfigHandle>,
}

impl GlossApp {
    /// 组装点移交的通道端点与配置句柄；窗口与帧状态在 `resumed` 时建立。
    fn new(endpoints: AppEndpoints, config: Arc<ConfigHandle>) -> Self {
        Self {
            windows: None,
            frame: None,
            next_repaint: None,
            auto_hide: None,
            endpoints: Some(endpoints),
            machine: TaskStateMachine::new(),
            config,
        }
    }

    /// 建窗口栈 → 建帧状态，一次做完。
    fn init(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Box<dyn Error>> {
        let (windows, frame) = build_window_stack(event_loop)?;
        self.frame = Some(frame);
        self.windows = Some(windows);
        self.draw();
        Ok(())
    }

    /// 画一帧：egui 出绘制数据 → wgpu 呈现，并把 egui 要求的下一帧记下
    /// 来；失败卡上的动作按钮（重试/打开设置）就地执行。
    fn draw(&mut self) {
        let Some(frame) = self.frame.as_mut() else {
            return;
        };
        let overlay_view = self.machine.overlay_view();
        let (repaint, action) = render_frame(frame, overlay_view);
        self.next_repaint = repaint;
        if let Some(action) = action {
            self.handle_error_action(action);
        }
    }

    /// 执行失败卡的动作出口（06 §7 错误映射的壳侧半边）。
    fn handle_error_action(&mut self, action: ErrorAction) {
        match action {
            ErrorAction::Retry => match self.machine.retry() {
                Some(request) => {
                    info!(
                        thread = thread::UI,
                        generation = request.generation,
                        "error card retry, task re-dispatched to tokio"
                    );
                    self.send_run(request);
                    // 重锚隐藏计时：重试的成功路径不该被失败卡出现时刻
                    // 锚定的旧计时掐断（与 accept_done 的重锚同一理由）。
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
            ErrorAction::OpenSettings => self.open_settings(),
        }
    }

    /// 打开设置窗口的统一入口（失败卡「打开设置」与托盘/热键的
    /// `OpenSettingsRequested` 走同一条路）。窗口本体随 M4-T6 落地，
    /// 在那之前只留诊断痕迹。
    fn open_settings(&mut self) {
        debug!(
            thread = thread::UI,
            "settings open requested, window lands with M4-T6"
        );
    }

    /// 统一显示入口：显示并启动自动隐藏计时。
    fn show_overlay(&mut self, position: LogicalPosition<f64>) {
        let Some(windows) = &self.windows else {
            return;
        };
        windows.show_at(position);
        self.auto_hide = Some(Instant::now() + AUTO_HIDE_AFTER);
    }

    /// 消费通道①：平台事件 → 取材命令。只有真实下发的命令才占用新代数
    /// （未接线事件不作废在途回传）；触发→命令的日志链路同时承担热键端到
    /// 端的验收验证（CI 无法合成真实按键，只能真机按日志走查）。
    fn drain_platform_events(&mut self) {
        // 配置快照在本批事件的起手处取一次（零锁读）：本批触发的任务都用
        // 同一份配置解析类型与选项——任务一旦触发，其配置就固定了。
        let config = self.config.snapshot();
        // 先收集再处理：endpoints 的借用与 &mut self 互斥，收进 Vec 后即
        // 归还，后续可用正常的方法调用。
        let events: Vec<PlatformEvent> = self
            .endpoints
            .as_ref()
            .map_or(Vec::new(), |e| e.platform_events.try_iter().collect());
        for event in events {
            let superseded = self.machine.current_cancel().is_some();
            if let Some(command) = self.machine.trigger(&event, &config) {
                info!(
                    thread = thread::UI,
                    generation = self.machine.generation(),
                    cancelled_inflight = superseded,
                    "platform event dispatched as acquire command"
                );
                self.send_acquire(command);
            } else {
                debug!(
                    thread = thread::UI,
                    event = ?event,
                    "platform event not wired yet, ignored"
                );
            }
        }
    }

    /// 通道②发送；接收端消失（事件线程死亡/退出）时落 Error 态兜底，
    /// 避免滞留 Fetching。
    fn send_acquire(&mut self, command: AcquireCommand) {
        let Some(endpoints) = &self.endpoints else {
            return;
        };
        if endpoints.acquire_commands.send(command).is_err() {
            warn!(
                thread = thread::UI,
                generation = self.machine.generation(),
                "acquire channel closed, command dropped"
            );
            self.machine.fail_acquire(self.machine.generation());
        }
    }

    /// 消费通道④：取材产物按代数采纳——连续快速触发时旧代数的产物被
    /// 丢弃，浮层只显示最后一次请求的结果。状态决策在 machine，壳只做
    /// 通道发送、浮层展示与日志。
    fn drain_events(&mut self, event_loop: &ActiveEventLoop) {
        let events: Vec<Event> = self
            .endpoints
            .as_ref()
            .map_or(Vec::new(), |e| e.events.try_iter().collect());
        let mut show_needed = false;
        for event in events {
            match event {
                Event::InputReady { generation, input } => {
                    show_needed |= self.accept_input(generation, input);
                }
                Event::TaskChunk { generation, delta } => {
                    self.accept_chunk(generation, delta);
                }
                Event::TaskDone {
                    generation,
                    outcome,
                } => {
                    if self.accept_done(generation, outcome) {
                        // 结果卡可见时长从完成时刻重新起算：慢任务不至于
                        // 刚出结果就被早先锚定的隐藏计时收起。
                        if self.windows.is_some() {
                            self.auto_hide = Some(Instant::now() + AUTO_HIDE_AFTER);
                        }
                    }
                }
                Event::TaskFailed { generation, error } => {
                    show_needed |= self.accept_failed(generation, &error);
                }
            }
        }
        if show_needed && let Some(windows) = &self.windows {
            let position = centered_position(event_loop, windows);
            self.show_overlay(position);
        }
        self.request_redraw();
    }

    /// 采纳取材产物：组装 Task 携令牌下发通道③，进入 Translating。
    /// 返回是否进入了需要展示浮层的新任务。
    fn accept_input(&mut self, generation: u64, input: TaskInput) -> bool {
        match self.machine.accept_input(generation, input) {
            Some(request) => {
                info!(
                    thread = thread::UI,
                    generation = request.generation,
                    "input ready, task dispatched to tokio"
                );
                self.send_run(request);
                true
            }
            None => {
                debug!(
                    thread = thread::UI,
                    generation = generation,
                    current = self.machine.generation(),
                    state = ?self.machine.state(),
                    "stale or unexpected input ready dropped"
                );
                false
            }
        }
    }

    /// 通道③发送；接收端不可用（tokio 桥死亡）时状态机降级落 Error。
    fn send_run(&mut self, request: RunRequest) {
        let Some(endpoints) = &self.endpoints else {
            self.machine.fail_transport(request.generation);
            return;
        };
        if endpoints
            .commands
            .send(Command::RunTask {
                generation: request.generation,
                task: request.task,
                cancel: request.cancel,
            })
            .is_err()
        {
            warn!(
                thread = thread::UI,
                generation = request.generation,
                "command channel closed, task dropped"
            );
            self.machine.fail_transport(request.generation);
        }
    }

    /// 采纳流式增量。返回是否有新内容需要重绘。
    fn accept_chunk(&mut self, generation: u64, delta: String) -> bool {
        let accepted = self.machine.accept_chunk(generation, delta);
        if !accepted {
            debug!(
                thread = thread::UI,
                generation = generation,
                current = self.machine.generation(),
                state = ?self.machine.state(),
                "superseded or hidden chunk dropped"
            );
        }
        accepted
    }

    /// 采纳任务产物：定格正文并进入 `Show`。返回是否需要重绘。
    fn accept_done(&mut self, generation: u64, outcome: gloss_core::task::TaskOutcome) -> bool {
        let accepted = self.machine.accept_done(generation, outcome);
        if accepted {
            info!(
                thread = thread::UI,
                generation = generation,
                "task done, showing outcome"
            );
        } else {
            debug!(
                thread = thread::UI,
                generation = generation,
                current = self.machine.generation(),
                state = ?self.machine.state(),
                "superseded or hidden task done dropped"
            );
        }
        accepted
    }

    /// 采纳任务失败：落 `Error` 态并展示失败卡（文案与动作出口由
    /// machine 按错误类别给出，见 `machine::error_action`）。
    /// 返回是否需要展示浮层。
    fn accept_failed(&mut self, generation: u64, error: &gloss_core::model::GlossError) -> bool {
        let accepted = self.machine.accept_failed(generation, error);
        if accepted {
            warn!(
                thread = thread::UI,
                generation = generation,
                error = %error,
                "task failed"
            );
        } else {
            debug!(
                thread = thread::UI,
                generation = generation,
                current = self.machine.generation(),
                "stale task failed dropped"
            );
        }
        accepted
    }

    /// 自动隐藏到点：收起浮层并回落 Idle。
    fn on_auto_hide(&mut self, _event_loop: &ActiveEventLoop) {
        self.auto_hide = None;
        if let Some(windows) = &self.windows {
            windows.hide();
        }
        self.machine.hide_overlay();
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
pub fn centered_position(
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
        // 预算（09 M1-T6）
        self.draw();
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _event: UserEvent) {
        self.drain_platform_events();
        self.drain_events(event_loop);
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
                // 失焦回 Idle：只隐藏不销毁
                if let Some(windows) = &self.windows {
                    windows.hide();
                    self.auto_hide = None;
                    self.machine.hide_overlay();
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
    use crate::machine::{AppState, OverlayView};
    use gloss_core::config::{Config, DEFAULT_TEXT_MODEL, ModelBinding};
    use gloss_core::model::Lang;
    use gloss_core::ports::mocks::MemoryConfigStore;
    use gloss_core::task::TaskKind;

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

    /// `driven_app` 交出的驱动端点：App 本体 + 配置句柄 + 四条通道的端点。
    type DrivenApp = (
        GlossApp,
        Arc<ConfigHandle>,
        crossbeam_channel::Sender<PlatformEvent>,
        crossbeam_channel::Receiver<AcquireCommand>,
        tokio::sync::mpsc::UnboundedReceiver<Command>,
        crossbeam_channel::Sender<Event>,
    );

    /// 构造接入真实通道与配置句柄的 App，返回各通道端点与句柄供测试驱动。
    ///
    /// 配置走 core 的内存桩（`test-util` 特性），测试可以 `handle.save(...)`
    /// 模拟设置页保存，观察下一次触发是否用上新配置。
    fn driven_app() -> DrivenApp {
        let crate::channel::Channels {
            platform_events,
            acquire_commands,
            commands,
            events,
        } = crate::channel::Channels::new();
        let crate::channel::CrossbeamPair {
            tx: pe_tx,
            rx: pe_rx,
        } = platform_events;
        let crate::channel::CrossbeamPair {
            tx: ac_tx,
            rx: ac_rx,
        } = acquire_commands;
        let crate::channel::CrossbeamPair {
            tx: ev_tx,
            rx: ev_rx,
        } = events;
        let crate::channel::CommandChannel {
            tx: cmd_tx,
            rx: cmd_rx,
        } = commands;
        let config = Arc::new(ConfigHandle::with_config(
            Arc::new(MemoryConfigStore::default()),
            Config::default(),
        ));
        let app = GlossApp::new(
            AppEndpoints {
                platform_events: pe_rx,
                acquire_commands: ac_tx,
                commands: cmd_tx,
                events: ev_rx,
            },
            Arc::clone(&config),
        );
        (app, config, pe_tx, ac_rx, cmd_rx, ev_tx)
    }

    /// 驱动一次触发（划词手势）走完通道①消费。
    fn trigger_selection(app: &mut GlossApp, pe_tx: &crossbeam_channel::Sender<PlatformEvent>) {
        pe_tx.send(PlatformEvent::SelectionGesture).unwrap();
        app.drain_platform_events();
    }

    fn text_input(text: &str) -> TaskInput {
        TaskInput::Text {
            text: text.into(),
            hint: None,
        }
    }

    fn plain_outcome(body: &str) -> gloss_core::task::TaskOutcome {
        gloss_core::task::TaskOutcome {
            kind: gloss_core::task::TaskKind::TranslateWord,
            body: body.into(),
            structured: gloss_core::task::OutcomeStructured::Plain { title: None },
        }
    }

    fn streaming_body(app: &GlossApp) -> &str {
        match app.machine.overlay_view() {
            Some(OverlayView::Streaming { body, .. }) => body,
            other => panic!("expected streaming view, got {other:?}"),
        }
    }

    fn outcome_body(app: &GlossApp) -> &str {
        match app.machine.overlay_view() {
            Some(OverlayView::Outcome(outcome)) => &outcome.body,
            other => panic!("expected outcome view, got {other:?}"),
        }
    }

    /// 验收标准：连续触发 A→B 时，A 的迟到 chunk / TaskDone 不串台——
    /// 新触发取消 A 的令牌、推进代数，A 的一切回传被陈旧过滤。
    #[test]
    fn late_events_of_superseded_trigger_do_not_bleed() {
        let (mut app, _config, pe_tx, ac_rx, mut cmd_rx, _ev_tx) = driven_app();

        // 触发 A：进入 Fetching，gen=1，取材命令下发。
        trigger_selection(&mut app, &pe_tx);
        assert_eq!(app.machine.state(), AppState::Fetching);
        assert_eq!(app.machine.generation(), 1);
        assert!(matches!(
            ac_rx.try_recv().unwrap(),
            AcquireCommand::AcquireText { generation: 1, .. }
        ));

        // A 的取材产物到达：组装 Task 携令牌下发③，进入 Translating。
        assert!(app.accept_input(1, text_input("A")));
        assert_eq!(app.machine.state(), AppState::Translating);
        let Command::RunTask {
            generation: 1,
            cancel: token_a,
            ..
        } = cmd_rx.try_recv().unwrap()
        else {
            panic!("run task expected");
        };
        assert!(!token_a.is_cancelled());

        // A 的流式增量到达并展示。
        assert!(app.accept_chunk(1, "部分A".into()));
        assert!(streaming_body(&app).contains("部分A"));

        // 触发 B：A 的令牌立即取消，代数推进，状态回 Fetching。
        trigger_selection(&mut app, &pe_tx);
        assert_eq!(app.machine.generation(), 2);
        assert_eq!(app.machine.state(), AppState::Fetching);
        assert!(token_a.is_cancelled(), "new trigger must cancel task A");

        // A 的迟到 chunk 被陈旧过滤：既不进入 B 的展示，也不改变状态。
        assert!(!app.accept_chunk(1, "迟到A".into()));
        assert!(
            !streaming_body(&app).contains("迟到A"),
            "late chunk of A must not bleed into the overlay"
        );

        // B 的产物链路照常。
        assert!(app.accept_input(2, text_input("B")));
        assert!(matches!(
            cmd_rx.try_recv().unwrap(),
            Command::RunTask { generation: 2, .. }
        ));
        assert!(!app.accept_done(1, plain_outcome("迟到结果A")));
        assert!(app.accept_done(2, plain_outcome("结果B")));
        assert_eq!(app.machine.state(), AppState::Show);
        assert_eq!(outcome_body(&app), "结果B");
    }

    /// 匹配的失败落 Error 态并可重试；Error 态再次触发即重试。
    #[test]
    fn failed_task_lands_in_error_and_retry_works() {
        let (mut app, _config, pe_tx, _ac_rx, _cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(1, text_input("A")));

        // 陈旧失败（旧代数）丢弃，不影响 Translating。
        assert!(!app.accept_failed(0, &gloss_core::model::GlossError::EngineNetwork));
        assert_eq!(app.machine.state(), AppState::Translating);

        assert!(app.accept_failed(1, &gloss_core::model::GlossError::EngineNetwork));
        assert_eq!(app.machine.state(), AppState::Error);
        assert!(app.machine.current_cancel().is_none());

        // Error 态再次触发即重试。
        trigger_selection(&mut app, &pe_tx);
        assert_eq!(app.machine.state(), AppState::Fetching);
    }

    /// 失败卡的重试按钮（06 §7）：可重试失败给出 Retry 出口，壳把它原样
    /// 重发到通道③——同代数、同任务、新令牌。
    #[test]
    fn retry_action_redispatches_the_failed_task() {
        let (mut app, _config, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();
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

        // 重试后的产物照常采纳。
        assert!(app.accept_done(1, plain_outcome("重试成功")));
        assert_eq!(app.machine.state(), AppState::Show);
    }

    /// 设置页出口（鉴权/配置类失败）：点「打开设置」不产生通道③流量，
    /// 状态停在 Error——窗口本体随 M4-T6 接入 open_settings()。
    #[test]
    fn open_settings_action_keeps_the_error_card() {
        let (mut app, _config, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(1, text_input("A")));
        // 排空首发的 RunTask，之后通道③应为空——「打开设置」不得产生
        // 重发流量。
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
    }

    /// 陈旧的 InputReady 不进入 Translating，也不下发③。
    #[test]
    fn stale_input_ready_is_dropped_entirely() {
        let (mut app, _config, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert!(!app.accept_input(42, text_input("来自未来")));
        assert_eq!(
            app.machine.state(),
            AppState::Fetching,
            "stale input must not move state"
        );
        assert!(
            cmd_rx.try_recv().is_err(),
            "stale input must not reach tokio"
        );
    }

    /// 验收标准（M4-T3）：运行时保存配置后，**下一次触发即生效**——目标
    /// 语言与模型随任务下发到通道③，无需重启。取材产物由测试直接注入，
    /// 走的仍是生产路径的 `drain_platform_events` → `accept_input`。
    #[test]
    fn saved_config_applies_to_the_next_trigger() {
        let (mut app, config, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();

        // 出厂默认：目标语言中文 + 出厂文本模型（用户只差 keychain 里那把钥匙）。
        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(1, text_input("A")));
        let Command::RunTask { task, .. } = cmd_rx.try_recv().unwrap();
        assert_eq!(task.options.target_lang, Some(Lang::Zh));
        assert_eq!(
            task.options.model_override.as_deref(),
            Some(DEFAULT_TEXT_MODEL)
        );

        // 设置页保存（ConfigHandle：写文件 + 原子替换快照）。
        config
            .save(Config {
                target_lang: Lang::Ja,
                model_by_kind: vec![ModelBinding {
                    kind: TaskKind::TranslateWord,
                    model: "deepseek-reasoner".into(),
                }],
                ..Default::default()
            })
            .expect("save should succeed");

        // 下一次触发：新配置立即生效。
        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(2, text_input("B")));
        let Command::RunTask { task, .. } = cmd_rx.try_recv().unwrap();
        assert_eq!(task.options.target_lang, Some(Lang::Ja));
        assert_eq!(
            task.options.model_override.as_deref(),
            Some("deepseek-reasoner")
        );
    }

    /// 快照在派发途中冻结：触发之后、取材产物到达之前保存了新配置，**在途
    /// 任务仍用触发时那份**（选项在 `trigger` 时解析，`accept_input` 不再
    /// 取配置），新配置只对下一次触发生效。
    ///
    /// 这是 App 层的护栏：`machine` 的单测证明不了它（`accept_input` 签名里
    /// 没有 config），而 M4-T6/T7 改 App 时最可能踩的就是「派发时重新取快照」。
    #[test]
    fn saved_config_does_not_leak_into_the_inflight_task() {
        let (mut app, config, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();

        // 触发（拿到 v1 快照）→ 保存 v2 → 才喂取材产物。
        trigger_selection(&mut app, &pe_tx);
        config
            .save(Config {
                target_lang: Lang::Ja,
                ..Default::default()
            })
            .expect("save should succeed");
        assert!(app.accept_input(1, text_input("A")));

        let Command::RunTask { task, .. } = cmd_rx.try_recv().unwrap();
        assert_eq!(
            task.options.target_lang,
            Some(Lang::Zh),
            "in-flight task must keep the snapshot taken at trigger"
        );

        // 新配置对下一次触发生效。
        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(2, text_input("B")));
        let Command::RunTask { task, .. } = cmd_rx.try_recv().unwrap();
        assert_eq!(task.options.target_lang, Some(Lang::Ja));
    }
}
