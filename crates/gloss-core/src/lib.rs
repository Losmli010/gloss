//! Gloss 领域层与端口：纯逻辑，零平台/UI 依赖。

// 测试桩的唯一源在 tests/mock/，这里把同一份源并入库内单测编译；
// 自引用别名让桩文件在两个编译上下文里统一写 `gloss_core::` 路径。
#[cfg(test)]
extern crate self as gloss_core;

#[cfg(test)]
#[path = "../tests/mock/mod.rs"]
pub(crate) mod mock;

pub mod cache;
pub mod config;
pub mod config_handle;
pub mod engine;
pub mod log;
pub mod model;
pub mod ports;
pub mod prompt;
pub mod task;
