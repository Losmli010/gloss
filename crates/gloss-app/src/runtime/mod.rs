//! 后台桥：后台运行时（tokio）消费泵、任务缓存与完成态解析。
//!
//! 非主线程层：不持有任何主线程类型，与主线程仅经通道③④与唤醒句柄交互
//! （拓扑由根入口决定，见壳层的事件装配模块）。

pub mod cache;
pub mod finalize;
pub mod pipeline;
