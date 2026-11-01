//! 流程装配：迭代实跑、标注落库与基线固化三条流的编排——执行、回归
//! 结算、聚合、产物落盘、检视页渲染与历史记录按序串接；环境变量与端
//! 点装配由入口注入，本模块不读环境、不打印。文件读写只经报告、资产
//! 与历史三个收口点；单次输出仅成功用例落行，契约故障无输出可固化。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use gloss_core::config_handle::ConfigHandle;
use gloss_core::ports::AiEngine;
use serde::Deserialize;

use crate::asset::{self, GoldenRow, LabelRow, OutputRow};
use crate::dataset::{EvalCase, load_curated};
use crate::detail::CaseDetail;
use crate::execute::{self, ExecuteOptions};
use crate::history;
use crate::regression;
use crate::report::{self, EvalContext, EvalReport};
use crate::view;

pub const LABELS_FILE: &str = "labels.jsonl";
pub const GOLDEN_FILE: &str = "golden.jsonl";
pub const OUTPUTS_FILE: &str = "iterate.outputs.jsonl";
pub const CASES_FILE: &str = "iterate.cases.json";
pub const VIEW_FILE: &str = "iterate.html";

#[derive(Deserialize)]
struct LabelsDocument {
    labels: Vec<LabelRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportSummary {
    pub imported: usize,
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcceptSummary {
    pub accepted: usize,
    pub unlabeled: usize,
}

pub async fn run_iterate(
    engine: &dyn AiEngine,
    config: &ConfigHandle,
    options: ExecuteOptions,
    dir: &Path,
    context: EvalContext,
) -> Result<EvalReport, String> {
    let curated = load_curated()?;
    let mut run = execute::run(engine, config, options).await?;
    let golden_path = dir.join(GOLDEN_FILE);
    let labels_path = dir.join(LABELS_FILE);
    let golden = asset::load_golden(&golden_path)?;
    let labels = asset::load_labels(&labels_path)?;
    regression::settle(&mut run.cases, &golden, &labels);
    let latency = latency_summary(&mut run.latency);
    let mut eval_report = report::assemble(&run.cases, &golden, &labels, latency);
    eval_report.context = Some(context);
    write_records(&eval_report, &run.cases, dir)?;
    let history_rows = record_history(&eval_report, dir)?;
    let pairs = case_pairs(&curated, &run.cases)?;
    write_view(
        &view::ViewInput {
            report: &eval_report,
            cases: &pairs,
            history: &history_rows,
            labels: &labels,
            golden: &golden,
        },
        dir,
    )?;
    Ok(eval_report)
}

pub fn import_labels(dir: &Path, file: &Path) -> Result<ImportSummary, String> {
    let text =
        std::fs::read_to_string(file).map_err(|error| format!("{}: {error}", file.display()))?;
    let document: LabelsDocument =
        serde_json::from_str(&text).map_err(|error| format!("{}: {error}", file.display()))?;
    let curated = load_curated()?;
    let known: BTreeSet<&str> = curated.iter().map(|case| case.id.as_str()).collect();
    let mut rows = Vec::new();
    let mut skipped = Vec::new();
    for row in document.labels {
        if known.contains(row.id.as_str()) {
            rows.push(row);
        } else {
            skipped.push(row.id);
        }
    }
    asset::upsert_labels(&dir.join(LABELS_FILE), &rows)?;
    Ok(ImportSummary {
        imported: rows.len(),
        skipped,
    })
}

pub fn accept(dir: &Path) -> Result<AcceptSummary, String> {
    let outputs = asset::read_outputs(&dir.join(OUTPUTS_FILE))?;
    let labels = asset::load_labels(&dir.join(LABELS_FILE))?;
    let accepted_at = report::format_utc_rfc3339(report::now_secs());
    let rows: Vec<GoldenRow> = outputs
        .into_iter()
        .map(|row| GoldenRow {
            id: row.id,
            rounds: row.rounds,
            accepted_at: accepted_at.clone(),
        })
        .collect();
    let unlabeled = rows
        .iter()
        .filter(|row| !labels.contains_key(&row.id))
        .count();
    asset::upsert_golden(&dir.join(GOLDEN_FILE), &rows)?;
    Ok(AcceptSummary {
        accepted: rows.len(),
        unlabeled,
    })
}

fn latency_summary(latency: &mut execute::Latency) -> Option<report::LatencySummary> {
    let count = latency.count();
    if count == 0 {
        return None;
    }
    Some(report::LatencySummary {
        count,
        p50_ms: latency.percentile(50.0).unwrap_or_default(),
        p95_ms: latency.percentile(95.0).unwrap_or_default(),
    })
}

fn write_records(eval_report: &EvalReport, cases: &[CaseDetail], dir: &Path) -> Result<(), String> {
    report::write(eval_report, dir)
        .map_err(|error| format!("write {}: {error}", report_path(dir).display()))?;
    let cases_json = serde_json::to_string_pretty(cases).map_err(|error| error.to_string())?;
    history::write_atomic(&dir.join(CASES_FILE), &cases_json)
        .map_err(|error| format!("write {}: {error}", dir.join(CASES_FILE).display()))?;
    let outputs: Vec<OutputRow> = cases
        .iter()
        .filter(|detail| !detail.engine_error)
        .map(|detail| OutputRow {
            id: detail.id.clone(),
            rounds: detail
                .rounds
                .iter()
                .map(|round| vec![round.text.clone()])
                .collect(),
        })
        .collect();
    asset::write_outputs(&dir.join(OUTPUTS_FILE), &outputs)
}

fn record_history(eval_report: &EvalReport, dir: &Path) -> Result<Vec<EvalReport>, String> {
    history::record_run(eval_report, dir).map_err(|error| format!("record history: {error}"))?;
    let history_path = dir.join(history::HISTORY_FILE);
    history::load_history(&history_path)
        .map_err(|error| format!("{}: {error}", history_path.display()))
}

fn write_view(input: &view::ViewInput<'_>, dir: &Path) -> Result<(), String> {
    let html = view::render_html(input);
    history::write_atomic(&dir.join(VIEW_FILE), &html)
        .map_err(|error| format!("write {}: {error}", dir.join(VIEW_FILE).display()))
}

fn case_pairs<'a>(
    curated: &'a [EvalCase],
    cases: &'a [CaseDetail],
) -> Result<Vec<view::CasePair<'a>>, String> {
    cases
        .iter()
        .map(|detail| {
            let case = curated
                .iter()
                .find(|case| case.id == detail.id)
                .ok_or_else(|| {
                    format!(
                        "case {} settled but missing from the curated set",
                        detail.id
                    )
                })?;
            Ok(view::CasePair { case, detail })
        })
        .collect()
}

fn report_path(dir: &Path) -> PathBuf {
    dir.join(format!("{}.json", report::ITERATE_MODE))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use futures::stream;

    use gloss_core::config::Config;
    use gloss_core::config_handle::ConfigHandle;
    use gloss_core::model::GlossError;
    use gloss_core::ports::{
        AiEngine, BoxFuture, ConfigStore, EngineRequest, TaskChunk, TaskStream,
    };

    use super::{AcceptSummary, ImportSummary, accept, import_labels, run_iterate};
    use crate::asset::{self, LabelRow};
    use crate::detail::{CaseDetail, LabelVerdict};
    use crate::execute::ExecuteOptions;
    use crate::report::EvalReport;

    const CLASSIFY_REPLY: &str = r#"{"kind":"TranslateWord"}"#;

    type Script = Vec<Result<TaskChunk, GlossError>>;

    struct ScriptEngine {
        script: std::sync::Mutex<Script>,
    }

    impl AiEngine for ScriptEngine {
        fn execute(
            &self,
            _request: EngineRequest,
        ) -> BoxFuture<'static, Result<TaskStream, GlossError>> {
            let script = self
                .script
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone();
            Box::pin(async move { Ok(Box::pin(stream::iter(script)) as TaskStream) })
        }
    }

    fn engine(script: Script) -> Arc<ScriptEngine> {
        Arc::new(ScriptEngine {
            script: std::sync::Mutex::new(script),
        })
    }

    fn reply_script(text: &str) -> Script {
        vec![
            Ok(TaskChunk::Delta(text.to_owned())),
            Ok(TaskChunk::Done {
                finish_reason: None,
            }),
        ]
    }

    fn config() -> Arc<ConfigHandle> {
        struct FixedStore;
        impl ConfigStore for FixedStore {
            fn load(&self) -> Result<Config, GlossError> {
                Ok(Config::default())
            }

            fn save(&self, _config: &Config) -> Result<(), GlossError> {
                Ok(())
            }
        }
        Arc::new(ConfigHandle::with_config(
            Arc::new(FixedStore),
            Config::default(),
        ))
    }

    fn options(limit: usize) -> ExecuteOptions {
        ExecuteOptions {
            judge: false,
            limit: Some(limit),
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "gloss-eval-flow-{tag}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn label(id: &str, verdict: LabelVerdict) -> LabelRow {
        LabelRow {
            id: id.to_owned(),
            verdict,
            note: None,
        }
    }

    #[tokio::test]
    async fn stub_iterate_writes_artifacts_records_history_and_renders_the_view() {
        let dir = temp_dir("iterate-artifacts");
        let report = run_iterate(
            engine(reply_script(CLASSIFY_REPLY)).as_ref(),
            &config(),
            options(1),
            &dir,
            super::EvalContext {
                commit: Some("c0ffee".to_owned()),
                source: None,
            },
        )
        .await
        .expect("the stub iterate settles");
        assert_eq!(report.mode, "iterate");
        assert_eq!(report.totals.cases, 1);
        assert_eq!(
            report.context.as_ref().and_then(|c| c.commit.as_deref()),
            Some("c0ffee")
        );

        let stored: EvalReport = serde_json::from_str(
            &std::fs::read_to_string(dir.join("iterate.json")).expect("report"),
        )
        .expect("the report file parses");
        assert_eq!(stored.mode, "iterate");

        let cases: Vec<CaseDetail> = serde_json::from_str(
            &std::fs::read_to_string(dir.join("iterate.cases.json")).expect("cases"),
        )
        .expect("the cases file parses");
        assert_eq!(cases.len(), 1);
        assert_eq!(cases[0].id, "cls-word-01");

        let outputs = asset::read_outputs(&dir.join(super::OUTPUTS_FILE)).expect("outputs");
        assert_eq!(outputs.len(), 1, "a healthy case lands in the outputs file");
        assert_eq!(outputs[0].rounds, vec![vec![CLASSIFY_REPLY.to_owned()]]);

        assert!(
            dir.join("history.svg").exists(),
            "history redraw rides the run"
        );
        let page = std::fs::read_to_string(dir.join(super::VIEW_FILE)).expect("view");
        assert!(
            page.contains("cls-word-01"),
            "the run case shows on the page"
        );

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[tokio::test]
    async fn engine_failures_settle_but_never_reach_the_outputs_file() {
        let dir = temp_dir("iterate-engine-failure");
        let report = run_iterate(
            engine(vec![Err(GlossError::EngineNetwork)]).as_ref(),
            &config(),
            options(1),
            &dir,
            super::EvalContext {
                commit: None,
                source: None,
            },
        )
        .await
        .expect("a failed request does not abort the run");
        assert!(report.totals.contract.engine_error == 1);
        let cases: Vec<CaseDetail> = serde_json::from_str(
            &std::fs::read_to_string(dir.join("iterate.cases.json")).expect("cases"),
        )
        .expect("the cases file parses");
        assert!(cases[0].engine_error);
        assert!(
            asset::read_outputs(&dir.join(super::OUTPUTS_FILE))
                .expect("the outputs file still exists")
                .is_empty(),
            "a failed request must not be freezable"
        );

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn import_merges_known_rows_and_rejects_bad_documents() {
        let dir = temp_dir("import");
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("labels.json");
        std::fs::write(
            &file,
            r#"{"labels":[{"id":"cls-word-01","verdict":"pass","note":"ok"},{"id":"nope","verdict":"fail"}]}"#,
        )
        .expect("fixture");
        let summary = import_labels(&dir, &file).expect("the import merges");
        assert_eq!(
            summary,
            ImportSummary {
                imported: 1,
                skipped: vec!["nope".to_owned()]
            }
        );
        let labels = asset::load_labels(&dir.join(super::LABELS_FILE)).expect("labels");
        assert_eq!(labels.len(), 1);
        assert_eq!(labels["cls-word-01"].verdict, LabelVerdict::Pass);

        std::fs::write(&file, "not-json").expect("fixture");
        let error = import_labels(&dir, &file).expect_err("bad documents are hard errors");
        assert!(error.contains("labels.json"), "{error}");

        std::fs::write(
            &file,
            r#"{"labels":[{"id":"cls-word-01","verdict":"maybe"}]}"#,
        )
        .expect("fixture");
        let error = import_labels(&dir, &file).expect_err("unknown verdicts are rejected");
        assert!(error.contains("unknown variant"), "{error}");

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn accept_freezes_outputs_counts_unlabeled_and_fails_clear_without_inputs() {
        let dir = temp_dir("accept");
        std::fs::create_dir_all(&dir).expect("dir");
        let outputs = &dir.join(super::OUTPUTS_FILE);
        let row = |id: &str| crate::asset::OutputRow {
            id: id.to_owned(),
            rounds: vec![vec![CLASSIFY_REPLY.to_owned()]],
        };
        asset::write_outputs(outputs, &[row("cls-word-01"), row("task-word-01")])
            .expect("fixture outputs");
        asset::upsert_labels(
            &dir.join(super::LABELS_FILE),
            &[label("cls-word-01", LabelVerdict::Pass)],
        )
        .expect("fixture labels");

        let summary = accept(&dir).expect("the accept freezes");
        assert_eq!(
            summary,
            AcceptSummary {
                accepted: 2,
                unlabeled: 1
            }
        );
        let golden = asset::load_golden(&dir.join(super::GOLDEN_FILE)).expect("golden");
        assert_eq!(golden.len(), 2);
        assert!(
            golden["cls-word-01"].accepted_at.ends_with('Z'),
            "accepted_at is a UTC stamp: {:?}",
            golden["cls-word-01"].accepted_at
        );
        let merged = accept(&dir).expect("a re-accept upserts in place");
        assert_eq!(merged.accepted, 2, "upsert keeps one row per id");
        assert_eq!(
            asset::load_golden(&dir.join(super::GOLDEN_FILE))
                .expect("golden")
                .len(),
            2
        );

        let error = accept(&temp_dir("accept-missing")).expect_err("missing inputs fail clear");
        assert!(error.contains("run eval iterate first"), "{error}");

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
