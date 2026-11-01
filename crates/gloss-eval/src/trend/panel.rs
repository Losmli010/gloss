//! 面板与序列的数据建模：曲线序列的唯一清单在此定形——指标汇总面绘
//! 契约有效率、回归通过率、标注合格率与模型总体均分（0–100 直读），
//! 延迟面绘中位延迟与 95 分位延迟；逐轮取值缺数据为无值（模型线缺该
//! 轮、延迟缺样本），全序列无值的面板不画。这里不产生任何矢量图
//! （SVG）字节，落墨在落墨子模块。

use crate::report::{EvalReport, LatencySummary};

use super::axis::{Axis, Gridline, Unit};
use super::{COLOR_AMBER, COLOR_GRAY, COLOR_GREEN, COLOR_TEXT};

pub(super) struct Series {
    pub(super) label: &'static str,
    pub(super) color: &'static str,
    pub(super) unit: Unit,
    pub(super) dashed: bool,
    pub(super) label_above: bool,
    pub(super) values: Vec<Option<f64>>,
}

pub(super) struct Panel {
    pub(super) title: &'static str,
    pub(super) lo: f64,
    pub(super) hi: f64,
    pub(super) gridlines: Vec<Gridline>,
    pub(super) series: Vec<Series>,
}

pub(super) fn build_panels(points: &[&EvalReport]) -> Vec<Panel> {
    let mut panels = Vec::new();
    push_summary_panel(&mut panels, points);
    push_latency_panel(&mut panels, points);
    panels
}

fn push_summary_panel(panels: &mut Vec<Panel>, points: &[&EvalReport]) {
    let rate = |pick: fn(&EvalReport) -> f64| -> Vec<Option<f64>> {
        points.iter().map(|run| Some(pick(run))).collect()
    };
    let model = points
        .iter()
        .map(|run| run.totals.quality.map(|quality| quality.overall.mean))
        .collect();
    push_panel(
        panels,
        "指标汇总",
        Axis::Percent,
        vec![
            Series {
                label: "契约有效率",
                color: COLOR_GREEN,
                unit: Unit::Percent,
                dashed: false,
                label_above: false,
                values: rate(|run| run.totals.valid_rate() * 100.0),
            },
            Series {
                label: "回归通过率",
                color: COLOR_AMBER,
                unit: Unit::Percent,
                dashed: false,
                label_above: false,
                values: rate(|run| run.totals.regression_pass_rate() * 100.0),
            },
            Series {
                label: "标注合格率",
                color: COLOR_TEXT,
                unit: Unit::Percent,
                dashed: false,
                label_above: true,
                values: rate(|run| run.totals.label_pass_rate() * 100.0),
            },
            Series {
                label: "模型总体",
                color: COLOR_GRAY,
                unit: Unit::Score,
                dashed: true,
                label_above: false,
                values: model,
            },
        ],
    );
}

fn push_latency_panel(panels: &mut Vec<Panel>, points: &[&EvalReport]) {
    let percentile = |pick: fn(LatencySummary) -> f64| -> Vec<Option<f64>> {
        points.iter().map(|run| run.latency.map(pick)).collect()
    };
    push_panel(
        panels,
        "延迟",
        Axis::Latency,
        vec![
            Series {
                label: "p50",
                color: COLOR_GREEN,
                unit: Unit::Milliseconds,
                dashed: false,
                label_above: false,
                values: percentile(|summary| summary.p50_ms),
            },
            Series {
                label: "p95",
                color: COLOR_AMBER,
                unit: Unit::Milliseconds,
                dashed: false,
                label_above: false,
                values: percentile(|summary| summary.p95_ms),
            },
        ],
    );
}

fn push_panel(panels: &mut Vec<Panel>, title: &'static str, axis: Axis, series: Vec<Series>) {
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
        title,
        lo,
        hi,
        gridlines,
        series,
    });
}
