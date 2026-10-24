//! 评测报告：以（JSON）落盘、以（Markdown）渲染（表格形态沿用基准
//! 汇总脚本的模式：一指标一行，比率带百分比）。

use std::collections::BTreeMap;

use gloss_core::schema::Violation;
use gloss_core::task::TaskKind;
use serde::{Deserialize, Serialize};

use crate::metrics::{ClassifyMetrics, Latency, TaskMetrics, violation_tag};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalReport {
    pub mode: String,
    pub generated_at: i64,
    pub classify: Option<ClassifyStats>,
    pub tasks: Vec<TaskStats>,
    pub latency: Option<LatencySummary>,
    pub judge: Option<JudgeSummary>,
    pub skipped: usize,
    #[serde(default)]
    pub context: Option<EvalContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalContext {
    pub commit: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifyStats {
    pub metrics: ClassifyMetrics,
    pub accuracy: f64,
    pub fallback_rate: f64,
    pub confusion_rows: Vec<ConfusionRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfusionRow {
    pub expected: TaskKind,
    pub actual: TaskKind,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStats {
    pub dataset: String,
    pub metrics: TaskMetrics,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LatencySummary {
    pub count: usize,
    pub p50: f64,
    pub p95: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct JudgeSummary {
    pub count: usize,
    #[serde(default)]
    pub attempted: usize,
    pub average: f64,
}

impl EvalReport {
    pub fn render_markdown(&self) -> String {
        let mut out = String::new();
        self.render_summary(&mut out);
        self.render_metric_rows(&mut out);
        self.render_confusion(&mut out);
        out
    }

    fn render_summary(&self, out: &mut String) {
        line(
            out,
            format!(
                "Eval report: {} mode ({} UTC)",
                self.mode,
                format_utc(self.generated_at)
            ),
        );
        line(
            out,
            format!(
                "evaluated {} cases, skipped (no fixtures) {}",
                self.classify
                    .as_ref()
                    .map_or(0, |stats| stats.metrics.evaluated)
                    + self
                        .tasks
                        .iter()
                        .map(|stats| stats.metrics.evaluated)
                        .sum::<usize>(),
                self.skipped
            ),
        );
        out.push('\n');
    }

    fn render_metric_rows(&self, out: &mut String) {
        out.push_str("| Metric | Value |\n| --- | --- |\n");
        self.render_classify_rows(out);
        self.render_task_rows(out);
        self.render_latency_and_judge_rows(out);
    }

    fn render_classify_rows(&self, out: &mut String) {
        let Some(stats) = &self.classify else {
            return;
        };
        line(
            out,
            format!(
                "| classify accuracy | {:.2}% ({}/{}) |",
                stats.accuracy * 100.0,
                stats.metrics.correct,
                stats.metrics.evaluated
            ),
        );
        line(
            out,
            format!(
                "| classify invalid JSON rate | {:.2}% |",
                rate(stats.metrics.invalid_json, stats.metrics.evaluated)
            ),
        );
        line(
            out,
            format!(
                "| classify fallback rate | {:.2}% |",
                stats.fallback_rate * 100.0
            ),
        );
        line(
            out,
            format!(
                "| classify repair rounds | {} |",
                repair_line(&stats.metrics.violations, stats.metrics.repair_rounds)
            ),
        );
    }

    fn render_task_rows(&self, out: &mut String) {
        for stats in &self.tasks {
            let metrics = &stats.metrics;
            line(
                out,
                format!(
                    "| {} contract JSON rate | {:.2}% |",
                    stats.dataset,
                    rate(metrics.json_object, metrics.evaluated)
                ),
            );
            line(
                out,
                format!(
                    "| {} note presence rate | {:.2}% |",
                    stats.dataset,
                    rate(metrics.note_present, metrics.evaluated)
                ),
            );
            line(
                out,
                format!(
                    "| {} fields-complete rate | {:.2}% |",
                    stats.dataset,
                    rate(metrics.fields_complete, metrics.evaluated)
                ),
            );
            line(
                out,
                format!(
                    "| {} degraded rate | {:.2}% |",
                    stats.dataset,
                    rate(metrics.degraded, metrics.evaluated)
                ),
            );
            line(
                out,
                format!(
                    "| {} repair rounds | {} |",
                    stats.dataset,
                    repair_line(&metrics.violations, metrics.repair_rounds)
                ),
            );
        }
    }

    fn render_latency_and_judge_rows(&self, out: &mut String) {
        if let Some(latency) = &self.latency {
            line(
                out,
                format!("| end-to-end latency p50 | {:.2} ms |", latency.p50),
            );
            line(
                out,
                format!("| end-to-end latency p95 | {:.2} ms |", latency.p95),
            );
        }
        if let Some(judge) = &self.judge {
            let detail = if judge.attempted > judge.count {
                format!(
                    "{:.2} ({}/{} counted, {} failed)",
                    judge.average,
                    judge.count,
                    judge.attempted,
                    judge.attempted - judge.count
                )
            } else {
                format!("{:.2} ({} counted)", judge.average, judge.count)
            };
            line(out, format!("| judge average (1..=5) | {detail} |"));
        }
    }

    fn render_confusion(&self, out: &mut String) {
        let Some(stats) = &self.classify else {
            return;
        };
        if stats.confusion_rows.is_empty() {
            return;
        }
        out.push('\n');
        out.push_str("Confusion matrix (expected → actual, errors only):\n\n");
        out.push_str("| Expected | Actual | Count |\n| --- | --- | --- |\n");
        for row in &stats.confusion_rows {
            line(
                out,
                format!("| {:?} | {:?} | {} |", row.expected, row.actual, row.count),
            );
        }
    }
}

fn line(out: &mut String, content: String) {
    out.push_str(&content);
    out.push('\n');
}

fn rate(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64 * 100.0
    }
}

pub fn format_utc(secs: i64) -> String {
    let (date, hour, minute) = utc_parts(secs);
    format!("{date} {hour:02}:{minute:02}")
}

pub fn format_utc_short(secs: i64) -> String {
    let (date, hour, minute) = utc_parts(secs);
    format!("{} {hour:02}:{minute:02}", &date[5..])
}

fn utc_parts(secs: i64) -> (String, u32, u32) {
    let (year, month, day) = civil_from_days(secs.div_euclid(86_400));
    let day_secs = secs.rem_euclid(86_400);
    (
        format!("{year:04}-{month:02}-{day:02}"),
        (day_secs / 3_600) as u32,
        ((day_secs % 3_600) / 60) as u32,
    )
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = (shifted - era * 146_097) as u64;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

fn repair_line(violations: &BTreeMap<Violation, usize>, repairs: u32) -> String {
    if violations.is_empty() {
        return format!("{repairs}");
    }
    let counts = violations
        .iter()
        .map(|(violation, count)| format!("{} ×{count}", violation_tag(violation)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{repairs} ({counts})")
}

pub struct TrackOutputs<'a> {
    pub classify: Option<ClassifyMetrics>,
    pub tasks: Vec<(&'a str, TaskMetrics)>,
    pub latency: Option<&'a mut Latency>,
    pub judge: Option<JudgeSummary>,
}

pub fn assemble_report(mode: &str, outputs: TrackOutputs<'_>, skipped: usize) -> EvalReport {
    let TrackOutputs {
        classify,
        tasks,
        latency,
        judge,
    } = outputs;
    let classify_stats = classify.map(|metrics| {
        let mut confusion_rows = Vec::new();
        for (expected, actuals) in &metrics.confusion {
            for (actual, count) in actuals {
                confusion_rows.push(ConfusionRow {
                    expected: *expected,
                    actual: *actual,
                    count: *count,
                });
            }
        }
        ClassifyStats {
            accuracy: metrics.accuracy(),
            fallback_rate: metrics.fallback_rate(),
            confusion_rows,
            metrics,
        }
    });
    let latency_summary = latency.and_then(|latency| {
        Some(LatencySummary {
            count: latency.count(),
            p50: latency.percentile(50.0)?,
            p95: latency.percentile(95.0)?,
        })
    });
    EvalReport {
        mode: mode.to_owned(),
        generated_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or_default(),
        classify: classify_stats,
        tasks: tasks
            .into_iter()
            .map(|(dataset, metrics)| TaskStats {
                dataset: dataset.to_owned(),
                metrics,
            })
            .collect(),
        latency: latency_summary,
        judge,
        skipped,
        context: None,
    }
}

pub fn write_report(report: &EvalReport, report_dir: &std::path::Path) -> std::io::Result<String> {
    std::fs::create_dir_all(report_dir)?;
    let markdown = report.render_markdown();
    serde_json::to_writer_pretty(
        std::io::BufWriter::new(std::fs::File::create(
            report_dir.join(format!("{}.json", report.mode)),
        )?),
        report,
    )
    .map_err(std::io::Error::other)?;
    std::fs::write(report_dir.join(format!("{}.md", report.mode)), &markdown)?;
    Ok(markdown)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use gloss_core::schema::Violation;

    use super::{EvalReport, JudgeSummary, TrackOutputs, assemble_report};
    use crate::dataset::TaskCase;
    use crate::metrics::{ClassifyMetrics, TaskMetrics, TaskVerdict};
    use gloss_core::task::TaskKind;

    #[test]
    fn report_renders_the_table_shape() {
        let report = fixture_report();
        let markdown = report.render_markdown();
        assert!(markdown.contains("| Metric | Value |"));
        assert!(markdown.contains("classify accuracy | 66.67% (2/3)"));
        assert!(markdown.contains("classify fallback rate | 33.33%"));
        assert!(markdown.contains("Confusion matrix"));
        assert!(markdown.contains("| TranslateWord | ExplainCode | 1 |"));
        assert!(markdown.contains("task_translate_word fields-complete rate | 50.00%"));
        assert!(markdown.contains("task_translate_word degraded rate | 50.00%"));
        assert!(markdown.contains("skipped (no fixtures) 1"));
    }

    #[test]
    fn judge_summary_reports_failures_when_attempted_exceeds_count() {
        let mut report = fixture_report();
        report.judge = Some(JudgeSummary {
            count: 0,
            attempted: 3,
            average: 0.0,
        });
        let markdown = report.render_markdown();
        assert!(
            markdown.contains("judge average (1..=5) | 0.00 (0/3 counted, 3 failed)"),
            "a fully failed judge run must be distinguishable from a disabled one: {markdown}"
        );

        report.judge = Some(JudgeSummary {
            count: 2,
            attempted: 2,
            average: 4.1,
        });
        let markdown = report.render_markdown();
        assert!(
            markdown.contains("judge average (1..=5) | 4.10 (2 counted)"),
            "a clean judge run reports the count without failure noise: {markdown}"
        );
    }

    #[test]
    fn generated_at_is_a_numeric_unix_timestamp() {
        let mut report = fixture_report();
        report.generated_at = 1_791_763_200;
        let json = serde_json::to_string(&report).expect("serialize");
        assert!(
            json.contains(r#""generated_at":1791763200"#),
            "the timestamp serializes as a bare number: {json}"
        );
        let parsed: EvalReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.generated_at, 1_791_763_200);
    }

    #[test]
    fn utc_formatting_is_fixed_and_timezone_free() {
        assert_eq!(super::format_utc(0), "1970-01-01 00:00");
        assert_eq!(super::format_utc(1_791_763_200), "2026-10-12 00:00");
        assert_eq!(
            super::format_utc(1_791_763_200 + 86_399),
            "2026-10-12 23:59"
        );
        assert_eq!(super::format_utc(-1), "1969-12-31 23:59");
        assert_eq!(super::format_utc_short(-86_400), "12-31 00:00");
        assert_eq!(super::format_utc_short(1_000), "01-01 00:16");
    }

    #[test]
    fn repair_rounds_render_with_violation_categories() {
        let report = fixture_report();
        let markdown = report.render_markdown();
        assert!(
            markdown.contains("classify repair rounds | 1 (not-json ×1) |"),
            "the classify repair line carries the total and per-category counts: {markdown}"
        );
        assert!(
            markdown.contains("task_translate_word repair rounds | 1 (not-json ×1) |"),
            "the task repair line rides the same shape: {markdown}"
        );

        let clean = ClassifyMetrics {
            evaluated: 1,
            correct: 1,
            ..ClassifyMetrics::default()
        };
        let verdict = TaskVerdict::for_reply(
            &word_cases()[0],
            r#"{"note":"义","structured":{"examples":[]}}"#,
        );
        let mut metrics = TaskMetrics::default();
        metrics.record(&verdict);
        let report = assemble_report(
            "replay",
            TrackOutputs {
                classify: Some(clean),
                tasks: vec![("task_translate_word", metrics)],
                latency: None,
                judge: None,
            },
            0,
        );
        let markdown = report.render_markdown();
        assert!(
            markdown.contains("classify repair rounds | 0 |")
                && markdown.contains("task_translate_word repair rounds | 0 |"),
            "a track without violations reports bare round counts: {markdown}"
        );
    }

    #[test]
    fn latency_only_shows_when_sampled() {
        let report = fixture_report();
        assert!(
            !report.render_markdown().contains("latency"),
            "replay has no latency samples"
        );
    }

    fn word_cases() -> Vec<TaskCase> {
        let word_jsonl = concat!(
            "{\"id\":\"w1\",\"kind\":\"TranslateWord\",\"text\":\"gloss\",\"reference\":{\"phonetic\":null,\"examples\":[]}}\n",
            "{\"id\":\"w2\",\"kind\":\"TranslateWord\",\"text\":\"idempotent\",\"reference\":{\"phonetic\":null,\"examples\":[]}}\n",
        );
        crate::dataset::load_task(word_jsonl).expect("cases")
    }

    fn fixture_report() -> EvalReport {
        let cases = word_cases();
        let confusion = BTreeMap::from([(
            TaskKind::TranslateWord,
            BTreeMap::from([(TaskKind::ExplainCode, 1)]),
        )]);
        let mut violations = BTreeMap::new();
        violations.insert(Violation::NotJson, 1);
        let classify = ClassifyMetrics {
            evaluated: 3,
            correct: 2,
            invalid_json: 1,
            confusion,
            rejected: 0,
            engine_errors: 0,
            violations: violations.clone(),
            repair_rounds: 1,
        };
        let verdicts = [
            TaskVerdict::for_rounds(
                &cases[0],
                &[
                    "整段散文，不是 JSON".to_owned(),
                    r#"{"note":"正文","structured":{"phonetic":null,"examples":[]}}"#.to_owned(),
                ],
            ),
            TaskVerdict::for_reply(&cases[1], "只有正文"),
        ];
        let mut task_metrics = TaskMetrics::default();
        for verdict in &verdicts {
            task_metrics.record(verdict);
        }
        assemble_report(
            "replay",
            TrackOutputs {
                classify: Some(classify),
                tasks: vec![("task_translate_word", task_metrics)],
                latency: None,
                judge: None,
            },
            1,
        )
    }
}
