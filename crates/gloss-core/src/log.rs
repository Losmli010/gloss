//! 统一日志封装：全项目唯一日志出口。
//!
//! 其余包不直接依赖 tracing，只经核心库日志模块使用宏与初始化入口。
//!
//! 跨线程排查靠结构化字段而非字符串拼接，三个字段全链路携带：
//! - 请求代数：串联一次任务的完整时序——由任务 span 自动带给
//!   范围内的日志，跨线程只传 span 句柄；
//! - 任务类别：按任务类型过滤；
//! - 线程角色：界面、事件、后台三种。
//!
//! 宏虽经本模块转发，事件的日志目标仍取**调用点**的模块路径，因此
//! 日志过滤环境变量能精确到下游包。
//!
//! 插桩属性宏无法跨包转发（下游会报编译错误）；
//! 下游请改用 span 宏并显式进入 span 范围。
//!
//! 输出两路：终端（标准错误）+ 文件（初始化入口收到目录时启用，按天滚动、只留最近 7 份）
//! ——用户报障时让他们把日志目录交出来即可。两路都是 **JSON 行**：一行一个
//! JSON 对象，字段是时间戳、级别、目标、消息与本次事件的
//! 结构化字段；任务 span 的代数直接提到顶层（不写嵌套
//! 对象）。查询举例：用日志检视配方按代数筛选。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Once, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

pub use tracing::{
    Instrument, Level, Span, debug, debug_span, enabled, error, error_span, event, info, info_span,
    trace, trace_span, warn, warn_span,
};

pub mod thread {
    pub const UI: &str = "ui";
    pub const EVENT: &str = "event";
    pub const MOUSE_TAP: &str = "mouse_tap";
    pub const FONTS: &str = "fonts";
    pub const TOKIO: &str = "tokio";
}

const DEFAULT_FILTER: &str = "info";

const FILE_PREFIX: &str = "gloss-";
const FILE_SUFFIX: &str = ".jsonl";

const MAX_LOG_FILES: usize = 7;

static INIT: Once = Once::new();
static FILE_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
static FILE_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

pub fn init(dir: Option<&Path>) -> Option<PathBuf> {
    INIT.call_once(|| {
        let filter = build_filter(std::env::var("RUST_LOG").ok().as_deref());
        let console = json_layer().with_writer(|| shaped_handle(io::stderr()));
        let subscriber = tracing_subscriber::registry().with(filter).with(console);
        let file = dir.and_then(open_file_writer);
        let active = match file {
            Some((writer, guard, dir)) => {
                #[allow(clippy::let_underscore_must_use)]
                let _ = FILE_GUARD.set(guard);
                subscriber
                    .with(json_layer().with_writer(move || shaped_handle(writer.clone())))
                    .init();
                Some(dir)
            }
            None => {
                subscriber.init();
                None
            }
        };
        #[allow(clippy::let_underscore_must_use)]
        let _ = FILE_DIR.set(active);
    });
    FILE_DIR.get().cloned().flatten()
}

pub fn task_span(generation: u64) -> Span {
    warn_span!("task", generation)
}

static MILESTONE_START: OnceLock<Instant> = OnceLock::new();

pub fn milestone(id: &str) {
    let start = MILESTONE_START.get_or_init(Instant::now);
    let elapsed_ms = start.elapsed().as_millis() as u64;
    info!(
        thread = thread::UI,
        milestone = id,
        elapsed_ms = elapsed_ms,
        "startup milestone"
    );
}

fn json_layer<S>() -> fmt::Layer<S, fmt::format::JsonFields, fmt::format::Format<fmt::format::Json>>
{
    fmt::layer()
        .json()
        .flatten_event(true)
        .with_span_list(false)
}

fn shaped(line: &[u8]) -> Vec<u8> {
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(line) else {
        return line.to_vec();
    };
    let Some(object) = value.as_object_mut() else {
        return line.to_vec();
    };
    let generation = object
        .remove("span")
        .and_then(|span| span.get("generation").cloned());
    object.remove("spans");
    if let Some(generation) = generation {
        object.insert("generation".to_owned(), generation);
    }
    let mut shaped = serde_json::to_vec(&value).unwrap_or_else(|_| line.to_vec());
    shaped.push(b'\n');
    shaped
}

fn shaped_handle<W: io::Write>(inner: W) -> ShapedHandle<W> {
    ShapedHandle {
        inner,
        pending: Vec::new(),
    }
}

struct ShapedHandle<W> {
    inner: W,
    pending: Vec<u8>,
}

impl<W: io::Write> io::Write for ShapedHandle<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.pending.extend_from_slice(buf);
        while let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            self.inner.write_all(&shaped(&line))?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn open_file_writer(dir: &Path) -> Option<(NonBlocking, WorkerGuard, PathBuf)> {
    fs::create_dir_all(dir).ok()?;
    let (writer, guard) = tracing_appender::non_blocking(DailyFileWriter::new(dir.to_path_buf()));
    Some((writer, guard, dir.to_path_buf()))
}

struct DailyFileWriter {
    dir: PathBuf,
    current: Option<(String, fs::File)>,
}

impl DailyFileWriter {
    fn new(dir: PathBuf) -> Self {
        Self { dir, current: None }
    }

    fn open_today(&mut self) -> io::Result<()> {
        let today = today_utc();
        if self
            .current
            .as_ref()
            .is_some_and(|(date, _)| *date == today)
        {
            return Ok(());
        }
        let path = self.dir.join(log_file_name(&today));
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        self.current = Some((today, file));
        prune_old_logs(&self.dir);
        Ok(())
    }
}

impl io::Write for DailyFileWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.open_today()?;
        match self.current.as_mut() {
            Some((_, file)) => file.write(buf),
            None => Err(io::Error::other("log file unavailable")),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.current.as_mut() {
            Some((_, file)) => file.flush(),
            None => Ok(()),
        }
    }
}

fn log_file_name(date: &str) -> String {
    format!("{FILE_PREFIX}{date}{FILE_SUFFIX}")
}

fn today_utc() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| (elapsed.as_secs() / 86_400) as i64)
        .unwrap_or(0);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = (month_index + if month_index < 10 { 3 } else { -9 }) as u32;
    (year + i64::from(month <= 2), month, day)
}

fn prune_old_logs(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with(FILE_PREFIX) && name.ends_with(FILE_SUFFIX))
        .collect();
    names.sort();
    let excess = names.len().saturating_sub(MAX_LOG_FILES);
    for name in names.into_iter().take(excess) {
        #[allow(clippy::let_underscore_must_use)]
        let _ = fs::remove_file(dir.join(name));
    }
}

pub fn capture<F: FnOnce()>(f: F) -> String {
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry()
        .with(build_filter(None))
        .with(json_layer().with_writer({
            let buffer = Arc::clone(&buffer);
            move || shaped_handle(CaptureHandle(Arc::clone(&buffer)))
        }));
    tracing::subscriber::with_default(subscriber, || {
        tracing::callsite::rebuild_interest_cache();
        f();
    });
    let bytes = buffer
        .lock()
        .map(|buffer| buffer.clone())
        .unwrap_or_default();
    String::from_utf8(bytes).unwrap_or_default()
}

pub fn capture_global() -> CaptureReader {
    static GLOBAL: OnceLock<Arc<Mutex<Vec<u8>>>> = OnceLock::new();
    let buffer = Arc::clone(GLOBAL.get_or_init(|| {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry()
            .with(build_filter(None))
            .with(json_layer().with_writer({
                let buffer = Arc::clone(&buffer);
                move || shaped_handle(CaptureHandle(Arc::clone(&buffer)))
            }));
        #[allow(clippy::let_underscore_must_use)]
        let _ = tracing::subscriber::set_global_default(subscriber);
        buffer
    }));
    CaptureReader(buffer)
}

pub struct CaptureReader(Arc<Mutex<Vec<u8>>>);

impl CaptureReader {
    pub fn text(&self) -> String {
        let bytes = self
            .0
            .lock()
            .map(|buffer| buffer.clone())
            .unwrap_or_default();
        String::from_utf8(bytes).unwrap_or_default()
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

fn build_filter(raw: Option<&str>) -> EnvFilter {
    EnvFilter::new(format!("{DEFAULT_FILTER},{}", raw.unwrap_or_default()))
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Mutex, MutexGuard, OnceLock};

    pub(crate) fn lock_dispatchers() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
#[allow(clippy::let_underscore_must_use)]
mod tests {
    use std::fs;
    use std::io::Write as _;
    use std::path::PathBuf;

    use super::test_support::lock_dispatchers;
    use super::{
        DailyFileWriter, FILE_PREFIX, MAX_LOG_FILES, build_filter, capture, civil_from_days, info,
        init, log_file_name, milestone, open_file_writer, prune_old_logs, task_span, today_utc,
    };

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
    fn civil_date_matches_known_anchors() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(10_957), (2000, 1, 1));
        assert_eq!(civil_from_days(20_720), (2026, 9, 24));
        assert_eq!(civil_from_days(20_818), (2026, 12, 31));
    }

    #[test]
    fn log_file_name_is_dated_jsonl() {
        assert_eq!(log_file_name("2026-09-24"), "gloss-2026-09-24.jsonl");
    }

    #[test]
    fn daily_writer_appends_into_todays_file() {
        let dir = temp_dir("daily");
        fs::create_dir_all(&dir).expect("dir");
        let mut writer = DailyFileWriter::new(dir.clone());

        writer
            .write_all(b"{\"message\":\"first\"}\n")
            .expect("write");
        writer
            .write_all(b"{\"message\":\"second\"}\n")
            .expect("write");
        writer.flush().expect("flush");

        let content = fs::read_to_string(dir.join(log_file_name(&today_utc())))
            .expect("today's log file should exist");
        assert!(
            content.contains("first") && content.contains("second"),
            "{content}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_keeps_the_newest_files_only() {
        let dir = temp_dir("prune");
        fs::create_dir_all(&dir).expect("dir");
        for day in 1..=9 {
            fs::write(dir.join(log_file_name(&format!("2026-09-0{day}"))), b"x").expect("seed");
        }
        fs::write(dir.join("notes.txt"), b"keep me").expect("seed");

        prune_old_logs(&dir);

        let mut left: Vec<String> = fs::read_dir(&dir)
            .expect("dir")
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect();
        left.sort();
        assert_eq!(left.len(), MAX_LOG_FILES + 1, "{left:?}");
        assert!(
            !left.contains(&"gloss-2026-09-01.jsonl".to_owned()),
            "{left:?}"
        );
        assert!(left.contains(&"notes.txt".to_owned()), "{left:?}");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn init_is_idempotent() {
        let _serial = lock_dispatchers();
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
        drop(guard);

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
        let _serial = lock_dispatchers();
        let text = capture(|| {
            let span = task_span(7);
            let _entered = span.enter();
            info!(thread = crate::log::thread::EVENT, "probe");
        });

        let line = probe_line(&text);
        let value: serde_json::Value = serde_json::from_str(line).expect("日志行应是 JSON");
        assert_eq!(value["generation"], 7, "{line}");
        assert!(value.get("span").is_none(), "{line}");
        assert_eq!(value["message"], "probe", "{line}");
    }

    #[test]
    fn logs_outside_a_task_carry_no_generation() {
        let _serial = lock_dispatchers();
        let text = capture(|| {
            info!(thread = crate::log::thread::UI, "probe without task");
        });

        let line = probe_line(&text);
        let value: serde_json::Value = serde_json::from_str(line).expect("日志行应是 JSON");
        assert!(value.get("generation").is_none(), "{line}");
        assert!(value.get("span").is_none(), "{line}");
    }

    #[test]
    fn json_lines_carry_the_structured_contract() {
        let _serial = lock_dispatchers();
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
    fn milestone_event_carries_id_and_elapsed_ms() {
        let _serial = lock_dispatchers();
        let text = capture(|| milestone("m_test_probe"));

        let line = probe_line(&text);
        let value: serde_json::Value = serde_json::from_str(line).expect("日志行应是 JSON");
        assert_eq!(value["level"], "INFO", "{line}");
        assert_eq!(value["message"], "startup milestone", "{line}");
        assert_eq!(value["thread"], "ui", "{line}");
        assert_eq!(value["milestone"], "m_test_probe", "{line}");
        assert!(
            value["elapsed_ms"].as_u64().is_some(),
            "elapsed_ms 应为非负整数：{line}"
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
