//! L3 显隐自检（M1-T6 验收，harness = false 的端到端测试目标）。
//!
//! 自带 `main()`、运行在进程主线程——满足 winit 事件循环的主线程约束。
//! 经 gloss-app 公共 API 驱动与生产完全相同的窗口栈：预创建窗口反复显
//! 隐 100 轮，统计 show → 首帧延迟与窗口句柄数（预创建复用与 < 100ms
//! 首帧预算计入门禁；句柄数进日志供人工走查）。
//!
//! 退出码：跑满 100 轮且有延迟统计 `0`；无帧、首帧超预算 `1`。
//! 仅 macOS：需要窗口服务与 GPU（其余平台直通成功）。

#[cfg(target_os = "macos")]
mod macos_selftest {
    use std::process::ExitCode;
    use std::time::{Duration, Instant};

    use gloss_app::app::{Frame, build_window_stack, centered_position, render_frame};
    use gloss_app::windows::WindowManager;
    use gloss_core::log::{error, info};
    use winit::application::ApplicationHandler;
    use winit::event::{StartCause, WindowEvent};
    use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
    use winit::window::WindowId;

    /// 每轮浮层停留时长
    const SELFTEST_VISIBLE: Duration = Duration::from_millis(80);
    /// 验收轮数（09 M1-T6：反复显隐 100 次）
    const SELFTEST_ROUNDS: usize = 100;
    /// 首次显示预算（09 M1-T6 验收：< 100ms，预创建生效）
    const SHOW_BUDGET: Duration = Duration::from_millis(100);

    /// 自检入口：跑满返回 SUCCESS，无帧或超预算返回 FAILURE。
    pub fn run() -> ExitCode {
        let event_loop = match EventLoop::builder().build() {
            Ok(loop_) => loop_,
            Err(err) => {
                error!(thread = gloss_core::log::thread::UI, error = %err, "failed to build event loop");
                return ExitCode::FAILURE;
            }
        };
        let mut handler = OverlaySelfTest::default();
        if event_loop.run_app(&mut handler).is_err() {
            return ExitCode::FAILURE;
        }
        let Some((first, max)) = handler.summary() else {
            error!(
                thread = gloss_core::log::thread::UI,
                "overlay self-test recorded no frames"
            );
            return ExitCode::FAILURE;
        };
        info!(
            thread = gloss_core::log::thread::UI,
            rounds = handler.round,
            first_show_ms = first.as_millis() as u64,
            max_show_ms = max.as_millis() as u64,
            window_handles = handler
                .windows
                .as_ref()
                .map_or(0, WindowManager::handle_refcount),
            "overlay self-test passed"
        );
        if first > SHOW_BUDGET {
            error!(
                thread = gloss_core::log::thread::UI,
                first_show_ms = first.as_millis() as u64,
                budget_ms = SHOW_BUDGET.as_millis() as u64,
                "first show exceeded budget"
            );
            return ExitCode::FAILURE;
        }
        ExitCode::SUCCESS
    }

    #[derive(Default)]
    struct OverlaySelfTest {
        windows: Option<WindowManager>,
        frame: Option<Frame>,
        /// egui 要求的下一帧时间点；`None` 表示等到有事件再画
        next_repaint: Option<Instant>,
        /// 本轮隐藏时刻
        auto_hide: Option<Instant>,
        /// 当前轮次（1-based）
        round: usize,
        /// 本轮 show_at 时刻；首帧记录后清空，保证每轮只计一次
        shown_at: Option<Instant>,
        latencies: Vec<Duration>,
    }

    impl OverlaySelfTest {
        /// 汇总统计（无帧返回 None）。
        fn summary(&self) -> Option<(Duration, Duration)> {
            let first = *self.latencies.first()?;
            let max = self.latencies.iter().max().copied()?;
            Some((first, max))
        }

        /// 自检的一轮：居中显示，停留 SELFTEST_VISIBLE 后由隐藏路径收回。
        fn begin_round(&mut self, event_loop: &ActiveEventLoop) {
            self.round += 1;
            let now = Instant::now();
            self.shown_at = Some(now);
            self.auto_hide = Some(now + SELFTEST_VISIBLE);
            if let Some(windows) = &self.windows {
                let position = centered_position(event_loop, windows);
                windows.show_at(position);
                windows.request_redraw();
            }
        }

        fn draw(&mut self) {
            let Some(frame) = &mut self.frame else {
                return;
            };
            // 自检期间恒渲染自检卡（视图 None）
            self.next_repaint = render_frame(frame, None);
            if let Some(shown) = self.shown_at.take() {
                self.latencies.push(Instant::now() - shown);
            }
        }
    }

    impl ApplicationHandler for OverlaySelfTest {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            // resumed 可能连续投递，窗口栈只起一次
            if self.windows.is_some() {
                return;
            }
            match build_window_stack(event_loop) {
                Ok((windows, frame)) => {
                    self.windows = Some(windows);
                    self.frame = Some(frame);
                    // 预创建即隐藏（Idle 态）；先在隐藏状态画一帧预热——
                    // egui 图集构建、Metal 管线编译与纹理上传都发生在首帧，
                    // 不预热的话首次显示会超预算
                    self.draw();
                    self.begin_round(event_loop);
                }
                // 没有窗口与渲染栈就没有可做的事，带病进循环只会静默空转
                Err(err) => {
                    error!(
                        thread = gloss_core::log::thread::UI,
                        error = %err,
                        "failed to start window and render stack"
                    );
                    event_loop.exit();
                }
            }
        }

        fn window_event(
            &mut self,
            _event_loop: &ActiveEventLoop,
            window_id: WindowId,
            event: WindowEvent,
        ) {
            if !self.windows.as_ref().is_some_and(|w| w.matches(window_id)) {
                return;
            }
            if let WindowEvent::RedrawRequested = event {
                self.draw();
            }
        }

        fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
            if !matches!(cause, StartCause::ResumeTimeReached { .. }) {
                return;
            }
            let now = Instant::now();
            if self.auto_hide.is_some_and(|deadline| deadline <= now) {
                self.auto_hide = None;
                if let Some(windows) = &self.windows {
                    windows.hide();
                }
                if self.round >= SELFTEST_ROUNDS {
                    event_loop.exit();
                    return;
                }
                self.begin_round(event_loop);
            }
            if self.next_repaint.is_some_and(|deadline| deadline <= now)
                && let Some(windows) = &self.windows
            {
                windows.request_redraw();
            }
        }

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            let next = [self.next_repaint, self.auto_hide]
                .into_iter()
                .flatten()
                .min();
            event_loop.set_control_flow(next.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
        }
    }
}

#[cfg(target_os = "macos")]
fn main() -> std::process::ExitCode {
    macos_selftest::run()
}

#[cfg(not(target_os = "macos"))]
fn main() -> std::process::ExitCode {
    // 无窗口服务/无 GPU 的平台：L3 不适用，直通成功。
    std::process::ExitCode::SUCCESS
}
