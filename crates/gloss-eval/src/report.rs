//! 评测报告：JSON 落盘 + Markdown 渲染（表格形态沿用 scripts/
//! bench-summary.py 的模式：一指标一行，比率带百分比）。

use gloss_core::task::TaskKind;
use serde::{Deserialize, Serialize};

use crate::metrics::{ClassifyMetrics, Latency, TaskMetrics};

/// 一次评测运行的完整报告（`target/eval/<mode>.json` 的形状）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalReport {
    /// 运行模式（replay / live）。
    pub mode: String,
    /// 报告生成时间（本地时区，人读用）。
    pub generated_at: String,
    /// 分类轨指标（replay 只统计有夹具的条目）。
    pub classify: Option<ClassifyStats>,
    /// 任务轨指标：数据集名 → 指标。
    pub tasks: Vec<TaskStats>,
    /// 延迟统计（仅 live 轨有样本；replay 恒 None）。
    pub latency: Option<LatencySummary>,
    /// judge 摘要（仅 live 轨且 `GLOSS_LIVE_JUDGE=1` 时有值）。
    pub judge: Option<JudgeSummary>,
    /// 数据集条目中无夹具覆盖、被跳过的数量。
    pub skipped: usize,
}

/// 分类轨的序列化视图（混淆矩阵拍平成行）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifyStats {
    /// 各指标数值。
    pub metrics: ClassifyMetrics,
    /// 准确率（0..1）。
    pub accuracy: f64,
    /// 回退率（0..1）。
    pub fallback_rate: f64,
    /// 混淆矩阵行：期望 → 实际 → 次数（仅计错误路径）。
    pub confusion_rows: Vec<ConfusionRow>,
}

/// 混淆矩阵一行。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfusionRow {
    /// 期望 kind。
    pub expected: TaskKind,
    /// 实际 kind。
    pub actual: TaskKind,
    /// 次数。
    pub count: usize,
}

/// 单个任务数据集的指标视图。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStats {
    /// 数据集名（文件名去扩展名）。
    pub dataset: String,
    /// 指标数值。
    pub metrics: TaskMetrics,
}

/// 延迟摘要（毫秒）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LatencySummary {
    /// 样本数。
    pub count: usize,
    /// p50。
    pub p50: f64,
    /// p95。
    pub p95: f64,
}

/// judge 摘要（1–5 分的逐条均值；解析不出分数的条目不计入）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct JudgeSummary {
    /// 计分的条数。
    pub count: usize,
    /// 均分（1..=5）。
    pub average: f64,
}

impl EvalReport {
    /// 渲染 Markdown 表格（结构沿用 bench-summary.py：总述行 + 指标表 +
    /// 混淆矩阵表）。
    pub fn render_markdown(&self) -> String {
        fn line(out: &mut String, content: String) {
            out.push_str(&content);
            out.push('\n');
        }
        let mut out = String::new();
        line(
            &mut out,
            format!("Eval 报告：{} 模式（{}）", self.mode, self.generated_at),
        );
        line(
            &mut out,
            format!(
                "共评测 {} 条，跳过（无夹具）{} 条",
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
        out.push_str("| 指标 | 值 |\n| --- | --- |\n");
        if let Some(stats) = &self.classify {
            line(
                &mut out,
                format!(
                    "| 分类 accuracy | {:.1}% ({}/{}) |",
                    stats.accuracy * 100.0,
                    stats.metrics.correct,
                    stats.metrics.evaluated
                ),
            );
            line(
                &mut out,
                format!(
                    "| 分类 无效 JSON 率 | {:.1}% |",
                    rate(stats.metrics.invalid_json, stats.metrics.evaluated)
                ),
            );
            line(
                &mut out,
                format!("| 分类 回退率 | {:.1}% |", stats.fallback_rate * 100.0),
            );
        }
        for stats in &self.tasks {
            let metrics = &stats.metrics;
            line(
                &mut out,
                format!(
                    "| {} 围栏在场率 | {:.1}% |",
                    stats.dataset,
                    rate(metrics.fence_present, metrics.evaluated)
                ),
            );
            line(
                &mut out,
                format!(
                    "| {} JSON 可解析率 | {:.1}% |",
                    stats.dataset,
                    rate(metrics.json_parseable, metrics.evaluated)
                ),
            );
            line(
                &mut out,
                format!(
                    "| {} 字段完整率 | {:.1}% |",
                    stats.dataset,
                    rate(metrics.fields_complete, metrics.evaluated)
                ),
            );
            line(
                &mut out,
                format!(
                    "| {} 降 Plain 率 | {:.1}% |",
                    stats.dataset,
                    rate(metrics.degraded, metrics.evaluated)
                ),
            );
        }
        if let Some(latency) = &self.latency {
            line(
                &mut out,
                format!("| 端到端延迟 p50 | {:.0} ms |", latency.p50),
            );
            line(
                &mut out,
                format!("| 端到端延迟 p95 | {:.0} ms |", latency.p95),
            );
        }
        if let Some(judge) = &self.judge {
            line(
                &mut out,
                format!(
                    "| judge 均分（1..=5） | {:.2}（{} 条计入） |",
                    judge.average, judge.count
                ),
            );
        }
        let Some(stats) = &self.classify else {
            return out;
        };
        {
            if !stats.confusion_rows.is_empty() {
                out.push('\n');
                out.push_str("混淆矩阵（期望 → 实际，仅错误路径）：\n\n");
                out.push_str("| 期望 | 实际 | 次数 |\n| --- | --- | --- |\n");
                for row in &stats.confusion_rows {
                    line(
                        &mut out,
                        format!("| {:?} | {:?} | {} |", row.expected, row.actual, row.count),
                    );
                }
            }
        }
        out
    }
}

fn rate(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64 * 100.0
    }
}

/// 把评分器的输出组装成报告（runner 与测试共用）：混淆矩阵拍平、比率
/// 预计算、时间戳取 UNIX 纪元秒（报告不带额外的时间库依赖）。
pub fn assemble_report(
    mode: &str,
    classify: Option<ClassifyMetrics>,
    tasks: Vec<(&str, TaskMetrics)>,
    latency: Option<&Latency>,
    judge: Option<JudgeSummary>,
    skipped: usize,
) -> EvalReport {
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
        generated_at: format!(
            "unix {}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_secs())
                .unwrap_or_default()
        ),
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
    }
}

/// 把报告写成 `report-dir/<mode>.json` + `<mode>.md`，返回 md 文本。
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

    use super::{EvalReport, assemble_report};
    use crate::dataset::TaskCase;
    use crate::metrics::{ClassifyMetrics, TaskMetrics, TaskVerdict};
    use gloss_core::task::TaskKind;

    #[test]
    fn report_renders_the_table_shape() {
        let report = fixture_report();
        let markdown = report.render_markdown();
        assert!(markdown.contains("| 指标 | 值 |"));
        assert!(markdown.contains("分类 accuracy | 66.7% (2/3)"));
        assert!(markdown.contains("分类 回退率 | 33.3%"));
        assert!(markdown.contains("混淆矩阵"));
        assert!(markdown.contains("| TranslateWord | ExplainCode | 1 |"));
        assert!(markdown.contains("task_translate_word 字段完整率 | 50.0%"));
        assert!(markdown.contains("task_translate_word 降 Plain 率 | 50.0%"));
        assert!(markdown.contains("跳过（无夹具）1 条"));
    }

    #[test]
    fn latency_only_shows_when_sampled() {
        let report = fixture_report();
        assert!(
            !report.render_markdown().contains("延迟"),
            "replay has no latency samples"
        );
    }

    fn fixture_report() -> EvalReport {
        let word_jsonl = concat!(
            "{\"id\":\"w1\",\"kind\":\"TranslateWord\",\"text\":\"gloss\",\"reference\":{\"word\":\"gloss\",\"phonetic\":null,\"senses\":[]}}\n",
            "{\"id\":\"w2\",\"kind\":\"TranslateWord\",\"text\":\"idempotent\",\"reference\":{\"word\":\"idempotent\",\"phonetic\":null,\"senses\":[]}}\n",
        );
        let cases: Vec<TaskCase> = crate::dataset::load_task(word_jsonl).expect("cases");
        let confusion = BTreeMap::from([(
            TaskKind::TranslateWord,
            BTreeMap::from([(TaskKind::ExplainCode, 1)]),
        )]);
        let classify = ClassifyMetrics {
            evaluated: 3,
            correct: 2,
            invalid_json: 1,
            confusion,
            rejected: 0,
        };
        let verdicts = [
            TaskVerdict::for_reply(
                &cases[0],
                "```gloss\n{\"word\":\"gloss\",\"senses\":[]}\n```",
            ),
            TaskVerdict::for_reply(&cases[1], "只有正文"),
        ];
        let mut task_metrics = TaskMetrics::default();
        for verdict in &verdicts {
            task_metrics.record(verdict);
        }
        assemble_report(
            "replay",
            Some(classify),
            vec![("task_translate_word", task_metrics)],
            None,
            None,
            1,
        )
    }
}
