//! L3 启动预算自检（harness = false 的端到端测试目标）。
//!
//! 经 gloss-app 公共 API 走生产同源的渲染栈初始化：EventLoop 建立后
//! `build_window_stack` 建窗口栈并画预热帧，进程内起点到预热帧完成的
//! 墙钟对 STARTUP_BUDGET 判门禁，分段耗时看日志里程碑 m5a/m5b/m5c。
//! 需要窗口服务与 GPU。退出码：不超预算 `0`；超预算或初始化失败 `1`。

use std::process::ExitCode;
use std::time::{Duration, Instant};

use gloss_app::present::{Frame, build_window_stack, render_frame};
use gloss_core::config::Theme;
use gloss_core::log::{error, info, init};
use gloss_core::model::Locale;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::WindowId;

const STARTUP_BUDGET: Duration = Duration::from_millis(4_000);

fn main() -> ExitCode {
    init(None);
    let started = Instant::now();
    let event_loop = match EventLoop::builder().build() {
        Ok(loop_) => loop_,
        Err(err) => {
            error!(
                thread = gloss_core::log::thread::UI,
                error = %err,
                "failed to build event loop"
            );
            return ExitCode::FAILURE;
        }
    };
    let mut handler = StartupSelfTest {
        started,
        frame: None,
        ready: None,
    };
    if event_loop.run_app(&mut handler).is_err() {
        return ExitCode::FAILURE;
    }
    let Some(elapsed) = handler.ready else {
        error!(
            thread = gloss_core::log::thread::UI,
            "startup self-test recorded no ready point"
        );
        return ExitCode::FAILURE;
    };
    let budget_ms = STARTUP_BUDGET.as_millis() as u64;
    info!(
        thread = gloss_core::log::thread::UI,
        elapsed_ms = elapsed.as_millis() as u64,
        budget_ms = budget_ms,
        "startup self-test finished"
    );
    if elapsed > STARTUP_BUDGET {
        error!(
            thread = gloss_core::log::thread::UI,
            elapsed_ms = elapsed.as_millis() as u64,
            budget_ms = budget_ms,
            "startup exceeded budget"
        );
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

struct StartupSelfTest {
    started: Instant,
    frame: Option<Frame>,
    ready: Option<Duration>,
}

impl ApplicationHandler for StartupSelfTest {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.frame.is_some() {
            return;
        }
        match build_window_stack(event_loop, Theme::System) {
            Ok((_windows, mut frame, _settings_frame)) => {
                let _ = render_frame(&mut frame, None, Locale::default());
                self.ready = Some(self.started.elapsed());
                self.frame = Some(frame);
                event_loop.exit();
            }
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
        _window_id: WindowId,
        _event: WindowEvent,
    ) {
    }
}
