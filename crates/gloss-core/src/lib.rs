//! 领域库（Gloss）的领域层、端口与任务人工智能（AI）层：纯逻辑加内建
//! 大语言模型（LLM）流式传输，零渲染与窗口依赖。
//!
//! 边界：跨层契约只在端口模块声明；模型随任务选项携带——本包不读配置、
//! 不碰缓存；日志与错误文本都不携带选区原文与密钥（防护模块只报类别）。
//!
//! 测试桩的源只在测试桩目录，经路径属性并入本包单测编译。

#[cfg(test)]
extern crate self as gloss_core;

#[cfg(test)]
#[path = "../tests/stubs/mod.rs"]
pub(crate) mod stubs;

mod briefs;
pub mod classify;
pub mod config;
pub mod config_handle;
pub mod engine;
pub mod guard;
pub mod lang_gate;
pub mod log;
pub mod model;
pub mod ports;
pub mod prompt;
mod repair;
pub mod schema;
pub mod task;
mod wordings;
