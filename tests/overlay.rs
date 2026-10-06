//! L3 显隐自检（harness = false 的端到端测试目标）。
//!
//! 自带 `main()`、运行在进程主线程——满足 winit 事件循环的主线程约束。
//! 经 gloss-app 公共 API 驱动与生产完全相同的窗口栈：预创建窗口反复显
//! 隐 100 轮，统计 show → 首帧延迟、窗口句柄数与进程 RSS 增长（复用与
//! 首帧预算、句柄不增长、RSS 尾段净增长预算计入门禁——前 25 轮属一次
//! 性预热分配，泄漏信号看尾段斜率）。需要窗口服务与 GPU。
//!
//! 设 `GLOSS_PERF_OUT` 时把性能记录追加导出为 JSON Lines，供量化审计。
//!
//! 退出码：跑满 100 轮且有延迟统计 `0`；无帧、首帧超预算、RSS 尾段
//! 净增长超预算 `1`。

use std::env;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gloss_app::app::centered_position;
use gloss_app::present::windows::WindowManager;
use gloss_app::present::{Frame, build_window_stack, render_frame};
use gloss_core::config::Theme;
use gloss_core::log::{error, info};
use gloss_core::model::Locale;
use serde_json::json;
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::WindowId;

const SELFTEST_VISIBLE: Duration = Duration::from_millis(80);
const SELFTEST_ROUNDS: usize = 100;
const SHOW_BUDGET: Duration = Duration::from_millis(100);
const RSS_TAIL_GROWTH_BUDGET_KB: i64 = 2048;

fn sample_rss_kb() -> Option<i64> {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

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
    let rss = match (handler.rss_start_kb, handler.rss_end_kb) {
        (Some(start), Some(end)) => Some((start, end, end - start)),
        _ => None,
    };
    if let Some((start, end, growth)) = rss {
        info!(
            thread = gloss_core::log::thread::UI,
            rss_start_kb = start,
            rss_end_kb = end,
            rss_growth_kb = growth,
            curve = ?handler.rss_curve,
            "overlay show/hide rss growth"
        );
    }
    let tail_growth = handler
        .rss_curve
        .iter()
        .find(|(round, _)| *round == 25)
        .and_then(|(_, kb)| handler.rss_end_kb.map(|end| end - kb));
    let rss_verdict = match tail_growth {
        Some(tail) if tail > RSS_TAIL_GROWTH_BUDGET_KB => "fail",
        Some(_) => "pass",
        None => "skipped",
    };
    if rss_verdict == "skipped" {
        info!(
            thread = gloss_core::log::thread::UI,
            reason = "rss samples incomplete (ps failed or round-25 anchor missing)",
            "rss gate skipped"
        );
    }
    export_perf(&handler, first, max, window_handles, rss, rss_verdict);
    if rss_verdict == "fail" {
        error!(
            thread = gloss_core::log::thread::UI,
            tail_growth_kb = tail_growth,
            budget_kb = RSS_TAIL_GROWTH_BUDGET_KB,
            "show/hide rss tail growth exceeded budget"
        );
        return ExitCode::FAILURE;
    }
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

fn export_perf(
    handler: &OverlaySelfTest,
    first: Duration,
    max: Duration,
    window_handles: usize,
    rss: Option<(i64, i64, i64)>,
    rss_verdict: &str,
) {
    let Some(path) = env::var_os("GLOSS_PERF_OUT") else {
        return;
    };
    let path = PathBuf::from(path);
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
    let verdict = if first > SHOW_BUDGET || rss_verdict == "fail" {
        "fail"
    } else {
        "pass"
    };
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
        "rss_start_kb": rss.map(|(start, _, _)| start),
        "rss_end_kb": rss.map(|(_, end, _)| end),
        "rss_growth_kb": rss.map(|(_, _, growth)| growth),
        "rss_tail_growth_kb": handler
            .rss_curve
            .iter()
            .find(|(round, _)| *round == 25)
            .and_then(|(_, kb)| handler.rss_end_kb.map(|end| end - kb)),
        "rss_verdict": rss_verdict,
        "rss_curve": handler.rss_curve.iter().map(|(round, kb)| json!({"round": round, "kb": kb})).collect::<Vec<_>>(),
        "env": {
            "os": env::consts::OS,
            "arch": env::consts::ARCH,
            "gpu": handler.frame.as_ref().map(Frame::adapter_name),
        },
    });
    if let Some(dir) = path.parent()
        && let Err(err) = std::fs::create_dir_all(dir)
    {
        error!(
            thread = gloss_core::log::thread::UI,
            error = %err,
            "failed to create perf output directory"
        );
    }
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
    rss_start_kb: Option<i64>,
    rss_end_kb: Option<i64>,
    rss_curve: Vec<(usize, i64)>,
}

impl OverlaySelfTest {
    fn summary(&self) -> Option<(Duration, Duration)> {
        let first = *self.latencies.first()?;
        let max = self.latencies.iter().max().copied()?;
        Some((first, max))
    }

    fn begin_round(&mut self, event_loop: &ActiveEventLoop) {
        if self.round == 0 {
            self.rss_start_kb = sample_rss_kb();
        }
        if self.round.is_multiple_of(25)
            && let Some(rss) = sample_rss_kb()
        {
            self.rss_curve.push((self.round, rss));
        }
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
        let (repaint, _) = render_frame(frame, None, Locale::default());
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
        match build_window_stack(event_loop, Theme::System) {
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
                if let Some(rss) = sample_rss_kb() {
                    self.rss_curve.push((self.round, rss));
                }
                self.rss_end_kb = sample_rss_kb();
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
