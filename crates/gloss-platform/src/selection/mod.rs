//! 文本取材适配器：AccessibilityReader（macOS AX 读选区）与
//! ClipboardFallbackReader（模拟复制兜底）。
//!
//! 端口 trait 定义尚在 gloss-core::ports，接线时对齐同一契约（读取须在
//! 平台事件线程上调用）。

pub mod accessibility;
pub mod clipboard;
