//! Gloss 表现层与应用层：状态机、窗口管理与渲染胶水。

// 测试桩的唯一源在 tests/mock/，这里把同一份源并入库内单测编译。
#[cfg(test)]
#[path = "../tests/mock/mod.rs"]
pub(crate) mod mock;

pub mod app;
pub mod channel;
pub mod gpu;
pub mod machine;
pub mod pipeline;
pub mod ui;
pub mod windows;
