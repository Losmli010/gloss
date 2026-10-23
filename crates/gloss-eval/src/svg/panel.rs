//! 面板与序列的数据建模：从报告历史收集各序列取值（缺样本为 `None`），
//! 按面板聚合并定值域。这里不产生任何 SVG 字节，落墨在 `super::draw`。

use crate::metrics::TaskMetrics;
use crate::report::EvalReport;

use super::axis::{Axis, Gridline, Unit};
use super::{COLOR_BLUE, COLOR_GREEN, COLOR_ORANGE, COLOR_PINK, COST_COLORS};

/// 面板内的一条序列：各点取值，缺样本为 `None`（断线跳段不造点）。
pub(super) struct Series {
    pub(super) label: String,
    pub(super) color: &'static str,
    pub(super) unit: Unit,
    pub(super) values: Vec<Option<f64>>,
}

/// 一个纵向面板：标题、值域、网格与序列组。
pub(super) struct Panel {
    pub(super) title: String,
    pub(super) lo: f64,
    pub(super) hi: f64,
    pub(super) gridlines: Vec<Gridline>,
    pub(super) series: Vec<Series>,
}

/// 按轨收集面板：分类质量、任务轨（逐数据集）、修复成本、judge 均分与
/// 端到端延迟——面板出现与否完全由数据决定（无样本整板隐去）。
pub(super) fn build_panels(points: &[EvalReport]) -> Vec<Panel> {
    let mut panels = Vec::new();
    let datasets = dataset_order(points);
    push_classify_panel(&mut panels, points);
    push_task_panels(&mut panels, points, &datasets);
    push_cost_panel(&mut panels, points, &datasets);
    push_judge_panel(&mut panels, points);
    push_latency_panel(&mut panels, points);
    panels
}

fn push_classify_panel(panels: &mut Vec<Panel>, points: &[EvalReport]) {
    let accuracy = points
        .iter()
        .map(|report| report.classify.as_ref().map(|stats| stats.accuracy * 100.0))
        .collect();
    let fallback = points
        .iter()
        .map(|report| {
            report
                .classify
                .as_ref()
                .map(|stats| stats.fallback_rate * 100.0)
        })
        .collect();
    push_panel(
        panels,
        "分类轨判定质量（%）",
        Axis::Percent,
        vec![
            Series {
                label: "accuracy".to_owned(),
                color: COLOR_BLUE,
                unit: Unit::Percent,
                values: accuracy,
            },
            Series {
                label: "回退率".to_owned(),
                color: COLOR_ORANGE,
                unit: Unit::Percent,
                values: fallback,
            },
        ],
    );
}

fn push_task_panels(panels: &mut Vec<Panel>, points: &[EvalReport], datasets: &[String]) {
    for dataset in datasets {
        let title = format!("任务轨 · {}（%）", dataset_label(dataset));
        push_panel(
            panels,
            &title,
            Axis::Percent,
            vec![
                Series {
                    label: "契约 JSON 率".to_owned(),
                    color: COLOR_BLUE,
                    unit: Unit::Percent,
                    values: dataset_rate(points, dataset, |metrics| metrics.json_object),
                },
                Series {
                    label: "字段完整率".to_owned(),
                    color: COLOR_GREEN,
                    unit: Unit::Percent,
                    values: dataset_rate(points, dataset, |metrics| metrics.fields_complete),
                },
                Series {
                    label: "降级率".to_owned(),
                    color: COLOR_PINK,
                    unit: Unit::Percent,
                    values: dataset_rate(points, dataset, |metrics| metrics.degraded),
                },
            ],
        );
    }
}

fn push_cost_panel(panels: &mut Vec<Panel>, points: &[EvalReport], datasets: &[String]) {
    let classify_rounds = points
        .iter()
        .map(|report| {
            report
                .classify
                .as_ref()
                .map(|stats| f64::from(stats.metrics.repair_rounds))
        })
        .collect();
    let mut cost = vec![Series {
        label: "分类轨".to_owned(),
        color: COLOR_BLUE,
        unit: Unit::Rounds,
        values: classify_rounds,
    }];
    for (index, dataset) in datasets.iter().enumerate() {
        cost.push(Series {
            label: dataset_label(dataset).to_owned(),
            color: COST_COLORS[index % COST_COLORS.len()],
            unit: Unit::Rounds,
            values: dataset_rounds(points, dataset),
        });
    }
    push_panel(panels, "修复成本（轮）", Axis::Rounds, cost);
}

fn push_judge_panel(panels: &mut Vec<Panel>, points: &[EvalReport]) {
    // count == 0（judge 启用但全失败）不画点：average 恒为 0.0，画出来会在
    // 1..=5 轴上被误读成「评了 0 分」——与延迟面板「无样本不画」同口径。
    let judge = points
        .iter()
        .map(|report| {
            report
                .judge
                .filter(|summary| summary.count > 0)
                .map(|summary| summary.average)
        })
        .collect();
    push_panel(
        panels,
        "judge 均分（1–5）",
        Axis::Judge,
        vec![Series {
            label: "judge 均分".to_owned(),
            color: COLOR_BLUE,
            unit: Unit::Score,
            values: judge,
        }],
    );
}

fn push_latency_panel(panels: &mut Vec<Panel>, points: &[EvalReport]) {
    let p50 = points
        .iter()
        .map(|report| report.latency.map(|summary| summary.p50))
        .collect();
    let p95 = points
        .iter()
        .map(|report| report.latency.map(|summary| summary.p95))
        .collect();
    push_panel(
        panels,
        "端到端延迟（ms）",
        Axis::Latency,
        vec![
            Series {
                label: "p50".to_owned(),
                color: COLOR_BLUE,
                unit: Unit::Milliseconds,
                values: p50,
            },
            Series {
                label: "p95".to_owned(),
                color: COLOR_ORANGE,
                unit: Unit::Milliseconds,
                values: p95,
            },
        ],
    );
}

/// 收一个面板：全部序列都缺样本的面板整体跳过，部分缺样本的序列只从
/// 图例与曲线里隐去自己。
fn push_panel(panels: &mut Vec<Panel>, title: &str, axis: Axis, series: Vec<Series>) {
    let series: Vec<Series> = series
        .into_iter()
        .filter(|series| series.values.iter().any(Option::is_some))
        .collect();
    if series.is_empty() {
        return;
    }
    let present: Vec<f64> = series
        .iter()
        .flat_map(|series| series.values.iter().copied().flatten())
        .collect();
    let (lo, hi, gridlines) = axis.layout(&present);
    panels.push(Panel {
        title: title.to_owned(),
        lo,
        hi,
        gridlines,
        series,
    });
}

/// 历史中出现过的数据集，按首次出现序排列（确定性遍历序）。去重期借用
/// 各点的数据集名，末尾一次性落成有副本的 `Vec`——每份只复制一次。
fn dataset_order(points: &[EvalReport]) -> Vec<String> {
    let mut seen: Vec<&str> = Vec::new();
    for report in points {
        for stats in &report.tasks {
            if !seen.contains(&stats.dataset.as_str()) {
                seen.push(stats.dataset.as_str());
            }
        }
    }
    seen.into_iter().map(str::to_owned).collect()
}

/// 数据集显示名（未知 id 原样展示）。
fn dataset_label(dataset: &str) -> &str {
    match dataset {
        "task_translate_word" => "词卡",
        "task_translate_sentence" => "句译",
        "task_explain_code" => "代码",
        other => other,
    }
}

/// 单数据集在历史各点的比率；该点缺席数据集或分母为 0 视作缺样本。
fn dataset_rate(
    points: &[EvalReport],
    dataset: &str,
    pick: impl Fn(&TaskMetrics) -> usize,
) -> Vec<Option<f64>> {
    points
        .iter()
        .map(|report| {
            report
                .tasks
                .iter()
                .find(|stats| stats.dataset == dataset)
                .and_then(|stats| {
                    (stats.metrics.evaluated > 0).then(|| {
                        pick(&stats.metrics) as f64 / stats.metrics.evaluated as f64 * 100.0
                    })
                })
        })
        .collect()
}

/// 单数据集在历史各点的修复轮次；该点缺席数据集视作缺样本。
fn dataset_rounds(points: &[EvalReport], dataset: &str) -> Vec<Option<f64>> {
    points
        .iter()
        .map(|report| {
            report
                .tasks
                .iter()
                .find(|stats| stats.dataset == dataset)
                .map(|stats| f64::from(stats.metrics.repair_rounds))
        })
        .collect()
}
