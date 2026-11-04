//! 剪贴板图片哨兵源（系统（Windows）占位）：消费方触及类型的同名镜像。
//!
//! 系统实现的“变更计数轮询 + 类型清单判定”对应剪贴板序列号
//! （GetClipboardSequenceNumber）查询，按移植方案后续阶段补齐；本桩的
//! 轮询恒产出空集，不触发图片取材。

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::events::EventSource;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasteboardImageObserved;

#[derive(Debug, Clone, Default)]
pub struct PasteboardObserver;

impl PasteboardObserver {
    pub fn matches(&self, _current: u64) -> bool {
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoundWrites {
    pub pre: u64,
    pub ours: u64,
}

#[derive(Debug, Clone, Default)]
pub struct WatchRoundFlag;

impl WatchRoundFlag {
    pub fn enter(&self, _writes: RoundWrites) {}
}

pub struct ClipboardWatchSource;

impl ClipboardWatchSource {
    pub fn new(_enabled: Arc<AtomicBool>) -> Self {
        Self
    }

    pub fn observer(&self) -> PasteboardObserver {
        PasteboardObserver
    }

    pub fn round_flag(&self) -> WatchRoundFlag {
        WatchRoundFlag
    }
}

impl EventSource<PasteboardImageObserved> for ClipboardWatchSource {
    fn poll(&mut self) -> Vec<PasteboardImageObserved> {
        Vec::new()
    }
}
