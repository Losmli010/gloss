//! 文本取材适配器：辅助功能读取器，即辅助功能（AX）读选区；宿主激活器，即
//! 浏览器内核家族（Chromium）宿主的辅助功能通道激活；剪贴板兜底读取器，即
//! 模拟复制兜底；以及组合读取器（组合通道）。
//!
//! 端口契约定义在核心层（gloss-core）的端口模块，实现方与调用方共用同一契约
//! （读取须在平台事件线程上调用）。

pub mod accessibility;
pub mod activation;
pub mod clipboard;
pub mod composite;
