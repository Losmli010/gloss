//! 文本取材适配器：用户界面自动化（UIA）读取器，即文本模式（TextPattern）
//! 读选区；剪贴板兜底读取器，即按键注入（SendInput）模拟复制兜底；以及
//! 组合读取器（组合通道）。
//!
//! 端口契约定义在核心层（gloss-core）的端口模块，实现方与调用方共用同一契约
//! （读取须在平台事件线程上调用）。

pub mod clipboard;
pub mod composite;
pub mod uia;
