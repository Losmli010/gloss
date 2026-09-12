//! 文本取材适配器：AccessibilityReader（macOS AX 读选区）。
//!
//! 后续在此汇合剪贴板兜底与 CompositeReader 组合；端口 trait 定义在
//! gloss-core::ports，接线时对齐同一契约（读取须在平台事件线程上调用）。

pub mod accessibility;
