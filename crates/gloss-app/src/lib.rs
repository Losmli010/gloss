//! Gloss 表现层与应用层：状态机、窗口管理与渲染胶水。

#[cfg(test)]
#[path = "../tests/stubs/mod.rs"]
pub(crate) mod stubs;

pub mod app;
pub mod channel;
pub(crate) mod flow;
pub mod machine;
pub mod present;
pub mod runtime;
pub mod ui;
pub mod update;
