//! 事件权限的预检与授权引导：划词链路只索取辅助功能（AX）一项授权。
//!
//! 辅助功能选区读取需要它，只听不注入的事件监听在它之下同样可
//! 建——手势不额外索取输入监控。缺授权时选区读取返回权限拒绝错误，不会
//! 静默降级成空选区。授权引导只走系统对话框，不经应用内界面。

use std::time::{Duration, Instant};

const WATCH_INTERVAL: Duration = Duration::from_secs(1);

pub fn preflight_accessibility() -> bool {
    crate::ffi::ax::is_process_trusted()
}

pub fn request_accessibility() -> bool {
    crate::ffi::ax::is_process_trusted_prompting()
}

pub struct AccessibilityWatch {
    armed: bool,
    last_poll: Option<Instant>,
    interval: Duration,
}

impl AccessibilityWatch {
    pub fn armed_if(grant_pending: bool) -> Self {
        Self {
            armed: grant_pending,
            last_poll: None,
            interval: WATCH_INTERVAL,
        }
    }

    pub fn poll(&mut self) -> bool {
        self.poll_at(Instant::now(), preflight_accessibility)
    }

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
