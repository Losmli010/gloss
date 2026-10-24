//! 文本取材适配器：辅助功能读取器（AX 读选区）、宿主激活器
//! （Chromium 宿主的 AX 通道激活）、剪贴板兜底读取器（模拟复制
//! 兜底）与组合读取器（组合通道）。
//!
//! 端口契约定义在 gloss-core 的端口模块，实现方与调用方共用同一契约（读取须
//! 在平台事件线程上调用）。

pub mod accessibility;
pub mod activation;
pub mod clipboard;
pub mod composite;
