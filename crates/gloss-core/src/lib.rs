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

#[path = "pipeline/briefs.rs"]
mod briefs;
#[path = "pipeline/classify.rs"]
pub mod classify;
#[path = "domain/config.rs"]
pub mod config;
pub mod config_handle;
#[path = "pipeline/engine/mod.rs"]
pub mod engine;
#[path = "domain/guard.rs"]
pub mod guard;
#[path = "pipeline/lang_gate/mod.rs"]
pub mod lang_gate;
#[path = "pipeline/langspec.rs"]
pub(crate) mod langspec;
pub mod log;
#[path = "domain/model.rs"]
pub mod model;
pub mod ports;
#[path = "pipeline/prompt.rs"]
pub mod prompt;
#[path = "pipeline/repair.rs"]
mod repair;
#[path = "domain/schema.rs"]
pub mod schema;
#[path = "domain/task.rs"]
pub mod task;
#[path = "pipeline/wordings.rs"]
mod wordings;
