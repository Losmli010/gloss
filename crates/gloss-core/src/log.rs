//! 统一日志封装：全项目唯一日志出口。
//!
//! 其余 crate 不直接依赖 `tracing`，只经 `gloss_core::log` 使用宏与 [`init`]。
//!
//! 跨线程排查靠结构化字段而非字符串拼接，三个字段全链路携带：
//! - `gen`：请求代数，串联一次任务的完整时序；
//! - `kind`：`TaskKind`，按任务类型过滤；
//! - `thread`：线程角色，取 [`thread::UI`] / [`thread::EVENT`] / [`thread::TOKIO`]。
//!
//! 宏虽经本模块转发，事件的 `target` 仍取**调用点**的模块路径，因此
//! `RUST_LOG=gloss_platform=debug` 能精确到下游 crate。
//!
//! `#[instrument]` 是过程宏，无法跨 crate 转发（下游会报 `E0433`）；
//! 下游请改用 [`info_span!`] 等 span 宏配合 [`Instrument`]。

use std::io::{self, IsTerminal};
use std::sync::Once;

use tracing_subscriber::EnvFilter;

pub use tracing::level_filters::LevelFilter;
pub use tracing::{
    Instrument, Level, Span, debug, debug_span, enabled, error, error_span, event, info, info_span,
    trace, trace_span, warn, warn_span,
};

/// 线程角色标签，供 `thread` 字段使用。
pub mod thread {
    pub const UI: &str = "ui";
    pub const EVENT: &str = "event";
    pub const TOKIO: &str = "tokio";
}

/// 基准级别：`RUST_LOG` 未设置或为空时生效。
const DEFAULT_FILTER: &str = "info";

static INIT: Once = Once::new();

/// 初始化全局日志：`info` 打底，`RUST_LOG` 在其之上追加或覆盖。
///
/// 例如 `RUST_LOG=gloss_platform=debug` 让该模块更详细、其余保持 `info`；
/// `RUST_LOG=off` / `warn` 等全局指令仍可整体压制。
///
/// 全进程只调用一次（入口 `src/main.rs` 启动第 1 步）；重复调用为空操作，
/// 库 crate 永不调用。
pub fn init() {
    INIT.call_once(|| {
        let filter = build_filter(std::env::var("RUST_LOG").ok().as_deref());
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_ansi(io::stderr().is_terminal())
            .with_writer(io::stderr)
            .init();
    });
}

/// 基准级别打底 + `raw` 追加：全局指令后写者胜，故 `off` / `warn` 仍能压制打底级别，
/// 而模块指令只影响自己的 target。非法指令由 `EnvFilter` 丢弃并打印原因。
fn build_filter(raw: Option<&str>) -> EnvFilter {
    EnvFilter::new(format!("{DEFAULT_FILTER},{}", raw.unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::{build_filter, init};

    #[test]
    fn init_is_idempotent() {
        init();
        init();
    }

    #[test]
    fn filter_falls_back_to_default_when_unset_or_empty() {
        assert_eq!(build_filter(None).to_string(), "info");
        assert_eq!(build_filter(Some("")).to_string(), "info");
    }

    #[test]
    fn filter_keeps_default_level_alongside_module_directives() {
        let filter = build_filter(Some("gloss_platform=debug")).to_string();
        assert!(
            filter.contains("gloss_platform=debug") && filter.contains("info"),
            "{filter}"
        );
    }

    #[test]
    fn filter_lets_global_directives_override_default() {
        for raw in ["off", "warn", "error"] {
            let filter = build_filter(Some(raw)).to_string();
            assert!(
                filter.contains(raw) && !filter.contains("info"),
                "{raw} -> {filter}"
            );
        }
    }

    #[test]
    fn filter_drops_invalid_directives_but_keeps_valid_ones() {
        assert_eq!(build_filter(Some("gloss=verboes")).to_string(), "info");
        let mixed = build_filter(Some("gloss=debug,NoThing===")).to_string();
        assert!(
            mixed.contains("gloss=debug") && mixed.contains("info"),
            "{mixed}"
        );
    }
}
