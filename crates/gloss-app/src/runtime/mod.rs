//! 后台桥：tokio 消费泵、任务缓存与完成态解析。
//!
//! 非主线程层：不持有任何主线程类型，与主线程仅经通道③④ + wake 交互
//! （拓扑由根 main.rs 决定，见 `app::events`）。

pub mod cache;
pub mod finalize;
pub mod pipeline;
