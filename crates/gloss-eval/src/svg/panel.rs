//! 面板与序列的数据建模：从报告历史收集延迟序列取值（缺样本为无值），
//! 按面板聚合并定值域。这里不产生任何矢量图（SVG）字节，落墨在落墨
//! 子模块。

use crate::report::EvalReport;

use super::axis::{Axis, Gridline, Unit};
use super::{COLOR_BLUE, COLOR_ORANGE};

pub(super) struct Series {
    pub(super) label: String,
    pub(super) color: &'static str,
    pub(super) unit: Unit,
    pub(super) values: Vec<Option<f64>>,
}

pub(super) struct Panel {
    pub(super) title: String,
    pub(super) lo: f64,
    pub(super) hi: f64,
    pub(super) gridlines: Vec<Gridline>,
    pub(super) series: Vec<Series>,
}

pub(super) fn build_panels(points: &[EvalReport]) -> Vec<Panel> {
    let mut panels = Vec::new();
    push_latency_panel(&mut panels, points);
    panels
}

fn push_latency_panel(panels: &mut Vec<Panel>, points: &[EvalReport]) {
    let p50 = points
        .iter()
        .map(|report| report.latency.map(|summary| summary.p50_ms))
        .collect();
    let p95 = points
        .iter()
        .map(|report| report.latency.map(|summary| summary.p95_ms))
        .collect();
    push_panel(
        panels,
        "end-to-end latency (ms)",
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
