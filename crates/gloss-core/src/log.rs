//! 统一日志封装：全项目唯一日志出口。
//!
//! 其余 crate 不直接依赖 `tracing`，只经 `gloss_core::log` 使用宏与 [`init`]。
//!
//! 跨线程排查靠结构化字段而非字符串拼接，三个字段全链路携带：
//! - `gen`：请求代数，串联一次任务的完整时序；
//! - `kind`：`TaskKind`，按任务类型过滤；
//! - `thread`：线程角色，取 [`thread::UI`] / [`thread::EVENT`] / [`thread::TOKIO`]。

use std::io;
use std::sync::Once;

use tracing_subscriber::EnvFilter;

pub use tracing::{
    Span, debug, debug_span, error, error_span, info, info_span, trace, trace_span, warn, warn_span,
};

/// 线程角色标签，供 `thread` 字段使用。
pub mod thread {
    pub const UI: &str = "ui";
    pub const EVENT: &str = "event";
    pub const TOKIO: &str = "tokio";
}

/// `RUST_LOG` 未设置或无法解析时使用的级别。
const DEFAULT_FILTER: &str = "info";

static INIT: Once = Once::new();

/// 初始化全局日志：默认 `info`，`RUST_LOG` 可运行时覆盖并按模块精细过滤
/// （如 `RUST_LOG=gloss_platform=debug`）。
///
/// 全进程只调用一次（入口 `src/main.rs` 启动第 1 步）；重复调用为空操作，
/// 库 crate 永不调用。
pub fn init() {
    INIT.call_once(|| {
        let filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(io::stderr)
            .init();
    });
}

#[cfg(test)]
mod tests {
    use super::init;

    #[test]
    fn init_is_idempotent() {
        init();
        init();
    }
}
