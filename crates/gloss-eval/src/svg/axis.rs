//! 面板纵轴的口径：值域、网格刻度与悬浮数值格式。每种轴一个变体，各
//! 自的布局函数保证网格线恒落在值域内（上下界按构造恒不等，画布映射
//! 的除法不会除零）。

#[derive(Debug, Clone, Copy)]
pub(super) enum Axis {
    Percent,
    Rounds,
    Judge,
    Latency,
}

impl Axis {
    pub(super) fn layout(&self, values: &[f64]) -> (f64, f64, Vec<Gridline>) {
        match self {
            Axis::Percent => percent_layout(),
            Axis::Rounds => rounds_layout(values),
            Axis::Judge => judge_layout(values),
            Axis::Latency => latency_layout(values),
        }
    }
}

fn percent_layout() -> (f64, f64, Vec<Gridline>) {
    (
        0.0,
        100.0,
        [0.0, 25.0, 50.0, 75.0, 100.0]
            .into_iter()
            .map(|value| Gridline {
                value,
                label: fmt_value(value),
            })
            .collect(),
    )
}

fn rounds_layout(values: &[f64]) -> (f64, f64, Vec<Gridline>) {
    let data_max = values.iter().cloned().fold(0.0, f64::max);
    let top = ((data_max / 5.0).ceil() * 5.0).max(10.0);
    let mid = top / 2.0;
    (
        0.0,
        top,
        [0.0, mid, top]
            .into_iter()
            .map(|value| Gridline {
                value,
                label: fmt_value(value),
            })
            .collect(),
    )
}

fn judge_layout(values: &[f64]) -> (f64, f64, Vec<Gridline>) {
    let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let half = (max - min).max(0.8) / 2.0;
    let center = (min + max) / 2.0;
    let lo = (center - half).max(1.0);
    let hi = (center + half).min(5.0);
    let mid = (lo + hi) / 2.0;
    let gridline = |value: f64| Gridline {
        value,
        label: format!("{value:.2}"),
    };
    (lo, hi, vec![gridline(lo), gridline(mid), gridline(hi)])
}

fn latency_layout(values: &[f64]) -> (f64, f64, Vec<Gridline>) {
    let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let mut pad = (max - min) * 0.1;
    if pad <= 0.0 {
        pad = max.abs() * 0.1;
    }
    if pad <= 0.0 {
        pad = 100.0;
    }
    let lo = min - pad;
    let hi = max + pad;
    let step = nice_step((hi - lo) / 3.0);
    let first = (lo / step).ceil() * step;
    let count = (((hi - first) / step) + 1e-9).floor().max(0.0) as usize;
    let mut gridlines: Vec<Gridline> = (0..=count)
        .map(|k| first + k as f64 * step)
        .filter(|value| *value <= hi + 1e-9)
        .map(|value| Gridline {
            value,
            label: fmt_value(value),
        })
        .collect();
    if gridlines.is_empty() {
        let mid = (lo + hi) / 2.0;
        for value in [lo, mid, hi] {
            gridlines.push(Gridline {
                value,
                label: fmt_value(value),
            });
        }
    }
    (lo, hi, gridlines)
}

pub(super) struct Gridline {
    pub(super) value: f64,
    pub(super) label: String,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum Unit {
    Percent,
    Rounds,
    Score,
    Milliseconds,
}

impl Unit {
    pub(super) fn title_value(self, value: f64) -> String {
        match self {
            Unit::Percent => format!("{value:.2} %"),
            Unit::Rounds => format!("{} rounds", fmt_value(value)),
            Unit::Score => format!("{value:.2} pts"),
            Unit::Milliseconds => format!("{value:.2} ms"),
        }
    }
}

fn nice_step(span: f64) -> f64 {
    let magnitude = 10.0f64.powf(span.log10().floor());
    for multiplier in [1.0, 2.0, 5.0, 10.0] {
        let step = multiplier * magnitude;
        if step >= span {
            return step;
        }
    }
    10.0 * magnitude
}

fn fmt_value(value: f64) -> String {
    if (value - value.round()).abs() < 1e-9 {
        format!("{}", value.round() as i64)
    } else {
        format!("{value:.2}")
    }
}
