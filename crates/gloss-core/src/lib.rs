//! Gloss 领域层、端口与任务 AI 层：纯逻辑 + 内建 LLM 流式传输，零渲染/窗口依赖。

// 测试桩的唯一源在 tests/stubs/，这里把同一份源并入库内单测编译；
// 自引用别名让桩文件在两个编译上下文里统一写 `gloss_core::` 路径。
#[cfg(test)]
extern crate self as gloss_core;

#[cfg(test)]
#[path = "../tests/stubs/mod.rs"]
pub(crate) mod stubs;

pub mod classify;
pub mod config;
pub mod config_handle;
pub mod engine;
pub mod guard;
pub mod log;
pub mod model;
pub mod ports;
pub mod prompt;
pub mod task;
