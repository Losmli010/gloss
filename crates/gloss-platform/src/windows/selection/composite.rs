//! 组合读取器的系统（Windows）绑定：用户界面自动化（UIA）读取器与剪贴板
//! 兜底读取器接上共享组合通道（平台中立共享层）。
//!
//! 组合判定、让渡调度与读等待预算住平台中立共享层（common）；本模块只绑
//! 定两个平台原语：用户界面自动化（UIA）文本模式读选区与按键注入
//! （SendInput）模拟复制兜底。激活闸门缺省——系统（Windows）没有需要唤
//! 醒的无障碍树：浏览器内核家族（Chromium）系宿主的树默认建树，文本模式缺席时等
//! 再久也读不到，注入才是解法；权限语义同样缺省——让渡耗尽与“读不到”
//! 都直接落兜底，没有权限拒绝分支。
//!
//! 端口契约定义在核心层（gloss-core）的端口模块，实现方与调用方共用同一
//! 契约（读取须在平台事件线程上调用）。

use gloss_core::ports::{TextAcquirer, TextRead, TextSettle};

use crate::clipboard::WatchRoundFlag;
use crate::common::composite::acquire;
use crate::selection::clipboard::ClipboardFallbackReader;
use crate::selection::uia::UiaReader;

pub struct CompositeReader {
    uia: UiaReader,
    clipboard: ClipboardFallbackReader,
}

impl CompositeReader {
    pub fn new(round: WatchRoundFlag) -> Self {
        Self {
            uia: UiaReader::new(),
            clipboard: ClipboardFallbackReader::new(round),
        }
    }
}

impl TextAcquirer for CompositeReader {
    fn read(&mut self) -> TextRead {
        let CompositeReader { uia, clipboard } = self;
        acquire(
            || uia.read(),
            false,
            None,
            || {
                let (outcome, settle) = clipboard.read();
                (outcome, TextSettle::deferred(move || settle.finish()))
            },
        )
    }
}
