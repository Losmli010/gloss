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
//!
//! 输出两路：终端（stderr，TTY 才带色）+ 文件（[`init`] 收到目录时启用，
//! 按天滚动、只留最近 7 份）——用户报障时让他们把日志目录交出来即可。

use std::fs;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::sync::{Once, OnceLock};

use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

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

/// 日志文件名前缀，实际文件为 `gloss.log.<日期>`。
const FILE_PREFIX: &str = "gloss.log";

/// 按天保留的日志文件个数（约一周）。
const MAX_LOG_FILES: usize = 7;

static INIT: Once = Once::new();
static FILE_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
/// 非阻塞写盘的 guard：必须活到进程结束，否则队列里未落盘的日志会被丢掉。
static FILE_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

/// 初始化全局日志：`info` 打底，`RUST_LOG` 在其之上追加或覆盖。
///
/// 例如 `RUST_LOG=gloss_platform=debug` 让该模块更详细、其余保持 `info`；
/// `RUST_LOG=off` / `warn` 等全局指令仍可整体压制。
///
/// `dir` 给出日志目录时终端与文件双写（按天滚动，只留最近 7 份）；目录建不出来
/// 则退回 stderr 单路——日志不可用不应该拖垮启动。返回实际启用的目录，供入口
/// 打印出来方便定位。
///
/// 全进程只调用一次（入口 `src/main.rs` 启动第 1 步）；重复调用为空操作，
/// 库 crate 永不调用。
pub fn init(dir: Option<&Path>) -> Option<PathBuf> {
    INIT.call_once(|| {
        let filter = build_filter(std::env::var("RUST_LOG").ok().as_deref());
        let console = fmt::layer()
            .with_ansi(io::stderr().is_terminal())
            .with_writer(io::stderr);
        let subscriber = tracing_subscriber::registry().with(filter).with(console);
        let file = dir.and_then(open_file_writer);
        let active = file.as_ref().map(|(_, _, dir)| dir.clone());
        match file {
            Some((writer, guard, _)) => {
                let _ = FILE_GUARD.set(guard);
                subscriber
                    .with(fmt::layer().with_ansi(false).with_writer(writer))
                    .init();
            }
            None => subscriber.init(),
        }
        let _ = FILE_DIR.set(active);
    });
    FILE_DIR.get().cloned().flatten()
}

/// 按天滚动的文件写入器；目录不可用时返回 `None`，让日志退回 stderr 单路。
fn open_file_writer(dir: &Path) -> Option<(NonBlocking, WorkerGuard, PathBuf)> {
    fs::create_dir_all(dir).ok()?;
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(FILE_PREFIX)
        .max_log_files(MAX_LOG_FILES)
        .build(dir)
        .ok()?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    Some((writer, guard, dir.to_path_buf()))
}

/// 基准级别打底 + `raw` 追加：全局指令后写者胜，故 `off` / `warn` 仍能压制打底级别，
/// 而模块指令只影响自己的 target。非法指令由 `EnvFilter` 丢弃并打印原因。
fn build_filter(raw: Option<&str>) -> EnvFilter {
    EnvFilter::new(format!("{DEFAULT_FILTER},{}", raw.unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write as _;
    use std::path::PathBuf;

    use super::{FILE_PREFIX, build_filter, init, open_file_writer};

    /// 每个用例独占一个临时目录，避免并行跑测试时互相踩。
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gloss-log-{name}"));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn init_is_idempotent() {
        init(None);
        init(None);
    }

    #[test]
    fn file_writer_creates_missing_directory() {
        let dir = temp_dir("mkdir");
        assert!(!dir.exists());

        let (_, guard, active) = open_file_writer(&dir).expect("writer should be created");

        assert!(dir.is_dir());
        assert_eq!(active, dir);
        drop(guard);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_writer_persists_lines_into_daily_file() {
        let dir = temp_dir("write");
        let (mut writer, guard, _) = open_file_writer(&dir).expect("writer should be created");

        writeln!(writer, "probe line").expect("write should not fail");
        drop(writer);
        drop(guard); // 关掉后台线程，把队列里剩下的日志刷盘

        let (name, content) = fs::read_dir(&dir)
            .expect("log dir should be readable")
            .filter_map(Result::ok)
            .find_map(|entry| {
                let text = fs::read_to_string(entry.path()).ok()?;
                Some((entry.file_name().to_string_lossy().into_owned(), text))
            })
            .expect("a log file should exist");
        assert!(
            name.starts_with(FILE_PREFIX),
            "unexpected file name: {name}"
        );
        assert!(
            content.contains("probe line"),
            "unexpected content: {content}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_writer_degrades_when_directory_is_unusable() {
        // 路径上蹲着一个常规文件，建目录必然失败
        let path = temp_dir("blocked");
        fs::write(&path, b"not a directory").expect("probe file should be writable");

        assert!(open_file_writer(&path).is_none());

        let _ = fs::remove_file(&path);
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
