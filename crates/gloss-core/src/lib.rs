//! `Gloss` 领域层、端口与任务 `AI` 层：纯逻辑 + 内建 `LLM` 流式传输，零渲染/窗口依赖。
//!
//! 边界：跨层 `trait` 只在 [`ports`] 声明；模型随任务选项携带——本 `crate` 不读配置、
//! 不碰缓存；日志与错误文本都不携带选区原文与密钥（[`guard`] 只报类别）。
//!
//! 测试桩的源只在 `tests/stubs/`，经下方 `#[path]` 并入本 `crate` 单测编译。

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
mod repair;
pub mod schema;
pub mod task;
