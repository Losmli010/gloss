//! L3 显隐自检（harness = false 的端到端测试目标）。
//!
//! 自带 `main()`、运行在进程主线程——满足 winit 事件循环的主线程约束。
//! 经 gloss-app 公共 API 驱动与生产完全相同的窗口栈：预创建窗口反复显
//! 隐 100 轮（每轮显示重置淡入，复现生产的逐次显形），统计 show → 首
//! 帧延迟、隐藏延迟（hide 调用的主线程耗时）、动画期帧间隔、窗口句柄
//! 数与进程 RSS 增长（首帧预算、RSS 尾段净增长预算计入门禁——前 25 轮
//! 属一次性预热分配，泄漏信号看尾段斜率）。需要窗口服务与 GPU。
//!
//! 信号走 stderr 结构化 JSON 行（`kind` 字段分型）：每轮一条轻量 round
//! 标记行（round 序号，第 25 轮为预热边界），收尾一条 `overlay_perf`
//! 汇总行（延迟分位/帧时间/尾段/句柄/预算与判定字段）；完整 RSS 曲线由
//! selftest-report wrapper 在进程外 ps 轮询采集，不在本测试内。
//!
//! 退出码：跑满 100 轮且有延迟统计 `0`；无帧、首帧超预算、RSS 尾段
//! 净增长超预算 `1`。
//!
//! 日志纪律的豁免面：信号行是数据导出通道（wrapper 按字段提取），不是
//! 诊断日志——诊断走 `error!`——stderr 结构化行只能直写，故整文件豁免
//! print_stderr（缘由见上）。
#![allow(clippy::print_stderr)]

use std::env;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use gloss_app::app::centered_position;
use gloss_app::present::windows::WindowManager;
use gloss_app::present::{Frame, build_window_stack, render_frame};
use gloss_core::config::Theme;
use gloss_core::log::{error, init};
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
const WARMUP_ROUNDS: usize = 25;
const FRAME_INTERVAL_BUDGET_MS: f64 = 32.0;

fn sample_rss_kb() -> Option<i64> {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[(rank.max(1) - 1).min(sorted.len() - 1)]
}

fn perf_commit() -> String {
    env::var("GLOSS_PERF_COMMIT")
        .or_else(|_| env::var("GITHUB_SHA"))
        .unwrap_or_default()
}

fn main() -> ExitCode {
    init(None);
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

    let mut latencies_ms: Vec<f64> = handler
        .latencies
        .iter()
        .map(|latency| latency.as_secs_f64() * 1000.0)
        .collect();
    latencies_ms.sort_by(f64::total_cmp);
    let mut hide_ms: Vec<f64> = handler
        .hide_latencies
        .iter()
        .map(|latency| latency.as_secs_f64() * 1000.0)
        .collect();
    hide_ms.sort_by(f64::total_cmp);
    let mut frame_ms = handler.frame_intervals.clone();
    frame_ms.sort_by(f64::total_cmp);
    let hide_p50_ms = (!hide_ms.is_empty()).then(|| percentile(&hide_ms, 50.0));
    let hide_max_ms = hide_ms.last().copied();
    let frame_p95_ms = (!frame_ms.is_empty()).then(|| percentile(&frame_ms, 95.0));

    let rss_growth_kb = match (handler.rss_start_kb, handler.rss_end_kb) {
        (Some(start), Some(end)) => Some(end - start),
        _ => None,
    };
    let tail_growth = handler
        .rss_curve
        .iter()
        .find(|(round, _)| *round == WARMUP_ROUNDS)
        .and_then(|(_, kb)| handler.rss_end_kb.map(|end| end - kb));
    let rss_verdict = match tail_growth {
        Some(tail) if tail > RSS_TAIL_GROWTH_BUDGET_KB => "fail",
        Some(_) => "pass",
        None => "skipped",
    };
    let verdict = if first > SHOW_BUDGET || rss_verdict == "fail" {
        "fail"
    } else {
        "pass"
    };
    eprintln!(
        "{}",
        json!({
            "kind": "overlay_perf",
            "commit": perf_commit(),
            "first_ms": first.as_secs_f64() * 1000.0,
            "p50_ms": percentile(&latencies_ms, 50.0),
            "p95_ms": percentile(&latencies_ms, 95.0),
            "max_ms": max.as_secs_f64() * 1000.0,
            "hide_p50_ms": hide_p50_ms,
            "hide_max_ms": hide_max_ms,
            "frame_p95_ms": frame_p95_ms,
            "frame_missed": handler.frame_missed,
            "rss_growth_kb": rss_growth_kb,
            "rss_tail_growth_kb": tail_growth,
            "rss_tail_budget_kb": RSS_TAIL_GROWTH_BUDGET_KB,
            "window_handles": window_handles,
            "budget_ms": SHOW_BUDGET.as_millis() as u64,
            "verdict": verdict,
            "env": {
                "os": env::consts::OS,
                "arch": env::consts::ARCH,
                "gpu": handler.frame.as_ref().map(Frame::adapter_name),
            },
        })
    );
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

#[derive(Default)]
struct OverlaySelfTest {
    windows: Option<WindowManager>,
    frame: Option<Frame>,
    next_repaint: Option<Instant>,
    auto_hide: Option<Instant>,
    round: usize,
    shown_at: Option<Instant>,
    latencies: Vec<Duration>,
    hide_latencies: Vec<Duration>,
    last_frame_at: Option<Instant>,
    frame_intervals: Vec<f64>,
    frame_missed: usize,
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
        if self.round.is_multiple_of(WARMUP_ROUNDS)
            && let Some(rss) = sample_rss_kb()
        {
            self.rss_curve.push((self.round, rss));
        }
        self.round += 1;
        eprintln!("{}", json!({"kind": "overlay_round", "round": self.round}));
        let now = Instant::now();
        self.shown_at = Some(now);
        self.auto_hide = Some(now + SELFTEST_VISIBLE);
        self.last_frame_at = None;
        if let Some((windows, frame)) = self.windows.as_ref().zip(self.frame.as_ref()) {
            frame.reset_appear_animation();
            let position = centered_position(event_loop, windows);
            windows.show_at(position);
            windows.request_redraw();
        }
    }

    fn draw(&mut self) {
        let Some(frame) = &mut self.frame else {
            return;
        };
        let render_started = Instant::now();
        let (repaint, _) = render_frame(frame, None, Locale::default());
        self.next_repaint = repaint;
        if let Some(shown) = self.shown_at.take() {
            self.latencies.push(Instant::now() - shown);
        }
        if let Some(prev) = self.last_frame_at {
            let interval_ms = (render_started - prev).as_secs_f64() * 1000.0;
            if interval_ms > FRAME_INTERVAL_BUDGET_MS {
                self.frame_missed += 1;
            }
            self.frame_intervals.push(interval_ms);
        }
        self.last_frame_at = Some(render_started);
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
            let hide_started = Instant::now();
            if let Some(windows) = &self.windows {
                windows.hide();
            }
            self.hide_latencies.push(hide_started.elapsed());
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
