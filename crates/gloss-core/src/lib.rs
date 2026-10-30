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

pub mod contract;
pub mod domain;
pub mod log;
pub mod pipeline;

pub use contract::config_handle;
pub use contract::ports;
pub use domain::config;
pub use domain::guard;
pub use domain::model;
pub use domain::schema;
pub use domain::task;
pub use pipeline::classify;
pub use pipeline::engine;
pub use pipeline::lang_gate;
pub(crate) use pipeline::langspec;
pub use pipeline::prompt;
pub(crate) use pipeline::repair;
pub(crate) use pipeline::wordings;
