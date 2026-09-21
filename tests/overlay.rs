//! L3 显隐自检（harness = false 的端到端测试目标）。
//!
//! 自带 `main()`、运行在进程主线程——满足 winit 事件循环的主线程约束。
//! 经 gloss-app 公共 API 驱动与生产完全相同的窗口栈：预创建窗口反复显
//! 隐 100 轮，统计 show → 首帧延迟与窗口句柄数（预创建复用与 < 100ms
//! 首帧预算计入门禁；句柄数进日志供人工走查）。需要窗口服务与 GPU。
//!
//! 设 `GLOSS_PERF_OUT` 时，把延迟统计（first/p50/p95/max）与运行环境
//! 以 JSON Lines 追加到指定文件，供量化审计；导出失败只记日志，不改变
//! 退出码。commit 取 `GLOSS_PERF_COMMIT`，缺省回退 CI 的 `GITHUB_SHA`。
//!
//! 退出码：跑满 100 轮且有延迟统计 `0`；无帧、首帧超预算 `1`。

use std::env;
use std::fs::OpenOptions;
use std::io::Write;
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gloss_app::app::{Frame, build_window_stack, centered_position, render_frame};
use gloss_app::windows::WindowManager;
use gloss_core::log::{error, info};
use serde_json::json;
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::WindowId;

const SELFTEST_VISIBLE: Duration = Duration::from_millis(80);
const SELFTEST_ROUNDS: usize = 100;
const SHOW_BUDGET: Duration = Duration::from_millis(100);

fn main() -> ExitCode {
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
    let window_handles = handler
        .windows
        .as_ref()
        .map_or(0, WindowManager::handle_refcount);
    info!(
        thread = gloss_core::log::thread::UI,
        rounds = handler.round,
        first_show_ms = first.as_millis() as u64,
        max_show_ms = max.as_millis() as u64,
        window_handles = window_handles,
        "overlay self-test passed"
    );
    export_perf(&handler, first, max, window_handles);
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

fn export_perf(handler: &OverlaySelfTest, first: Duration, max: Duration, window_handles: usize) {
    let Some(path) = env::var_os("GLOSS_PERF_OUT") else {
        return;
    };
    let mut sorted: Vec<f64> = handler
        .latencies
        .iter()
        .map(|latency| latency.as_secs_f64() * 1000.0)
        .collect();
    sorted.sort_by(f64::total_cmp);
    let percentile = |p: f64| -> f64 {
        if sorted.is_empty() {
            return 0.0;
        }
        let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
        sorted[(rank.max(1) - 1).min(sorted.len() - 1)]
    };
    let verdict = if first > SHOW_BUDGET { "fail" } else { "pass" };
    let record = json!({
        "kind": "overlay",
        "commit": perf_commit(),
        "ts_unix_ms": SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as u64),
        "rounds": handler.round,
        "first_ms": first.as_secs_f64() * 1000.0,
        "p50_ms": percentile(50.0),
        "p95_ms": percentile(95.0),
        "max_ms": max.as_secs_f64() * 1000.0,
        "budget_ms": SHOW_BUDGET.as_millis() as u64,
        "verdict": verdict,
        "window_handles": window_handles,
        "env": {
            "os": env::consts::OS,
            "arch": env::consts::ARCH,
            "gpu": handler.frame.as_ref().map(Frame::adapter_name),
        },
    });
    let outcome = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| writeln!(file, "{record}"));
    if let Err(err) = outcome {
        error!(
            thread = gloss_core::log::thread::UI,
            error = %err,
            "failed to append perf record"
        );
    }
}

fn perf_commit() -> String {
    env::var("GLOSS_PERF_COMMIT")
        .or_else(|_| env::var("GITHUB_SHA"))
        .unwrap_or_default()
}

#[derive(Default)]
struct OverlaySelfTest {
    windows: Option<WindowManager>,
    frame: Option<Frame>,
    next_repaint: Option<Instant>,
    auto_hide: Option<Instant>,
    round: usize,
    shown_at: Option<Instant>,
    latencies: Vec<Duration>,
}

impl OverlaySelfTest {
    fn summary(&self) -> Option<(Duration, Duration)> {
        let first = *self.latencies.first()?;
        let max = self.latencies.iter().max().copied()?;
        Some((first, max))
    }

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
        let (repaint, _, _) = render_frame(frame, None);
        self.next_repaint = repaint;
        if let Some(shown) = self.shown_at.take() {
            self.latencies.push(Instant::now() - shown);
        }
    }
}

impl ApplicationHandler for OverlaySelfTest {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.windows.is_some() {
            return;
        }
        match build_window_stack(event_loop) {
            Ok((windows, frame, _settings_frame)) => {
                self.windows = Some(windows);
                self.frame = Some(frame);
                self.draw();
                self.begin_round(event_loop);
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
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if !self
            .windows
            .as_ref()
            .is_some_and(|w| w.matches_overlay(window_id))
        {
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
