//! 事件权限的预检与授权引导（系统（Windows）占位）。
//!
//! 系统（Windows）无系统级授权门：预检恒放行、引导为无操作，选区读取走
//! 用户界面自动化（UIA）不索取权限（移植方案能力映射的定案）。授权
//! 监视源保留同名占位，轮询恒不放行——按移植方案后续阶段随真实现
//! 一并落定或移除。

use std::time::{Duration, Instant};

const WATCH_INTERVAL: Duration = Duration::from_secs(1);

pub fn preflight_accessibility() -> bool {
    true
}

pub fn request_accessibility() -> bool {
    preflight_accessibility()
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
        if !self.armed {
            return false;
        }
        if let Some(last) = self.last_poll
            && Instant::now().duration_since(last) < self.interval
        {
            return false;
        }
        self.last_poll = Some(Instant::now());
        false
    }
}
