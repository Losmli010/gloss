//! 面板纵轴的口径：值域、网格刻度与数值格式。指标汇总轴固定 0–100、
//! 按 25% 步进；延迟轴从零起、上界取四等分后落整刻度的天花板，网格线
//! 恒落在值域内（上下界按构造恒不等，画布映射的除法不会除零）。

#[derive(Debug, Clone, Copy)]
pub(super) enum Axis {
    Percent,
    Latency,
}

impl Axis {
    pub(super) fn layout(&self, values: &[f64]) -> (f64, f64, Vec<Gridline>) {
        match self {
            Axis::Percent => percent_layout(),
            Axis::Latency => latency_layout(values),
        }
    }
}

fn percent_layout() -> (f64, f64, Vec<Gridline>) {
    let gridlines = [100.0, 75.0, 50.0, 25.0, 0.0]
        .into_iter()
        .map(|value| Gridline {
            value,
            label: format_percent(value),
        })
        .collect();
    (0.0, 100.0, gridlines)
}

fn latency_layout(values: &[f64]) -> (f64, f64, Vec<Gridline>) {
    let max = values.iter().cloned().fold(0.0_f64, f64::max);
    let hi = nice_ceiling(max);
    let gridlines = [4, 3, 2, 1, 0]
        .into_iter()
        .map(|quarter| {
            let value = hi * quarter as f64 / 4.0;
            Gridline {
                value,
                label: format_ms(value),
            }
        })
        .collect();
    (0.0, hi, gridlines)
}

pub(super) struct Gridline {
    pub(super) value: f64,
    pub(super) label: String,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum Unit {
    Percent,
    Score,
    Milliseconds,
}

impl Unit {
    pub(super) fn title_value(self, value: f64) -> String {
        match self {
            Unit::Percent => format_percent(value),
            Unit::Score => format_trimmed(value),
            Unit::Milliseconds => format_ms(value),
        }
    }
}

fn format_percent(value: f64) -> String {
    format_trimmed(value) + "%"
}

fn format_ms(value: f64) -> String {
    if value >= 1000.0 {
        format_trimmed(value / 1000.0) + "s"
    } else {
        format!("{}ms", value.round() as i64)
    }
}

fn format_trimmed(value: f64) -> String {
    let tenth = (value * 10.0).round() as i64;
    if tenth % 10 == 0 {
        format!("{}", tenth / 10)
    } else {
        format!("{value:.1}")
    }
}

fn nice_ceiling(max: f64) -> f64 {
    for candidate in QUARTER_CEILINGS {
        if *candidate >= max {
            return *candidate;
        }
    }
    let unit = QUARTER_CEILINGS[QUARTER_CEILINGS.len() - 1];
    (max / unit).ceil() * unit
}

const QUARTER_CEILINGS: &[f64] = &[
    400.0, 800.0, 1200.0, 1600.0, 2000.0, 4000.0, 8000.0, 16000.0, 32000.0, 60000.0, 120000.0,
    240000.0, 480000.0, 960000.0,
];
