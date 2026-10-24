//! 评测历史数据层：报告追加进历史记录文件——一行一份（JSON）文本，
//! 不包壳——并从全量历史重绘趋势图。本模块是历史数据的唯一碰盘点；
//! 趋势渲染是被它调用的纯渲染层，不反向依赖。

use std::path::Path;

use crate::report::EvalReport;
use crate::svg;

const HISTORY_FILE: &str = "history.jsonl";
const SVG_FILE: &str = "history.svg";

pub fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    let dir = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("out");
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, contents)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            std::fs::remove_file(&tmp).ok();
            Err(error)
        }
    }
}

pub fn record_run(report: &EvalReport, dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(HISTORY_FILE);
    let existing = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    let line = serde_json::to_string(report).map_err(std::io::Error::other)?;
    let mut lines: Vec<String> = existing
        .lines()
        .filter(|row| !row.trim().is_empty())
        .map(str::to_owned)
        .collect();
    let mut replaced = false;
    if let Some(key) = upsert_key(report) {
        for row in &mut lines {
            if let Some((mode, commit)) = parsed_key(row)
                && (mode.as_str(), commit.as_str()) == key
            {
                *row = line.clone();
                replaced = true;
            }
        }
    }
    if !replaced {
        lines.push(line);
    }
    let mut body = lines.join("\n");
    body.push('\n');
    write_atomic(&path, &body)?;
    let history = load_history(&path)?;
    write_atomic(&dir.join(SVG_FILE), &svg::render_svg(&history))?;
    Ok(())
}

pub fn load_history(path: &Path) -> std::io::Result<Vec<EvalReport>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut history = Vec::new();
    for (index, row) in text.lines().enumerate() {
        if row.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<EvalReport>(row) {
            Ok(report) => history.push(report),
            Err(error) => warn_bad_row(path, index + 1, &error),
        }
    }
    Ok(history)
}

fn upsert_key(report: &EvalReport) -> Option<(&str, &str)> {
    let commit = report.context.as_ref()?.commit.as_deref()?;
    Some((&report.mode, commit))
}

fn parsed_key(row: &str) -> Option<(String, String)> {
    let report = serde_json::from_str::<EvalReport>(row).ok()?;
    let (mode, commit) = upsert_key(&report)?;
    Some((mode.to_owned(), commit.to_owned()))
}

#[allow(clippy::print_stderr)]
fn warn_bad_row(path: &Path, line: usize, error: &serde_json::Error) {
    eprintln!(
        "警告：{} 第 {line} 行无法解析，已跳过（{error}）",
        path.display()
    );
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::history::{load_history, record_run};
    use crate::metrics::{ClassifyMetrics, TaskMetrics};
    use crate::report::{EvalContext, EvalReport, TrackOutputs, assemble_report};

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "gloss-eval-history-{tag}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn classify_metrics() -> ClassifyMetrics {
        ClassifyMetrics {
            evaluated: 2,
            correct: 1,
            ..ClassifyMetrics::default()
        }
    }

    fn report(mode: &str, generated_at: i64) -> EvalReport {
        let mut report = assemble_report(
            mode,
            TrackOutputs {
                classify: Some(classify_metrics()),
                tasks: vec![("task_translate_word", TaskMetrics::default())],
                latency: None,
                judge: None,
            },
            0,
        );
        report.generated_at = generated_at;
        report
    }

    fn with_context(report: &EvalReport, commit: Option<&str>, source: Option<&str>) -> EvalReport {
        let mut owned = report.clone();
        owned.context = Some(EvalContext {
            commit: commit.map(str::to_owned),
            source: source.map(str::to_owned),
        });
        owned
    }

    fn write_lines(dir: &std::path::Path, body: &str) {
        let path = dir.join("history.jsonl");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("parent");
        }
        std::fs::write(path, body).expect("write fixture");
    }

    #[test]
    fn first_run_appends_one_line() {
        let dir = temp_dir("first-run");
        record_run(&report("replay", 1000), &dir).expect("record");
        let path = dir.join("history.jsonl");
        let text = std::fs::read_to_string(&path).expect("history text");
        assert!(text.ends_with('\n'), "lines end with LF");
        assert_eq!(text.lines().count(), 1);
        let history = load_history(&path).expect("load");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].mode, "replay");
        assert_eq!(history[0].generated_at, 1000);
        assert!(history[0].context.is_none(), "local runs carry no context");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn same_mode_and_commit_overwrites_in_place() {
        let dir = temp_dir("upsert-same");
        let first = with_context(&report("replay", 1000), Some("ec96217"), Some("pipeline"));
        let second = with_context(&report("replay", 2000), Some("ec96217"), Some("pipeline"));
        record_run(&first, &dir).expect("first");
        record_run(&second, &dir).expect("second");
        let text = std::fs::read_to_string(dir.join("history.jsonl")).expect("history text");
        assert_eq!(text.lines().count(), 1, "same key rewrites in place");
        assert!(
            text.contains(r#""generated_at":2000"#),
            "the newer report wins"
        );
        assert_eq!(
            load_history(&dir.join("history.jsonl"))
                .expect("load")
                .len(),
            1
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn different_commit_or_mode_appends() {
        let dir = temp_dir("upsert-diff");
        let base = report("replay", 1000);
        record_run(&with_context(&base, Some("aaaaaaa"), None), &dir).expect("first");
        record_run(&with_context(&base, Some("bbbbbbb"), None), &dir).expect("second commit");
        record_run(
            &with_context(&report("live", 1000), Some("aaaaaaa"), None),
            &dir,
        )
        .expect("other mode");
        let history = load_history(&dir.join("history.jsonl")).expect("load");
        assert_eq!(history.len(), 3, "mode is part of the upsert key");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_commit_always_appends() {
        let dir = temp_dir("local-append");
        record_run(&report("replay", 1000), &dir).expect("first");
        record_run(&report("replay", 2000), &dir).expect("second");
        let text = std::fs::read_to_string(dir.join("history.jsonl")).expect("history text");
        assert_eq!(
            text.lines().count(),
            2,
            "local runs never participate in upsert"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn blank_lines_are_skipped_on_load() {
        let dir = temp_dir("blank-lines");
        write_lines(
            &dir,
            "\n{\"mode\":\"replay\",\"generated_at\":1000,\"classify\":null,\"tasks\":[],\"latency\":null,\"judge\":null,\"skipped\":0}\n\n",
        );
        let history = load_history(&dir.join("history.jsonl")).expect("load");
        assert_eq!(history.len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupt_lines_are_skipped_and_warned() {
        let dir = temp_dir("corrupt-lines");
        write_lines(
            &dir,
            concat!(
                "{\"mode\":\"replay\",\"generated_at\":1000,\"classify\":null,\"tasks\":[],\"latency\":null,\"judge\":null,\"skipped\":0}\n",
                "{broken json\n",
                "{\"mode\":\"live\",\"generated_at\":2000,\"classify\":null,\"tasks\":[],\"latency\":null,\"judge\":null,\"skipped\":0}\n",
            ),
        );
        let history = load_history(&dir.join("history.jsonl")).expect("load");
        assert_eq!(
            history
                .iter()
                .map(|report| report.mode.as_str())
                .collect::<Vec<_>>(),
            vec!["replay", "live"],
            "a bad line neither aborts the load nor takes good rows with it"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unterminated_final_line_is_readable() {
        let dir = temp_dir("tail-line");
        write_lines(
            &dir,
            "{\"mode\":\"replay\",\"generated_at\":1000,\"classify\":null,\"tasks\":[],\"latency\":null,\"judge\":null,\"skipped\":0}\n{\"mode\":\"live\",\"generated_at\":2000,\"classify\":null,\"tasks\":[],\"latency\":null,\"judge\":null,\"skipped\":0}",
        );
        let history = load_history(&dir.join("history.jsonl")).expect("load");
        assert_eq!(
            history.len(),
            2,
            "a complete line without the LF still parses"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn record_run_redraws_history_svg() {
        let dir = temp_dir("redraw");
        record_run(&report("replay", 1000), &dir).expect("record");
        let svg = std::fs::read_to_string(dir.join("history.svg")).expect("svg text");
        assert!(svg.starts_with("<svg"), "record_run renders the trend file");
        record_run(
            &with_context(&report("replay", 2000), Some("ec96217"), None),
            &dir,
        )
        .expect("second");
        let redrawn = std::fs::read_to_string(dir.join("history.svg")).expect("svg text");
        assert!(
            redrawn.contains("（2 点）"),
            "the redraw sees the full history"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn context_roundtrips_and_bare_rows_default_to_none() {
        let dir = temp_dir("context-roundtrip");
        let pipeline = with_context(&report("replay", 1000), Some("ec96217"), Some("pipeline"));
        record_run(&pipeline, &dir).expect("record");
        let line = std::fs::read_to_string(dir.join("history.jsonl")).expect("history text");
        assert!(
            line.contains(r#""context":{"commit":"ec96217","source":"pipeline"}"#),
            "the context segment rides the line tail: {line}"
        );
        let history = load_history(&dir.join("history.jsonl")).expect("load");
        assert_eq!(
            serde_json::to_string(&history[0]).expect("re-serialize"),
            line.trim_end(),
            "the row round-trips byte-identically"
        );

        let bare = r#"{"mode":"replay","generated_at":1000,"classify":null,"tasks":[],"latency":null,"judge":null,"skipped":0}"#;
        let parsed: EvalReport = serde_json::from_str(bare).expect("bare row parses");
        assert!(
            parsed.context.is_none(),
            "old rows without the segment default to none"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
