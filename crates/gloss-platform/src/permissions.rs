//! 事件权限的预检与授权引导。
//!
//! 划词链路的取材授权只有**辅助功能**（kTCCServiceAccessibility）一项：
//! AX 选区读取的前提，缺失时选区读取返回
//! `gloss_core::model::GlossError::AccessibilityDenied`；listen-only 事件
//! tap 在辅助功能授权下同样可建立，手势不索取第二项系统授权。有公开查询
//! API（[`preflight_accessibility`]），且支持让系统直接弹授权引导对话框
//! （[`request_accessibility`]）。
//!
//! 授权引导统一走系统级 UI（系统授权对话框），不经过应用内对话框。启动
//! 预检未就绪的会话可用 [`AccessibilityWatch`] 在运行中观察授权落定。

use std::time::{Duration, Instant};

/// 运行中授权观察的查询间隔：事件线程 33ms 的 tick 远密于授权变化的时间
/// 尺度，纯查询按此间隔节流；授权落定后秒级延迟无感知差异（用户刚从系统
/// 设置切回来）。
const WATCH_INTERVAL: Duration = Duration::from_secs(1);

/// 当前进程是否已获辅助功能授权（AX 选区读取可用）；纯查询，无副作用。
pub fn preflight_accessibility() -> bool {
    crate::ffi::ax::is_process_trusted()
}

/// 查询辅助功能授权，未授权时顺带弹系统授权引导对话框（内含「打开系统
/// 设置」入口）。返回值仍是查询时刻的授权状态；系统对话框非阻塞，最终
/// 结果以用户在系统设置里的操作为准。调用方应把调用频次控制在启动预检
/// 这类低频节点上。
pub fn request_accessibility() -> bool {
    crate::ffi::ax::is_process_trusted_prompting()
}

/// 运行中辅助功能授权观察器：启动预检未就绪时由组装点武装，事件线程每轮
/// tick 抽干时 [`AccessibilityWatch::poll`] 一次；真实查询按
/// [`WATCH_INTERVAL`] 节流，授权落定即产出一次 `true` 并解除武装——一次性
/// 迁移（授权不会倒退），持续轮询没有后续事件可等。
pub struct AccessibilityWatch {
    armed: bool,
    last_poll: Option<Instant>,
    interval: Duration,
}

impl AccessibilityWatch {
    /// 武装观察器：`grant_pending` 为启动预检结论的否定——预检已就绪的
    /// 会话没有可等的事件，返回未武装实例（`poll` 恒 `false` 且不查询）。
    pub fn armed_if(grant_pending: bool) -> Self {
        Self {
            armed: grant_pending,
            last_poll: None,
            interval: WATCH_INTERVAL,
        }
    }

    /// 抽干一轮：到查询节奏且授权已落定时返回 `true`（仅一次）。
    pub fn poll(&mut self) -> bool {
        self.poll_at(Instant::now(), preflight_accessibility)
    }

    /// [`poll`] 的时间与查询注入版：纯逻辑，测试驱动时序与授权序列用。
    fn poll_at(&mut self, now: Instant, mut granted: impl FnMut() -> bool) -> bool {
        if !self.armed {
            return false;
        }
        if let Some(last) = self.last_poll
            && now.duration_since(last) < self.interval
        {
            return false;
        }
        self.last_poll = Some(now);
        let granted = granted();
        if granted {
            self.armed = false;
        }
        granted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn armed_watch_fires_once_then_disarms() {
        let t0 = Instant::now();
        let mut watch = AccessibilityWatch::armed_if(true);
        assert!(!watch.poll_at(t0, || false));
        assert!(watch.poll_at(t0 + WATCH_INTERVAL, || true));
        let mut queries = 0;
        assert!(!watch.poll_at(t0 + 2 * WATCH_INTERVAL, || {
            queries += 1;
            true
        }));
        assert_eq!(queries, 0, "a fired watch must disarm and stop querying");
    }

    #[test]
    fn unarmed_watch_never_queries() {
        let mut queries = 0;
        let mut watch = AccessibilityWatch::armed_if(false);
        assert!(!watch.poll_at(Instant::now(), || {
            queries += 1;
            true
        }));
        assert_eq!(queries, 0);
    }

    #[test]
    fn queries_are_throttled_to_the_interval() {
        use std::cell::Cell;
        let t0 = Instant::now();
        let mut watch = AccessibilityWatch::armed_if(true);
        let queries = Cell::new(0);
        let mut grant = || {
            queries.set(queries.get() + 1);
            false
        };
        assert!(!watch.poll_at(t0, &mut grant));
        assert!(!watch.poll_at(t0 + WATCH_INTERVAL / 2, &mut grant));
        assert_eq!(queries.get(), 1, "half-interval polls must skip the query");
        assert!(!watch.poll_at(t0 + WATCH_INTERVAL, &mut grant));
        assert_eq!(queries.get(), 2, "a full-interval poll queries again");
    }

    #[test]
    fn a_poll_exactly_at_the_interval_queries() {
        let t0 = Instant::now();
        let mut watch = AccessibilityWatch::armed_if(true);
        assert!(!watch.poll_at(t0, || false));
        let mut queries = 0;
        assert!(!watch.poll_at(t0 + WATCH_INTERVAL, || {
            queries += 1;
            false
        }));
        assert_eq!(queries, 1);
    }
}
