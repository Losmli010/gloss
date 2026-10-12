//! 文本取材适配器：AccessibilityReader（AX 读选区）、AxActivator
//! （Chromium 宿主的 AX 通道激活）、ClipboardFallbackReader（模拟复制
//! 兜底）与 CompositeReader（组合通道）。
//!
//! 端口 trait 定义尚在 gloss-core::ports，接线时对齐同一契约（读取须在
//! 平台事件线程上调用）。

pub mod accessibility;
pub mod activation;
pub mod clipboard;
pub mod composite;
