//! 组合读取器的系统（macOS）绑定：辅助功能（AX）读取器、宿主激活器与剪
//! 贴板兜底读取器接上共享组合通道（平台中立共享层）。
//!
//! 组合判定、让渡调度、激活闸门与读等待预算住平台中立共享层（common）；
//! 本模块只绑定三个平台原语：辅助功能（AX）读选区、浏览器内核家族
//! （Chromium）宿主的辅助功能通道激活与模拟复制兜底。唤醒常客判定随激活
//! 器跨手势复用，会话内按进程标识（pid）去重并记忆；读取入口现读一次——常客与否
//! 随前台应用，一次读取期间不变；激活器的可变借用留给唤醒闭包，注入只在
//! 通道让步后发生。取材轮次闩锁句柄传给兜底通道——写盘轮次由它置位，哨
//! 兵按计数归因消费。“权限被拒不兜底”的降级边界由共享通道的激活闸门形
//! 态承载：无闸门形态没有权限语义，系统（Windows）侧照旧直接落兜底。
//!
//! 端口契约定义在核心层（gloss-core）的端口模块，实现方与调用方共用同一
//! 契约（读取须在平台事件线程上调用）。

use gloss_core::ports::{TextAcquirer, TextRead, TextSettle};

use crate::clipboard::WatchRoundFlag;
use crate::common::composite::acquire;
use crate::selection::accessibility::AccessibilityReader;
use crate::selection::activation::AxActivator;
use crate::selection::clipboard::ClipboardFallbackReader;

#[derive(Debug)]
pub struct CompositeReader {
    ax: AccessibilityReader,
    clipboard: ClipboardFallbackReader,
    activator: AxActivator,
}

impl CompositeReader {
    pub fn new(round: WatchRoundFlag) -> Self {
        Self {
            ax: AccessibilityReader,
            clipboard: ClipboardFallbackReader::new(round),
            activator: AxActivator::default(),
        }
    }
}

impl TextAcquirer for CompositeReader {
    fn read(&mut self) -> TextRead {
        let CompositeReader {
            ax,
            clipboard,
            activator,
        } = self;
        let responds_to_wake = activator.responds_to_wake();
        let mut wake = || activator.wake();
        let wake_ref: &mut dyn FnMut() -> bool = &mut wake;
        acquire(
            || ax.read(),
            responds_to_wake,
            Some(wake_ref),
            || {
                let (outcome, settle) = clipboard.read();
                (outcome, TextSettle::deferred(move || settle.finish()))
            },
        )
    }
}
