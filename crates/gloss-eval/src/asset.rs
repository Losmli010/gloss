//! 运行时资产存储仓：标注（labels.jsonl）、基线（golden.jsonl）与单次
//! 输出（iterate.outputs.jsonl）三类评测资产的唯一碰盘点。一行一份
//! （JSON）、换行符（LF）结尾；文件缺失按空表处理（首轮无历史是常态），
//! 坏行与未知道具拒绝——评测资产是受控工件，静默跳过会污染回归判定；
//! 全部落盘一律原子写，合并按条目以标识为准（后写覆盖先写）。

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::detail::LabelVerdict;
use crate::history::write_atomic;

pub type LabelMap = BTreeMap<String, LabelRow>;

pub type GoldenMap = BTreeMap<String, GoldenRow>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelRow {
    pub id: String,
    pub verdict: LabelVerdict,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoldenRow {
    pub id: String,
    pub rounds: Vec<Vec<String>>,
    pub accepted_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputRow {
    pub id: String,
    pub rounds: Vec<Vec<String>>,
}

pub fn load_labels(path: &Path) -> Result<LabelMap, String> {
    let rows: Vec<LabelRow> = load_rows(path)?;
    Ok(rows.into_iter().map(|row| (row.id.clone(), row)).collect())
}

pub fn load_golden(path: &Path) -> Result<GoldenMap, String> {
    let rows: Vec<GoldenRow> = load_rows(path)?;
    Ok(rows.into_iter().map(|row| (row.id.clone(), row)).collect())
}

pub fn upsert_labels(path: &Path, rows: &[LabelRow]) -> Result<(), String> {
    let mut merged = load_labels(path)?;
    for row in rows {
        merged.insert(row.id.clone(), row.clone());
    }
    write_rows(path, merged.into_values().collect())
}

pub fn upsert_golden(path: &Path, rows: &[GoldenRow]) -> Result<(), String> {
    let mut merged = load_golden(path)?;
    for row in rows {
        merged.insert(row.id.clone(), row.clone());
    }
    write_rows(path, merged.into_values().collect())
}

pub fn read_outputs(path: &Path) -> Result<Vec<OutputRow>, String> {
    let text = read_required(path)?;
    parse_lines(&text).map_err(|error| format!("{}: {error}", path.display()))
}

pub fn write_outputs(path: &Path, rows: &[OutputRow]) -> Result<(), String> {
    write_rows(path, rows.to_vec())
}

fn load_rows<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Vec<T>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    parse_lines(&text).map_err(|error| format!("{}: {error}", path.display()))
}

fn read_required(path: &Path) -> Result<String, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(format!(
            "{}: no such file — run eval iterate first to produce outputs",
            path.display()
        )),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

fn parse_lines<T: for<'de> Deserialize<'de>>(text: &str) -> Result<Vec<T>, String> {
    let mut rows = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let row: T = serde_json::from_str(trimmed)
            .map_err(|error| format!("line {}: {error}", index + 1))?;
        rows.push(row);
    }
    Ok(rows)
}

fn write_rows<T: Serialize>(path: &Path, rows: Vec<T>) -> Result<(), String> {
    let mut body = String::new();
    for row in &rows {
        let line = serde_json::to_string(row).map_err(|error| error.to_string())?;
        body.push_str(&line);
        body.push('\n');
    }
    write_atomic(path, &body).map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        GoldenRow, LabelRow, OutputRow, load_golden, load_labels, read_outputs, upsert_golden,
        upsert_labels, write_outputs,
    };
    use crate::detail::LabelVerdict;

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "gloss-eval-asset-{tag}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn label(id: &str, verdict: LabelVerdict, note: Option<&str>) -> LabelRow {
        LabelRow {
            id: id.to_owned(),
            verdict,
            note: note.map(str::to_owned),
        }
    }

    fn golden(id: &str, accepted_at: &str) -> GoldenRow {
        GoldenRow {
            id: id.to_owned(),
            rounds: vec![vec![r#"{"kind":"TranslateWord"}"#.to_owned()]],
            accepted_at: accepted_at.to_owned(),
        }
    }

    #[test]
    fn missing_files_load_as_empty_maps() {
        let dir = temp_dir("missing-empty");
        let labels = load_labels(&dir.join("labels.jsonl")).expect("missing labels load");
        assert!(labels.is_empty());
        let golden = load_golden(&dir.join("golden.jsonl")).expect("missing golden loads");
        assert!(golden.is_empty());
    }

    #[test]
    fn upserts_merge_by_id_and_keep_lf_and_note_roundtrip() {
        let dir = temp_dir("upsert-merge");
        let path = dir.join("labels.jsonl");
        upsert_labels(
            &path,
            &[label("cls-word-01", LabelVerdict::Fail, Some("错译"))],
        )
        .expect("first upsert");
        upsert_labels(
            &path,
            &[
                label("cls-word-01", LabelVerdict::Pass, None),
                label("task-word-01", LabelVerdict::Fail, Some("例句少")),
            ],
        )
        .expect("second upsert");
        let text = std::fs::read_to_string(&path).expect("labels text");
        assert!(text.ends_with('\n'), "rows end with LF");
        assert_eq!(text.lines().count(), 2, "same id overwrites, not appends");
        assert!(
            text.starts_with(r#"{"id":"cls-word-01""#),
            "rows are sorted by id for deterministic files: {text}"
        );
        let labels = load_labels(&path).expect("reload");
        assert_eq!(labels["cls-word-01"].verdict, LabelVerdict::Pass);
        assert_eq!(labels["cls-word-01"].note, None);
        assert_eq!(
            labels["task-word-01"].note.as_deref(),
            Some("例句少"),
            "notes ride the roundtrip"
        );

        let path = dir.join("golden.jsonl");
        upsert_golden(&path, &[golden("task-word-01", "2026-11-01T10:00:00Z")])
            .expect("golden upsert");
        upsert_golden(&path, &[golden("task-word-01", "2026-11-02T10:00:00Z")])
            .expect("golden re-accept");
        let golden = load_golden(&path).expect("reload golden");
        assert_eq!(golden.len(), 1);
        assert_eq!(golden["task-word-01"].accepted_at, "2026-11-02T10:00:00Z");
    }

    #[test]
    fn bad_rows_and_unknown_verdicts_are_rejected_with_line_numbers() {
        let dir = temp_dir("bad-rows");
        let path = dir.join("labels.jsonl");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(&path, "{\"id\":\"a\",\"verdict\":\"pass\"}\nnot-json\n").expect("fixture");
        let error = load_labels(&path).expect_err("bad rows are hard errors");
        assert!(error.contains("line 2"), "{error}");

        std::fs::write(&path, "{\"id\":\"a\",\"verdict\":\"maybe\"}\n").expect("fixture");
        let error = load_labels(&path).expect_err("unknown verdicts are rejected");
        assert!(error.contains("line 1"), "{error}");

        let golden_path = dir.join("golden.jsonl");
        std::fs::write(&golden_path, "{\"id\":\"a\"}\n").expect("fixture");
        let error = load_golden(&golden_path).expect_err("golden rows carry rounds");
        assert!(error.contains("line 1"), "{error}");
    }

    #[test]
    fn outputs_overwrite_on_write_and_fail_clear_on_missing_read() {
        let dir = temp_dir("outputs");
        let path = dir.join("iterate.outputs.jsonl");
        let row = OutputRow {
            id: "task-word-01".to_owned(),
            rounds: vec![vec![r#"{"kind":"TranslateWord"}"#.to_owned()]],
        };
        write_outputs(&path, std::slice::from_ref(&row)).expect("write");
        let outputs = read_outputs(&path).expect("read");
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].id, "task-word-01");
        assert_eq!(outputs[0].rounds, row.rounds);

        write_outputs(&path, &[]).expect("overwrite clears");
        assert!(
            read_outputs(&path)
                .expect("an existing empty file reads as zero rows")
                .is_empty()
        );

        let error = read_outputs(&dir.join("absent.jsonl"))
            .expect_err("a missing outputs file is a hard error");
        assert!(
            error.contains("run eval iterate first"),
            "the missing-input error names the remedy: {error}"
        );
    }
}
