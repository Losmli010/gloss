//! 文本取材适配器：`AccessibilityReader`（`AX` 读选区）、`AxActivator`
//! （`Chromium` 宿主的 `AX` 通道激活）、`ClipboardFallbackReader`（模拟复制
//! 兜底）与 `CompositeReader`（组合通道）。
//!
//! 端口 `trait` 定义在 `gloss-core::ports`，实现方与调用方共用同一契约（读取须
//! 在平台事件线程上调用）。

pub mod accessibility;
pub mod activation;
pub mod clipboard;
pub mod composite;
