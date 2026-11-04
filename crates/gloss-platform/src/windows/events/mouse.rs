//! 全局鼠标手势源（系统（Windows）占位）：消费方触及类型的同名镜像。
//!
//! 系统实现的事件监听对应低级鼠标钩子（SetWindowsHookExW），钩子线程
//! 跑消息泵宿主，按移植方案后续阶段补齐；本桩的监听启动即报告
//! 不可用（源为空、降级标志不复位），划词手势在该目标下不产生事件。

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use gloss_core::model::ScreenPoint;

use crate::events::EventSource;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseGesture {
    Selection { pos: ScreenPoint },
}

pub struct MouseSource;

impl MouseSource {
    pub fn spawn() -> (Option<Self>, Arc<AtomicBool>) {
        (None, Arc::new(AtomicBool::new(false)))
    }
}

impl EventSource<MouseGesture> for MouseSource {
    fn poll(&mut self) -> Vec<MouseGesture> {
        Vec::new()
    }
}
