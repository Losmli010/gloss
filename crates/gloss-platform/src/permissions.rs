//! 事件权限的预检与授权引导：划词链路只索取辅助功能一项授权。
//!
//! AX 选区读取需要它，listen-only 事件 tap 在它之下同样可建——手势不
//! 额外索取输入监控。缺授权时选区读取返回 `AccessibilityDenied`，不会
//! 静默降级成空选区。授权引导只走系统对话框，不经应用内 UI。

use std::time::{Duration, Instant};

/// 运行中观察的查询间隔：事件线程 33ms 的 tick 远密于授权变化的时间尺度，
/// 按 1s 节流；用户刚从系统设置切回来，秒级延迟无感。
const WATCH_INTERVAL: Duration = Duration::from_secs(1);

/// 纯查询，不弹授权框：用于启动预检与运行中轮询。
pub fn preflight_accessibility() -> bool {
    crate::ffi::ax::is_process_trusted()
}

/// 未授权时顺带弹系统授权引导对话框（内含「打开系统设置」入口）。返回值
/// 仍是查询时刻的状态：对话框非阻塞，最终授权以用户在系统设置为准，调用
/// 方应只在启动预检这类低频节点调用。
pub fn request_accessibility() -> bool {
    crate::ffi::ax::is_process_trusted_prompting()
}

/// 运行中授权观察器：预检未就绪时武装，事件线程每轮 tick 调用一次
/// [`AccessibilityWatch::poll`]。授权落定即产出一次 `true` 并解除武装——
/// 授权不会倒退，之后没有可等的事件。
pub struct AccessibilityWatch {
    armed: bool,
    last_poll: Option<Instant>,
    interval: Duration,
}

impl AccessibilityWatch {
    /// `grant_pending` 是启动预检结论的否定：预检已就绪时返回未武装实例，
    /// 此后 `poll` 恒 `false` 且不查询。
    pub fn armed_if(grant_pending: bool) -> Self {
        Self {
            armed: grant_pending,
            last_poll: None,
            interval: WATCH_INTERVAL,
        }
    }

    /// 到查询节奏且已授权时返回 `true`，此后不再查询。
    pub fn poll(&mut self) -> bool {
        self.poll_at(Instant::now(), preflight_accessibility)
    }

    /// [`poll`] 的时间与查询注入版：测试据此驱动时序与授权序列。
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
