//! 浮层显隐自检（M1-T6 验收，分层测试的 L3 被测对象）。
//!
//! 独立的 `ApplicationHandler`：复用预创建窗口栈反复显隐 100 轮，统计
//! show → 首帧延迟与窗口句柄数。与生产 [`crate::app::GlossApp`] 完全
//! 分离——自检不进入生产状态机。退出码：无帧或未达成为 `Err`（进程退
//! 出码非 0），供 CI 冒烟与本地 `just e2e` 类脚本断言。

use std::error::Error;
use std::time::{Duration, Instant};

use gloss_core::log::{error, info, thread, warn};
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::WindowId;

use crate::app::{Frame, build_window_stack, centered_position, render_frame};
use crate::windows::WindowManager;

/// 每轮浮层停留时长
const SELFTEST_VISIBLE: Duration = Duration::from_millis(80);
/// 验收轮数（09 M1-T6：反复显隐 100 次）
const SELFTEST_ROUNDS: usize = 100;
/// 首次显示预算（09 M1-T6 验收：< 100ms，预创建生效）
const SHOW_BUDGET: Duration = Duration::from_millis(100);

/// 启动自检事件循环；全部轮次跑完且有延迟统计则 `Ok`，否则 `Err`。
pub fn run() -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::builder().build()?;
    let mut handler = OverlaySelfTest::default();
    event_loop.run_app(&mut handler)?;
    match handler.summary() {
        Some((first, max)) => {
            let handles = handler
                .windows
                .as_ref()
                .map_or(0, WindowManager::handle_refcount);
            info!(
                thread = thread::UI,
                rounds = SELFTEST_ROUNDS,
                first_show_ms = first.as_millis() as u64,
                max_show_ms = max.as_millis() as u64,
                window_handles = handles,
                "overlay self-test passed"
            );
            if first > SHOW_BUDGET {
                return Err(format!(
                    "first show {}ms exceeded budget {}ms",
                    first.as_millis(),
                    SHOW_BUDGET.as_millis()
                )
                .into());
            }
            Ok(())
        }
        None => Err("overlay self-test recorded no frames".into()),
    }
}

#[derive(Default)]
struct OverlaySelfTest {
    windows: Option<WindowManager>,
    frame: Option<Frame>,
    /// egui 要求的下一帧时间点；`None` 表示等到有事件再画
    next_repaint: Option<Instant>,
    /// 本轮隐藏时刻
    auto_hide: Option<Instant>,
    /// 轮次与延迟统计
    stats: Option<Stats>,
}

/// 纯逻辑部分：轮次推进与首帧延迟统计，不碰窗口，可单测。
///
/// 每轮 = show_at → 首帧上屏（记延迟）→ 到点隐藏；跑满即输出统计退出。
struct Stats {
    /// 当前轮次（1-based）
    round: usize,
    /// 本轮 show_at 时刻；首帧记录后清空，保证每轮只计一次
    shown_at: Option<Instant>,
    latencies: Vec<Duration>,
}

impl Stats {
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
        if let Some(latency) = &latency {
            self.latencies.push(*latency);
        }
        latency
    }

    /// 是否已跑满计划轮数。
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

impl OverlaySelfTest {
    /// 自检的一轮：居中显示，停留 SELFTEST_VISIBLE 后由隐藏路径收回。
    fn begin_round(&mut self, event_loop: &ActiveEventLoop) {
        let Some(windows) = &self.windows else {
            return;
        };
        let position = centered_position(event_loop, windows);
        if let Some(windows) = &self.windows {
            windows.show_at(position);
        }
        let now = Instant::now();
        self.auto_hide = Some(now + SELFTEST_VISIBLE);
        if let Some(stats) = &mut self.stats {
            stats.start_round(now);
        }
        if let Some(windows) = &self.windows {
            windows.request_redraw();
        }
    }

    fn hide(&mut self) {
        self.auto_hide = None;
        if let Some(windows) = &self.windows {
            windows.hide();
        }
    }

    fn draw(&mut self) {
        let Some(frame) = self.frame.as_mut() else {
            return;
        };
        let repaint_at = render_frame(frame, None);
        self.next_repaint = repaint_at;

        // 每轮只把 show→首帧 计一次（first_paint 内部已去重）
        if let Some(latency) = self
            .stats
            .as_mut()
            .and_then(|stats| stats.first_paint(Instant::now()))
            && latency > SHOW_BUDGET
        {
            warn!(
                thread = thread::UI,
                latency_ms = latency.as_millis() as u64,
                "overlay show exceeded budget"
            );
        }
    }

    fn request_redraw(&self) {
        if let Some(windows) = &self.windows {
            windows.request_redraw();
        }
    }

    /// 自检收尾：退出事件循环，统计结果交给 [`run`] 判定。
    fn finish(&self, event_loop: &ActiveEventLoop) {
        if self.stats.as_ref().and_then(Stats::summary).is_none() {
            error!(thread = thread::UI, "overlay self-test recorded no frames");
        }
        event_loop.exit();
    }
}

impl OverlaySelfTest {
    /// 汇总统计（无帧返回 None），供 [`run`] 判定退出码。
    fn summary(&self) -> Option<(Duration, Duration)> {
        self.stats.as_ref().and_then(Stats::summary)
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
                if self.stats.is_none() {
                    self.stats = Some(Stats::new());
                }
                // 浮层预创建即隐藏（Idle 态）；先在隐藏状态画一帧预热——
                // egui 图集构建、Metal 管线编译与纹理上传都发生在首帧
                self.draw();
                self.begin_round(event_loop);
            }
            // 没有窗口与渲染栈就没有可做的事，带病进主循环只会静默空转
            Err(err) => {
                error!(
                    thread = thread::UI,
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
            self.hide();
            let complete = self
                .stats
                .as_ref()
                .is_some_and(|s| s.is_complete(SELFTEST_ROUNDS));
            if complete {
                self.finish(event_loop);
            } else {
                self.begin_round(event_loop);
            }
        }
        if self.next_repaint.is_some_and(|deadline| deadline <= now) {
            self.request_redraw();
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 验收（09 M1-T6）：显隐 100 次后收尾，每轮恰好记一次首帧延迟。
    #[test]
    fn selftest_completes_after_hundred_rounds() {
        let mut st = Stats::new();
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
        let mut st = Stats::new();
        for _ in 0..SELFTEST_ROUNDS - 1 {
            st.start_round(Instant::now());
            st.is_complete(SELFTEST_ROUNDS);
        }
        assert!(!st.is_complete(SELFTEST_ROUNDS));
    }
}
