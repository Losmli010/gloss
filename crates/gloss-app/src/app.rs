//! winit 事件循环：主线程的窗口生命周期与渲染驱动。

use std::error::Error;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::ViewportId;
use gloss_core::log::{debug, error, info, thread, warn};
use gloss_core::model::GlossError;
use gloss_core::task::{InputSource, Task, TaskInput, TaskKind, TaskOptions, TaskOutcome};
use tokio_util::sync::CancellationToken;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalPosition;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

use crate::channel::{AcquireCommand, AppEndpoints, Command, Event, PlatformEvent};
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
/// `endpoints` 是 App 侧通道端点（① 收平台事件、② 发取材命令、④ 收回传
/// 事件），由组装点拆出移交。
///
/// `on_waker` 拿到唤醒句柄——`main.rs` 是唯一组装点，句柄要由它分发给
/// 平台事件线程与 tokio，库这边不替上层决定跨线程拓扑。
pub fn run(
    self_test: bool,
    endpoints: AppEndpoints,
    on_waker: impl FnOnce(Waker),
) -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let waker = Waker(event_loop.create_proxy());
    on_waker(waker);
    let mut app = GlossApp {
        self_test: self_test.then(SelfTest::new),
        endpoints: Some(endpoints),
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

/// 应用状态机（06 §6.1）：触发 → 取材 → 推理 → 展示/失败。
///
/// 转移概要：任何可见态收到新触发（`begin_trigger`）都取消在途任务并回
/// `Fetching`；`Fetching` 采纳 `InputReady` 后携取消令牌下发通道③进
/// `Translating`；`Translating` 收 `TaskChunk` 追加展示、收 `TaskDone`
/// 定格 `Show`、收 `TaskFailed` 落 `Error`；失焦/超时隐藏回 `Idle`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum AppState {
    /// 浮层隐藏，无在途任务。
    #[default]
    Idle,
    /// 取材中：通道②命令已下发，等待 `InputReady`。
    Fetching,
    /// 推理中：`RunTask` 已下发 tokio，chunk 流式到达。
    Translating,
    /// 展示产物。
    Show,
    /// 失败态：显示失败信息，等待下一次触发重试。
    Error,
    /// 框选交互（占位，随 M5 框选遮罩落地；过渡期内无转移路径）。
    #[allow(dead_code)]
    RegionSelecting,
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
    /// 组装点移交的通道端点（① 收、② 发、③ 发、④ 收）。
    endpoints: Option<AppEndpoints>,
    /// 请求代数：**唯一赋值点**是 `begin_trigger`——只有真实下发的触发
    /// 才递增；回传事件按 `generation` 匹配，不匹配即陈旧丢弃。
    generation: u64,
    /// 应用状态机当前态。
    state: AppState,
    /// 触发时确定的任务类型，待 `InputReady` 到达后组装 `Task`。
    pending_kind: Option<TaskKind>,
    /// 在途推理的取消令牌：新触发时取消旧任务（唯一取消机制，08 §4.2）。
    current_cancel: Option<CancellationToken>,
    /// 当前浮层展示的文本（原文 / 流式累积 / 产物 / 失败信息）；`None`
    /// 时浮层显示渲染自检卡。
    overlay_text: Option<String>,
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
        let overlay_text = self.overlay_text.as_deref();
        let output = frame
            .egui_ctx
            .run_ui(input, |ui| ui::popup::draw(ui, overlay_text));
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
        // 先收集再处理：endpoints 的借用与 &mut self 互斥，收进 Vec 后即
        // 归还，后续可用正常的方法调用。
        let events: Vec<PlatformEvent> = self
            .endpoints
            .as_ref()
            .map_or(Vec::new(), |e| e.platform_events.try_iter().collect());
        for event in events {
            if let Some(command) = self.begin_trigger(&event) {
                self.send_acquire(command);
            }
        }
    }

    /// 触发的状态机入口：取消在途任务 → 推进代数（唯一赋值点）→ 组装取
    /// 材命令。未接线的平台事件返回 None 且不产生任何状态副作用。
    fn begin_trigger(&mut self, event: &PlatformEvent) -> Option<AcquireCommand> {
        let command = acquire_command_for(event, self.generation + 1)?;
        // 最新触发取代在途任务：旧推理立即取消（其迟到产物经代数过滤
        // 丢弃），令牌清空等待新任务。
        if let Some(cancel) = self.current_cancel.take() {
            cancel.cancel();
            info!(
                thread = thread::UI,
                superseded = self.generation,
                "in-flight task cancelled by newer trigger"
            );
        }
        self.generation += 1;
        info!(
            thread = thread::UI,
            generation = self.generation,
            from = ?self.state,
            "platform event dispatched as acquire command"
        );
        if let AcquireCommand::AcquireText { kind, .. } = &command {
            self.pending_kind = Some(*kind);
        }
        self.state = AppState::Fetching;
        Some(command)
    }

    /// 通道②发送；接收端已消失（事件线程死亡/退出）时只留痕。
    fn send_acquire(&mut self, command: AcquireCommand) {
        let Some(endpoints) = &self.endpoints else {
            return;
        };
        if endpoints.acquire_commands.send(command).is_err() {
            warn!(
                thread = thread::UI,
                "acquire channel closed, command dropped"
            );
        }
    }

    /// 消费通道④：取材产物按代数采纳——连续快速触发时旧代数的产物被
    /// 丢弃，浮层只显示最后一次请求的结果。
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
                    self.accept_done(generation, outcome);
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

    /// 采纳取材产物：组装 `Task` 携新令牌下发通道③，进入 `Translating`。
    /// 返回是否进入了需要展示浮层的新任务。
    fn accept_input(&mut self, generation: u64, input: TaskInput) -> bool {
        if generation != self.generation || self.state != AppState::Fetching {
            debug!(
                thread = thread::UI,
                generation = generation,
                current = self.generation,
                state = ?self.state,
                "stale or unexpected input ready dropped"
            );
            return false;
        }
        let Some(kind) = self.pending_kind.take() else {
            warn!(
                thread = thread::UI,
                generation = generation,
                "input ready without pending kind, dropped"
            );
            return false;
        };
        let TaskInput::Text { text, hint } = input else {
            debug!(
                thread = thread::UI,
                generation = generation,
                "non-text input ignored"
            );
            return false;
        };
        let cancel = CancellationToken::new();
        self.current_cancel = Some(cancel.clone());
        let task = Task {
            kind,
            input: TaskInput::Text {
                text: text.clone(),
                hint,
            },
            options: TaskOptions::default(),
        };
        let sent = self.endpoints.as_ref().is_some_and(|endpoints| {
            endpoints
                .commands
                .send(Command::RunTask {
                    generation,
                    task,
                    cancel,
                })
                .is_ok()
        });
        if !sent {
            warn!(
                thread = thread::UI,
                generation = generation,
                "command channel closed, task dropped"
            );
            self.current_cancel = None;
            self.state = AppState::Error;
            self.overlay_text = Some("任务失败：推理通道不可用".into());
            return true;
        }
        info!(
            thread = thread::UI,
            generation = generation,
            chars = text.chars().count(),
            "input ready, task dispatched to tokio"
        );
        self.overlay_text = Some(text);
        self.state = AppState::Translating;
        true
    }

    /// 采纳流式增量：追加到浮层文本（原始流，围栏过滤归 M3-T9 的结果卡
    /// 渲染）。返回是否有新内容需要重绘。
    fn accept_chunk(&mut self, generation: u64, delta: String) -> bool {
        if generation != self.generation || self.state != AppState::Translating {
            debug!(
                thread = thread::UI,
                generation = generation,
                current = self.generation,
                state = ?self.state,
                "stale chunk dropped"
            );
            return false;
        }
        match &mut self.overlay_text {
            Some(text) => text.push_str(&delta),
            None => self.overlay_text = Some(delta),
        }
        true
    }

    /// 采纳任务产物：定格正文并进入 `Show`。返回是否需要重绘。
    fn accept_done(&mut self, generation: u64, outcome: TaskOutcome) -> bool {
        if generation != self.generation || self.state != AppState::Translating {
            debug!(
                thread = thread::UI,
                generation = generation,
                current = self.generation,
                state = ?self.state,
                "stale task done dropped"
            );
            return false;
        }
        info!(
            thread = thread::UI,
            generation = generation,
            kind = ?outcome.kind,
            "task done, showing outcome"
        );
        self.overlay_text = Some(outcome.body);
        self.state = AppState::Show;
        true
    }

    /// 采纳任务失败：落 `Error` 态并展示失败信息（下一次触发即重试）。
    /// 返回是否需要展示浮层。
    fn accept_failed(&mut self, generation: u64, error: &GlossError) -> bool {
        if generation != self.generation {
            debug!(
                thread = thread::UI,
                generation = generation,
                current = self.generation,
                "stale task failed dropped"
            );
            return false;
        }
        warn!(
            thread = thread::UI,
            generation = generation,
            error = %error,
            "task failed"
        );
        self.current_cancel = None;
        self.state = AppState::Error;
        self.overlay_text = Some(format!("任务失败：{error}（再次触发可重试）"));
        true
    }

    /// 浮层收起后的状态回落：展示/失败信息不再有意义，清空回到 `Idle`。
    /// 在途任务（若恰在 Translating 时被手动收起）不取消——产物到达时
    /// 浮层虽不在展示，状态机仍按代数走完转移。
    fn hide_overlay_state(&mut self) {
        if matches!(
            self.state,
            AppState::Show | AppState::Error | AppState::Translating
        ) {
            self.state = AppState::Idle;
        }
        self.overlay_text = None;
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
        self.hide_overlay_state();
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

/// 平台事件 → 取材命令的映射（纯逻辑，可单测）：每次触发占用一个新代数；
/// 未接线的平台事件（框选、设置、退出）返回 None，由调用方留诊断日志。
fn acquire_command_for(event: &PlatformEvent, generation: u64) -> Option<AcquireCommand> {
    match event {
        PlatformEvent::HotkeyTriggered { binding } => match binding.source {
            InputSource::Selection => Some(AcquireCommand::AcquireText {
                generation,
                kind: binding.kind,
            }),
            // 图像取材待框选路径接入后消费。
            InputSource::Region => None,
        },
        PlatformEvent::SelectionGesture => Some(AcquireCommand::AcquireText {
            generation,
            kind: TaskKind::TranslateWord,
        }),
        PlatformEvent::RegionGesture { .. }
        | PlatformEvent::OpenSettingsRequested
        | PlatformEvent::QuitRequested => None,
    }
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
                // 失焦回 Idle：只隐藏不销毁；自检轮次不受真实焦点影响
                if self.self_test.is_none()
                    && let Some(windows) = &self.windows
                {
                    windows.hide();
                    self.auto_hide = None;
                    self.hide_overlay_state();
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
    use gloss_core::model::ScreenRect;
    use gloss_core::task::HotkeyBinding;

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

    #[test]
    fn selection_gesture_maps_to_translate_word_command() {
        assert_eq!(
            acquire_command_for(&PlatformEvent::SelectionGesture, 1),
            Some(AcquireCommand::AcquireText {
                generation: 1,
                kind: TaskKind::TranslateWord
            })
        );
    }

    #[test]
    fn hotkey_binding_carries_its_kind_and_requires_selection_source() {
        let binding = HotkeyBinding {
            trigger: "Cmd+Shift+F".into(),
            kind: TaskKind::TranslateSentence,
            source: InputSource::Selection,
        };
        assert_eq!(
            acquire_command_for(&PlatformEvent::HotkeyTriggered { binding }, 7),
            Some(AcquireCommand::AcquireText {
                generation: 7,
                kind: TaskKind::TranslateSentence
            })
        );

        let region = HotkeyBinding {
            trigger: "Cmd+Shift+R".into(),
            kind: TaskKind::ImageOcr,
            source: InputSource::Region,
        };
        assert!(
            acquire_command_for(&PlatformEvent::HotkeyTriggered { binding: region }, 8).is_none(),
            "region source has no acquisition path yet"
        );
    }

    #[test]
    fn unwired_platform_events_are_ignored() {
        let events = [
            PlatformEvent::OpenSettingsRequested,
            PlatformEvent::QuitRequested,
            PlatformEvent::RegionGesture {
                rect: ScreenRect {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
            },
        ];
        for event in &events {
            assert!(
                acquire_command_for(event, 1).is_none(),
                "{event:?} must not acquire"
            );
        }
    }

    /// 构造接入真实通道的 App，返回各通道端点供测试驱动。
    fn driven_app() -> (
        GlossApp,
        crossbeam_channel::Sender<PlatformEvent>,
        crossbeam_channel::Receiver<AcquireCommand>,
        tokio::sync::mpsc::UnboundedReceiver<Command>,
        crossbeam_channel::Sender<Event>,
    ) {
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
        let app = GlossApp {
            endpoints: Some(AppEndpoints {
                platform_events: pe_rx,
                acquire_commands: ac_tx,
                commands: cmd_tx,
                events: ev_rx,
            }),
            ..GlossApp::default()
        };
        (app, pe_tx, ac_rx, cmd_rx, ev_tx)
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

    fn plain_outcome(body: &str) -> TaskOutcome {
        TaskOutcome {
            kind: TaskKind::TranslateWord,
            body: body.into(),
            structured: gloss_core::task::OutcomeStructured::Plain { title: None },
        }
    }

    /// 验收标准：连续触发 A→B 时，A 的迟到 chunk / TaskDone 不串台——
    /// 新触发取消 A 的令牌、推进代数，A 的一切回传被陈旧过滤。
    #[test]
    fn late_events_of_superseded_trigger_do_not_bleed() {
        let (mut app, pe_tx, ac_rx, mut cmd_rx, _ev_tx) = driven_app();

        // 触发 A：进入 Fetching，gen=1，取材命令下发。
        trigger_selection(&mut app, &pe_tx);
        assert_eq!(app.state, AppState::Fetching);
        assert_eq!(app.generation, 1);
        assert!(matches!(
            ac_rx.try_recv().unwrap(),
            AcquireCommand::AcquireText { generation: 1, .. }
        ));

        // A 的取材产物到达：组装 Task 携令牌下发③，进入 Translating。
        assert!(app.accept_input(1, text_input("A")));
        assert_eq!(app.state, AppState::Translating);
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
        assert!(app.overlay_text.as_deref().unwrap().contains("部分A"));

        // 触发 B：A 的令牌立即取消，代数推进，状态回 Fetching。
        trigger_selection(&mut app, &pe_tx);
        assert_eq!(app.generation, 2);
        assert_eq!(app.state, AppState::Fetching);
        assert!(token_a.is_cancelled(), "new trigger must cancel task A");

        // A 的迟到 chunk 被陈旧过滤：既不进入 B 的展示，也不改变状态。
        assert!(!app.accept_chunk(1, "迟到A".into()));
        assert!(
            !app.overlay_text.as_deref().unwrap().contains("迟到A"),
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
        assert_eq!(app.state, AppState::Show);
        assert_eq!(app.overlay_text.as_deref(), Some("结果B"));
    }

    /// 取材失败的回传（gen 不匹配）被丢弃；匹配的失败落 Error 态并可重试。
    #[test]
    fn failed_task_lands_in_error_and_retry_works() {
        let (mut app, pe_tx, _ac_rx, _cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert!(app.accept_input(1, text_input("A")));

        // 陈旧失败（旧代数）丢弃，不影响 Translating。
        assert!(!app.accept_failed(0, &gloss_core::model::GlossError::EngineNetwork));
        assert_eq!(app.state, AppState::Translating);

        assert!(app.accept_failed(1, &gloss_core::model::GlossError::EngineNetwork));
        assert_eq!(app.state, AppState::Error);
        assert!(app.current_cancel.is_none());
        assert!(app.overlay_text.as_deref().unwrap().contains("任务失败"));

        // Error 态再次触发即重试。
        trigger_selection(&mut app, &pe_tx);
        assert_eq!(app.state, AppState::Fetching);
    }

    /// 陈旧的 InputReady 不进入 Translating，也不下发③。
    #[test]
    fn stale_input_ready_is_dropped_entirely() {
        let (mut app, pe_tx, _ac_rx, mut cmd_rx, _ev_tx) = driven_app();
        trigger_selection(&mut app, &pe_tx);
        assert!(!app.accept_input(42, text_input("来自未来")));
        assert_eq!(
            app.state,
            AppState::Fetching,
            "stale input must not move state"
        );
        assert!(
            cmd_rx.try_recv().is_err(),
            "stale input must not reach tokio"
        );
    }
}
