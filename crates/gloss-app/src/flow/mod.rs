//! 编排层：主线程用例——通道消费、动作执行、编辑会话生命周期。
//!
//! 上承壳（只分发事件），下调决策与呈现；
//! 视图只经返回值上交动作，由本层执行。

pub(crate) mod actions;
pub(crate) mod probe;
pub(crate) mod reveal;
pub(crate) mod session;
pub(crate) mod settings_session;
pub(crate) mod task;
