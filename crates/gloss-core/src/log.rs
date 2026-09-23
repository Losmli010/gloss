//! 统一日志封装：全项目唯一日志出口。
//!
//! 其余 crate 不直接依赖 `tracing`，只经 `gloss_core::log` 使用宏与 [`init`]。
//!
//! 跨线程排查靠结构化字段而非字符串拼接，三个字段全链路携带：
//! - `gen`：请求代数，串联一次任务的完整时序——由 [`task_span`] 的 span 字段
//!   自动带给范围内的日志，跨线程只传 span 句柄；
//! - `kind`：`TaskKind`，按任务类型过滤；
//! - `thread`：线程角色，取 [`thread::UI`] / [`thread::EVENT`] / [`thread::TOKIO`]。
//!
//! 宏虽经本模块转发，事件的 `target` 仍取**调用点**的模块路径，因此
//! `RUST_LOG=gloss_platform=debug` 能精确到下游 crate。
//!
//! `#[instrument]` 是过程宏，无法跨 crate 转发（下游会报 `E0433`）；
//! 下游请改用 [`info_span!`] 等 span 宏配合 [`Instrument`]。
//!
//! 输出两路：终端（stderr）+ 文件（[`init`] 收到目录时启用，按天滚动、只留最近 7 份）
//! ——用户报障时让他们把日志目录交出来即可。两路都是 **JSON Lines**：一行一个
//! JSON 对象，字段是 `timestamp` / `level` / `target` / `message` 与本次事件的
//! 结构化字段；span 字段挂在 `span` 下（任务 span 的代数因此在 `span.generation`）。
//! 查询举例：`jq 'select(.span.generation == 2)' 日志文件`。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Once, OnceLock};

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
    /// 主线程：winit 事件循环 + UI。
    pub const UI: &str = "ui";
    /// 平台事件线程：热键、鼠标手势与取材。
    pub const EVENT: &str = "event";
    /// 鼠标 tap 监听线程：rdev 全局事件流的独立宿主。
    pub const MOUSE_TAP: &str = "mouse_tap";
    /// tokio 后台：网络请求与缓存。
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
/// 非阻塞写盘的 guard：必须活到进程结束。
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
        let console = json_layer().with_writer(io::stderr);
        let subscriber = tracing_subscriber::registry().with(filter).with(console);
        let file = dir.and_then(open_file_writer);
        let active = file.as_ref().map(|(_, _, dir)| dir.clone());
        match file {
            Some((writer, guard, _)) => {
                // init 在 INIT.call_once 内只执行一次，OnceLock::set 的已占用
                // Err 在这里不可能发生，丢弃即可。
                #[allow(clippy::let_underscore_must_use)]
                let _ = FILE_GUARD.set(guard);
                subscriber.with(json_layer().with_writer(writer)).init();
            }
            None => subscriber.init(),
        }
        // 同上：call_once 内首次 set，Err 不可能发生。
        #[allow(clippy::let_underscore_must_use)]
        let _ = FILE_DIR.set(active);
    });
    FILE_DIR.get().cloned().flatten()
}

/// 任务 span：以 `generation` 字段承载请求代数，范围内所有日志自动带上它。
///
/// 在触发点创建（代数在那里赋值），句柄随通道下发到平台事件线程与 tokio——
/// 深层的日志点（取材读选区、推理引擎）因此不必自己携带代数。无订阅者的
/// 进程里是禁用 span，进入它是空操作。
///
/// 级别取 `WARN` 而不是 `INFO`：span 被级别压掉时**字段也不会出现**，而任务
/// 作用域里要留住的恰是 warn 及以上那几行（拒绝授权、后台 panic）——排障时
/// 最需要它们。作用域内没有 error 级日志，故 warn 覆盖了「任何可能打印的
/// 级别」；`RUST_LOG=error` 时这些行同样被压掉，不丢信息。
pub fn task_span(generation: u64) -> Span {
    warn_span!("task", generation)
}

/// 日志行格式（两路共用）：JSON Lines，事件字段摊到顶层，span 字段留在 `span` 下
/// （JSON 形态本身不带颜色转义，不需要 ANSI 开关）。span 列表关掉是为了避免把同一个
/// span 再抄一份到 `spans` 数组里。
fn json_layer<S>() -> fmt::Layer<S, fmt::format::JsonFields, fmt::format::Format<fmt::format::Json>>
{
    fmt::layer()
        .json()
        .flatten_event(true)
        .with_span_list(false)
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

/// 在捕获订阅者下运行 `f`，返回这段时间里格式化后的日志文本（含 span 字段）。
///
/// 订阅者是**线程局部**的：与全局 [`init`] 互不影响，也不干扰其它线程，因此
/// 可以并行调用；过滤器取 [`init`] 的同一档基准级别，所以断言同时钉住了
/// 「这条日志在生产默认级别下也打得出来」。
///
/// 两个坑：**span 必须在 `f` 里创建**——tracing 在创建时就按当时订阅者的兴趣
/// 定启用与否，没有订阅者时创建出来的 span 永远是禁用态，事后再捕获也补不
/// 回来；以及 `f` 内新起的线程不在此订阅者覆盖范围内（跨线程断言用
/// [`capture_global`]）。
pub fn capture<F: FnOnce()>(f: F) -> String {
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry()
        .with(build_filter(None))
        .with(json_layer().with_writer(CaptureWriter(Arc::clone(&buffer))));
    tracing::subscriber::with_default(subscriber, f);
    let bytes = buffer
        .lock()
        .map(|buffer| buffer.clone())
        .unwrap_or_default();
    String::from_utf8(bytes).unwrap_or_default()
}

/// 装上**进程级**捕获订阅者，返回读取端；日志来自别的线程时用它。
///
/// 与 [`init`] 互斥（一个进程只有一个全局订阅者）：测试进程里没人调用 [`init`]，
/// 同一进程重复调用本函数拿到同一个读取端（只装一次）。
pub fn capture_global() -> CaptureReader {
    static GLOBAL: OnceLock<Arc<Mutex<Vec<u8>>>> = OnceLock::new();
    let buffer = Arc::clone(GLOBAL.get_or_init(|| {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry()
            .with(build_filter(None))
            .with(json_layer().with_writer(CaptureWriter(Arc::clone(&buffer))));
        // 测试进程里没有别的全局订阅者；万一已有，说明调用方用错了工具，
        // 这里按「捕获不可用」继续，断言会如实失败。
        #[allow(clippy::let_underscore_must_use)]
        let _ = tracing::subscriber::set_global_default(subscriber);
        buffer
    }));
    CaptureReader(buffer)
}

/// [`capture_global`] 的读取端：按需取走累积到此刻的日志文本。
pub struct CaptureReader(Arc<Mutex<Vec<u8>>>);

impl CaptureReader {
    /// 已捕获的日志文本（每次调用取一份快照，不清空缓冲）。
    pub fn text(&self) -> String {
        let bytes = self
            .0
            .lock()
            .map(|buffer| buffer.clone())
            .unwrap_or_default();
        String::from_utf8(bytes).unwrap_or_default()
    }
}

/// 把格式化后的日志行收进内存缓冲的写入器（[`capture`] 用）。
struct CaptureWriter(Arc<Mutex<Vec<u8>>>);

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CaptureWriter {
    type Writer = CaptureHandle;

    fn make_writer(&'a self) -> Self::Writer {
        CaptureHandle(Arc::clone(&self.0))
    }
}

struct CaptureHandle(Arc<Mutex<Vec<u8>>>);

impl io::Write for CaptureHandle {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .map(|mut bytes| bytes.extend_from_slice(buf))
            .map_err(|_| io::Error::other("capture buffer poisoned"))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// 基准级别打底 + `raw` 追加：全局指令后写者胜，故 `off` / `warn` 仍能压制打底级别，
/// 而模块指令只影响自己的 target。非法指令由 `EnvFilter` 丢弃并打印原因。
fn build_filter(raw: Option<&str>) -> EnvFilter {
    EnvFilter::new(format!("{DEFAULT_FILTER},{}", raw.unwrap_or_default()))
}

#[cfg(test)]
#[allow(clippy::let_underscore_must_use)]
mod tests {
    use std::fs;
    use std::io::Write as _;
    use std::path::PathBuf;

    use super::{FILE_PREFIX, build_filter, capture, info, init, open_file_writer, task_span};

    fn probe_line(text: &str) -> &str {
        text.lines()
            .find(|line| line.contains("probe"))
            .expect("probe line must be captured")
    }

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
        let path = temp_dir("blocked");
        fs::write(&path, b"not a directory").expect("probe file should be writable");

        assert!(open_file_writer(&path).is_none());

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn task_span_carries_generation_into_events() {
        let text = capture(|| {
            let span = task_span(7);
            let _entered = span.enter();
            info!(thread = crate::log::thread::EVENT, "probe");
        });

        let line = probe_line(&text);
        let value: serde_json::Value = serde_json::from_str(line).expect("日志行应是 JSON");
        assert_eq!(value["span"]["generation"], 7, "{line}");
        assert_eq!(value["message"], "probe", "{line}");
    }

    #[test]
    fn json_lines_carry_the_structured_contract() {
        let text = capture(|| {
            info!(
                thread = crate::log::thread::EVENT,
                kind = "probe",
                "probe json contract"
            );
        });

        let line = probe_line(&text);
        assert!(!line.contains('\u{1b}'), "JSON 行不带 ANSI 转义：{line}");

        let value: serde_json::Value = serde_json::from_str(line).expect("日志行应是 JSON");
        assert_eq!(value["level"], "INFO", "{line}");
        assert_eq!(value["message"], "probe json contract", "{line}");
        assert_eq!(value["thread"], "event", "{line}");
        assert_eq!(value["kind"], "probe", "{line}");
        assert!(
            value["target"]
                .as_str()
                .is_some_and(|t| t.starts_with("gloss")),
            "{line}"
        );
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
