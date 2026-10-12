//! 评测趋势 SVG 渲染：`render_svg` 是纯函数——入参只认报告切片，不读
//! 环境、不落盘；输出确定性字节（无时间戳、浮点坐标固定一位小数、遍历
//! 次序稳定），同输入两次渲染逐字节一致，入库后 PR diff 即数据 diff。
//!
//! 形态基准是 `docs/plans/2026-10-12-eval-history-demo.svg`：960 宽白底、
//! 面板纵向堆叠（标题 + 内联图例、细网格与刻度、frame、polyline + 圆点、
//! x 轴稀疏刻度）、每点内嵌 `<title>` 悬浮数值；缺样本断线跳段不造点。

use crate::metrics::TaskMetrics;
use crate::report::EvalReport;

/// 趋势图最多绘制的点数：只画最近 100 点（JSONL 本身不裁剪）。
pub const HISTORY_SVG_MAX_POINTS: usize = 100;

const WIDTH: usize = 960;
const PLOT_LEFT: f64 = 46.0;
const PLOT_RIGHT: f64 = 946.0;
const X_INSET: f64 = 14.0;
const FIRST_TITLE_Y: usize = 74;
const PANEL_PITCH: usize = 188;
const FRAME_TOP_OFFSET: usize = 8;
const FRAME_HEIGHT: usize = 132;
const TICK_LABEL_DY: usize = 16;
const BOTTOM_PAD: usize = 14;
const EMPTY_HEIGHT: usize = 80;
const MAX_X_TICKS: usize = 4;
const LEGEND_FONT_SIZE: f64 = 12.0;
const SWATCH_SIZE: usize = 10;
const SWATCH_BASELINE_OFFSET: usize = 9;
const SWATCH_TEXT_GAP: f64 = 4.0;
const LEGEND_ITEM_GAP: f64 = 30.0;

const COLOR_TEXT: &str = "#24292f";
const COLOR_MUTED: &str = "#5b6470";
const COLOR_GRID: &str = "#e7eaee";
const COLOR_FRAME: &str = "#cdd2da";
const COLOR_BLUE: &str = "#3b6fe0";
const COLOR_ORANGE: &str = "#e8862e";
const COLOR_GREEN: &str = "#2f9e57";
const COLOR_PINK: &str = "#d64c7f";

const FONT_STACK: &str = "-apple-system,'Segoe UI',sans-serif";
const FOOTNOTE: &str = "口径：replay 轨度量生产校验器对固化夹具的判定，live 轨为模型现状；数据源 history.jsonl，仅绘最近 100 点。";
const EMPTY_NOTE: &str = "暂无评测历史。";

/// 确定性渲染：超 [`HISTORY_SVG_MAX_POINTS`] 只画最近 100 点；面板数随
/// 数据条件变化（judge 与延迟仅历史有样本才画）。
pub fn render_svg(history: &[EvalReport]) -> String {
    let points = latest_points(history);
    let labels: Vec<String> = points.iter().map(point_label).collect();
    let xs = x_positions(points.len());
    let panels = build_panels(points);
    let height = svg_height(panels.len());
    let mut out = String::new();
    line(
        &mut out,
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{WIDTH}\" height=\"{height}\" viewBox=\"0 0 {WIDTH} {height}\" font-family=\"{FONT_STACK}\">"
        ),
    );
    line(
        &mut out,
        format!("<rect x=\"0\" y=\"0\" width=\"{WIDTH}\" height=\"{height}\" fill=\"#ffffff\"/>"),
    );
    line(
        &mut out,
        format!(
            "<text x=\"46\" y=\"26\" font-size=\"14\" font-weight=\"600\" fill=\"{COLOR_TEXT}\">Prompt 评测趋势（{} 点）</text>",
            points.len()
        ),
    );
    line(
        &mut out,
        format!(
            "<text x=\"46\" y=\"46\" font-size=\"11\" fill=\"{COLOR_MUTED}\">{FOOTNOTE}</text>"
        ),
    );
    if panels.is_empty() {
        line(
            &mut out,
            format!(
                "<text x=\"46\" y=\"66\" font-size=\"12\" fill=\"{COLOR_MUTED}\">{EMPTY_NOTE}</text>"
            ),
        );
    }
    for (index, panel) in panels.iter().enumerate() {
        draw_panel(&mut out, index, panel, &labels, &xs);
    }
    line(&mut out, "</svg>".to_owned());
    out
}

/// 面板纵轴的口径：决定值域与网格刻度的取法。
#[derive(Debug, Clone, Copy)]
enum Axis {
    /// 百分比轴：固定 0..=100，每 25 一条网格。
    Percent,
    /// 修复轮次轴：0 起，顶值 = 容纳数据的 5 的倍数（至少 10）。
    Rounds,
    /// judge 分轴：1..=5 内按数据跨度留白。
    Judge,
    /// 延迟轴：数据范围上下各留 10%，网格取 1-2-5 步长。
    Latency,
}

impl Axis {
    /// 值域与网格（值域上下界把网格线含在图内；上下界按构造恒不等）。
    fn layout(&self, values: &[f64]) -> (f64, f64, Vec<Gridline>) {
        match self {
            Axis::Percent => (
                0.0,
                100.0,
                [0.0, 25.0, 50.0, 75.0, 100.0]
                    .into_iter()
                    .map(|value| Gridline {
                        value,
                        label: fmt_value(value),
                    })
                    .collect(),
            ),
            Axis::Rounds => {
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
            Axis::Judge => {
                let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
                let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                let half = (max - min).max(0.8) / 2.0;
                let center = (min + max) / 2.0;
                let lo = (center - half).max(1.0);
                let hi = (center + half).min(5.0);
                let mid = (lo + hi) / 2.0;
                let gridline = |value: f64| Gridline {
                    value,
                    label: format!("{value:.1}"),
                };
                (lo, hi, vec![gridline(lo), gridline(mid), gridline(hi)])
            }
            Axis::Latency => {
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
        }
    }
}

/// 网格刻度：值 + 轴标签文本。
struct Gridline {
    value: f64,
    label: String,
}

/// 序列数值在悬浮框里的单位与格式。
#[derive(Debug, Clone, Copy)]
enum Unit {
    /// 百分比（一位小数 + 空格 + %）。
    Percent,
    /// 修复轮次（整数 + 轮）。
    Rounds,
    /// judge 分（一位小数 + 分）。
    Score,
    /// 毫秒（整数 + ms）。
    Milliseconds,
}

impl Unit {
    fn title_value(self, value: f64) -> String {
        match self {
            Unit::Percent => format!("{value:.1} %"),
            Unit::Rounds => format!("{} 轮", fmt_value(value)),
            Unit::Score => format!("{value:.1} 分"),
            Unit::Milliseconds => format!("{} ms", fmt_value(value)),
        }
    }
}

/// 面板内的一条序列：各点取值，缺样本为 `None`（断线跳段不造点）。
struct Series {
    label: &'static str,
    color: &'static str,
    unit: Unit,
    values: Vec<Option<f64>>,
}

/// 一个纵向面板：标题、值域、网格与序列组。
struct Panel {
    title: String,
    lo: f64,
    hi: f64,
    gridlines: Vec<Gridline>,
    series: Vec<Series>,
}

fn build_panels(points: &[EvalReport]) -> Vec<Panel> {
    let mut panels = Vec::new();
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
        &mut panels,
        "分类轨判定质量（%）",
        Axis::Percent,
        vec![
            Series {
                label: "accuracy",
                color: COLOR_BLUE,
                unit: Unit::Percent,
                values: accuracy,
            },
            Series {
                label: "回退率",
                color: COLOR_ORANGE,
                unit: Unit::Percent,
                values: fallback,
            },
        ],
    );

    push_panel(
        &mut panels,
        "任务轨契约达成（%，全任务集聚合）",
        Axis::Percent,
        vec![
            Series {
                label: "契约 JSON 率",
                color: COLOR_BLUE,
                unit: Unit::Percent,
                values: aggregate_task_rate(points, |metrics| metrics.json_object),
            },
            Series {
                label: "字段完整率",
                color: COLOR_GREEN,
                unit: Unit::Percent,
                values: aggregate_task_rate(points, |metrics| metrics.fields_complete),
            },
            Series {
                label: "降级率",
                color: COLOR_PINK,
                unit: Unit::Percent,
                values: aggregate_task_rate(points, |metrics| metrics.degraded),
            },
        ],
    );

    let classify_rounds = points
        .iter()
        .map(|report| {
            report
                .classify
                .as_ref()
                .map(|stats| f64::from(stats.metrics.repair_rounds))
        })
        .collect();
    let task_rounds = points
        .iter()
        .map(|report| {
            (!report.tasks.is_empty()).then(|| {
                report
                    .tasks
                    .iter()
                    .map(|stats| f64::from(stats.metrics.repair_rounds))
                    .sum()
            })
        })
        .collect();
    push_panel(
        &mut panels,
        "修复成本（轮）",
        Axis::Rounds,
        vec![
            Series {
                label: "分类轨",
                color: COLOR_BLUE,
                unit: Unit::Rounds,
                values: classify_rounds,
            },
            Series {
                label: "任务轨",
                color: COLOR_ORANGE,
                unit: Unit::Rounds,
                values: task_rounds,
            },
        ],
    );

    let judge = points
        .iter()
        .map(|report| report.judge.map(|summary| summary.average))
        .collect();
    push_panel(
        &mut panels,
        "judge 均分（1–5）",
        Axis::Judge,
        vec![Series {
            label: "judge 均分",
            color: COLOR_BLUE,
            unit: Unit::Score,
            values: judge,
        }],
    );

    let p50 = points
        .iter()
        .map(|report| report.latency.map(|summary| summary.p50))
        .collect();
    let p95 = points
        .iter()
        .map(|report| report.latency.map(|summary| summary.p95))
        .collect();
    push_panel(
        &mut panels,
        "端到端延迟（ms）",
        Axis::Latency,
        vec![
            Series {
                label: "p50",
                color: COLOR_BLUE,
                unit: Unit::Milliseconds,
                values: p50,
            },
            Series {
                label: "p95",
                color: COLOR_ORANGE,
                unit: Unit::Milliseconds,
                values: p95,
            },
        ],
    );
    panels
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

/// 全任务集聚合的任务轨比率：分子跨数据集求和，分母 = 总 evaluated；
/// 没有任何样本（分母 0）视作缺样本。
fn aggregate_task_rate(
    points: &[EvalReport],
    pick: impl Fn(&TaskMetrics) -> usize,
) -> Vec<Option<f64>> {
    points
        .iter()
        .map(|report| {
            let mut numerator = 0usize;
            let mut denominator = 0usize;
            for stats in &report.tasks {
                numerator += pick(&stats.metrics);
                denominator += stats.metrics.evaluated;
            }
            (denominator > 0).then(|| numerator as f64 / denominator as f64 * 100.0)
        })
        .collect()
}

fn draw_panel(out: &mut String, index: usize, panel: &Panel, labels: &[String], xs: &[f64]) {
    let title_y = FIRST_TITLE_Y + index * PANEL_PITCH;
    let frame_top = title_y + FRAME_TOP_OFFSET;
    let frame_bottom = frame_top + FRAME_HEIGHT;
    let tick_y = frame_bottom + TICK_LABEL_DY;
    line(
        out,
        format!(
            "<text x=\"46\" y=\"{title_y}\" font-size=\"13\" font-weight=\"600\" fill=\"{COLOR_TEXT}\">{}</text>",
            esc(&panel.title)
        ),
    );
    let mut swatch_x = PLOT_RIGHT - SWATCH_SIZE as f64;
    for series in panel.series.iter().rev() {
        line(
            out,
            format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{SWATCH_SIZE}\" height=\"{SWATCH_SIZE}\" rx=\"2\" fill=\"{}\"/>",
                f1(swatch_x),
                title_y - SWATCH_BASELINE_OFFSET,
                series.color
            ),
        );
        line(
            out,
            format!(
                "<text x=\"{}\" y=\"{title_y}\" font-size=\"12\" fill=\"{COLOR_MUTED}\" text-anchor=\"end\">{}</text>",
                f1(swatch_x - SWATCH_TEXT_GAP),
                esc(series.label)
            ),
        );
        swatch_x -= SWATCH_TEXT_GAP + text_w(series.label, LEGEND_FONT_SIZE) + LEGEND_ITEM_GAP;
    }
    let span = panel.hi - panel.lo;
    let y_of = |value: f64| frame_bottom as f64 - (value - panel.lo) / span * FRAME_HEIGHT as f64;
    for gridline in &panel.gridlines {
        let y = y_of(gridline.value);
        line(
            out,
            format!(
                "<line x1=\"46\" y1=\"{}\" x2=\"946\" y2=\"{}\" stroke=\"{COLOR_GRID}\" stroke-width=\"1\"/>",
                f1(y),
                f1(y)
            ),
        );
        line(
            out,
            format!(
                "<text x=\"40\" y=\"{}\" font-size=\"11\" fill=\"{COLOR_MUTED}\" text-anchor=\"end\">{}</text>",
                f1(y + 4.0),
                gridline.label
            ),
        );
    }
    line(
        out,
        format!(
            "<rect x=\"46\" y=\"{frame_top}\" width=\"900\" height=\"{FRAME_HEIGHT}\" fill=\"none\" stroke=\"{COLOR_FRAME}\" stroke-width=\"1\"/>"
        ),
    );
    for tick in tick_indices(xs.len()) {
        line(
            out,
            format!(
                "<text x=\"{}\" y=\"{tick_y}\" font-size=\"11\" fill=\"{COLOR_MUTED}\" text-anchor=\"middle\">{}</text>",
                f1(xs[tick]),
                esc(&labels[tick])
            ),
        );
    }
    for series in &panel.series {
        for segment in segments(&series.values) {
            if segment.len() >= 2 {
                let points = segment
                    .iter()
                    .map(|(point, value)| format!("{},{}", f1(xs[*point]), f1(y_of(*value))))
                    .collect::<Vec<_>>()
                    .join(" ");
                line(
                    out,
                    format!(
                        "<polyline points=\"{points}\" fill=\"none\" stroke=\"{}\" stroke-width=\"2\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/>",
                        series.color
                    ),
                );
            }
            for (point, value) in &segment {
                let hover = format!(
                    "{} · {}：{}",
                    series.label,
                    labels[*point],
                    series.unit.title_value(*value)
                );
                line(
                    out,
                    format!(
                        "<circle cx=\"{}\" cy=\"{}\" r=\"2.5\" fill=\"{}\"><title>{}</title></circle>",
                        f1(xs[*point]),
                        f1(y_of(*value)),
                        series.color,
                        esc(&hover)
                    ),
                );
            }
        }
    }
}

/// 只保留最近 [`HISTORY_SVG_MAX_POINTS`] 个点。
fn latest_points(history: &[EvalReport]) -> &[EvalReport] {
    if history.len() > HISTORY_SVG_MAX_POINTS {
        &history[history.len() - HISTORY_SVG_MAX_POINTS..]
    } else {
        history
    }
}

/// 面板数对应的画布高（含顶部标题区与底部刻度留白）。
fn svg_height(panels: usize) -> usize {
    if panels == 0 {
        EMPTY_HEIGHT
    } else {
        FIRST_TITLE_Y
            + (panels - 1) * PANEL_PITCH
            + FRAME_TOP_OFFSET
            + FRAME_HEIGHT
            + TICK_LABEL_DY
            + BOTTOM_PAD
    }
}

/// x 坐标：首末点各内缩一格，中间均匀分布；单点居中。
fn x_positions(count: usize) -> Vec<f64> {
    if count <= 1 {
        return vec![(PLOT_LEFT + PLOT_RIGHT) / 2.0];
    }
    let step = (PLOT_RIGHT - PLOT_LEFT - 2.0 * X_INSET) / (count - 1) as f64;
    (0..count)
        .map(|index| PLOT_LEFT + X_INSET + index as f64 * step)
        .collect()
}

/// x 轴稀疏刻度取样：≤4 个，均匀取（四舍五入到最近点）。
fn tick_indices(count: usize) -> Vec<usize> {
    if count <= MAX_X_TICKS {
        return (0..count).collect();
    }
    let last = (count - 1) as f64;
    (0..MAX_X_TICKS)
        .map(|index| (index as f64 * last / (MAX_X_TICKS - 1) as f64).round() as usize)
        .collect()
}

/// 连续有样本的段（断线跳段不造点）。
fn segments(values: &[Option<f64>]) -> Vec<Vec<(usize, f64)>> {
    let mut all = Vec::new();
    let mut current: Vec<(usize, f64)> = Vec::new();
    for (index, value) in values.iter().enumerate() {
        match value {
            Some(value) => current.push((index, *value)),
            None if !current.is_empty() => all.push(std::mem::take(&mut current)),
            None => {}
        }
    }
    if !current.is_empty() {
        all.push(current);
    }
    all
}

/// 点标签：context.commit 优先，否则 generated_at 的 UTC 日期 + mode。
fn point_label(report: &EvalReport) -> String {
    if let Some(commit) = report
        .context
        .as_ref()
        .and_then(|context| context.commit.as_ref())
    {
        return commit.clone();
    }
    format!("{} {}", utc_date(&report.generated_at), report.mode)
}

/// `generated_at`（"unix <秒>"）→ UTC 的 MM-DD；解析不出时原样返回。
/// 一律按 UTC 固定换算——跟随本地时区会让同一时间戳在不同机器漂一天。
fn utc_date(generated_at: &str) -> String {
    let Some(secs) = generated_at
        .strip_prefix("unix ")
        .and_then(|rest| rest.trim().parse::<u64>().ok())
    else {
        return generated_at.to_owned();
    };
    let (_, month, day) = civil_from_days((secs / 86_400) as i64);
    format!("{month:02}-{day:02}")
}

/// UNIX 天数 → UTC 民用日期（Howard Hinnant 的 civil_from_days 算法，
/// 零时间库依赖）。
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

/// 容纳 span 的最小 1-2-5×10^k 步长。
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

/// 轴刻度数值文本：整值不带小数，其余一位小数。
fn fmt_value(value: f64) -> String {
    if (value - value.round()).abs() < 1e-9 {
        format!("{}", value.round() as i64)
    } else {
        format!("{value:.1}")
    }
}

/// 按字符宽度估算文本像素宽（CJK/全角 1em、空格 0.52em、其余 0.58em），
/// 供图例右对齐排版。
fn text_w(text: &str, size: f64) -> f64 {
    let em: f64 = text
        .chars()
        .map(|ch| {
            if ch == ' ' {
                0.52
            } else if (ch as u32) >= 0x2E80 || "●▲▼—…".contains(ch) {
                1.0
            } else {
                0.58
            }
        })
        .sum();
    em * size
}

fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// 浮点坐标固定一位小数。
fn f1(value: f64) -> String {
    format!("{value:.1}")
}

fn line(out: &mut String, content: String) {
    out.push_str(&content);
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use crate::metrics::{ClassifyMetrics, Latency, TaskMetrics};
    use crate::report::{ClassifyStats, EvalContext, EvalReport, JudgeSummary, assemble_report};
    use crate::svg::{HISTORY_SVG_MAX_POINTS, render_svg};

    fn classify_metrics() -> ClassifyMetrics {
        ClassifyMetrics {
            evaluated: 60,
            correct: 52,
            invalid_json: 2,
            rejected: 1,
            engine_errors: 1,
            repair_rounds: 5,
            ..ClassifyMetrics::default()
        }
    }

    fn task_metrics() -> TaskMetrics {
        TaskMetrics {
            evaluated: 2,
            json_object: 2,
            note_present: 2,
            fields_complete: 1,
            degraded: 1,
            repair_rounds: 3,
            ..TaskMetrics::default()
        }
    }

    fn latency() -> Latency {
        let mut latency = Latency::default();
        latency.record(100.0);
        latency.record(200.0);
        latency
    }

    fn report_with(
        mode: &str,
        generated_at: &str,
        latency: Option<&Latency>,
        judge: Option<JudgeSummary>,
    ) -> EvalReport {
        let mut report = assemble_report(
            mode,
            Some(classify_metrics()),
            vec![("task_translate_word", task_metrics())],
            latency,
            judge,
            0,
        );
        report.generated_at = generated_at.to_owned();
        report
    }

    fn replay_report(generated_at: &str) -> EvalReport {
        report_with("replay", generated_at, None, None)
    }

    fn live_report(generated_at: &str) -> EvalReport {
        let latency = latency();
        report_with(
            "live",
            generated_at,
            Some(&latency),
            Some(JudgeSummary {
                count: 2,
                average: 4.1,
            }),
        )
    }

    fn with_commit(report: &EvalReport, commit: String) -> EvalReport {
        let mut owned = report.clone();
        owned.context = Some(EvalContext {
            commit: Some(commit),
            source: None,
        });
        owned
    }

    fn manual_report(
        mode: &str,
        generated_at: &str,
        classify: Option<ClassifyStats>,
    ) -> EvalReport {
        EvalReport {
            mode: mode.to_owned(),
            generated_at: generated_at.to_owned(),
            classify,
            tasks: Vec::new(),
            latency: None,
            judge: None,
            skipped: 0,
            context: None,
        }
    }

    #[test]
    fn panel_count_follows_available_samples() {
        let empty = render_svg(&[]);
        assert!(!empty.contains("<polyline"), "no series without points");
        assert!(empty.contains("暂无评测历史"), "empty history gets a note");

        let replay_only = render_svg(&[replay_report("unix 1000")]);
        for title in ["分类轨判定质量", "任务轨契约达成", "修复成本"] {
            assert!(replay_only.contains(title), "{title} panel expected");
        }
        assert!(
            !replay_only.contains("judge 均分（1–5）") && !replay_only.contains("端到端延迟（ms）"),
            "judge and latency panels stay hidden without samples"
        );

        let complete = render_svg(&[live_report("unix 2000")]);
        for title in [
            "分类轨判定质量",
            "任务轨契约达成",
            "修复成本",
            "judge 均分（1–5）",
            "端到端延迟（ms）",
        ] {
            assert!(complete.contains(title), "{title} panel expected");
        }
    }

    #[test]
    fn panels_render_polylines_ticks_and_legends() {
        let svg = render_svg(&[live_report("unix 1000"), live_report("unix 2000")]);
        assert!(svg.contains("<polyline"), "polylines drawn");
        assert!(svg.contains("<circle"), "point dots drawn");
        assert!(
            svg.contains("<title>accuracy · 01-01 live："),
            "hover titles carry the point label: {svg}"
        );
        assert!(svg.contains("stroke=\"#e7eaee\""), "grid lines present");
        assert!(
            svg.contains("text-anchor=\"middle\""),
            "x axis ticks present"
        );
        assert!(svg.contains(">accuracy<"), "legend entries present");
        assert!(
            svg.contains("fill=\"none\" stroke=\"#cdd2da\""),
            "panel frames present"
        );
    }

    #[test]
    fn text_is_xml_escaped() {
        let mode_report = report_with("a<b>&c", "unix 0", None, None);
        let committed = with_commit(&replay_report("unix 0"), "x&<y".to_owned());
        let svg = render_svg(&[mode_report, committed]);
        assert!(
            svg.contains("01-01 a&lt;b&gt;&amp;c"),
            "mode text escapes to entities"
        );
        assert!(
            svg.contains("&amp;&lt;y"),
            "commit text escapes to entities"
        );
        assert!(
            !svg.contains(">x&<y<"),
            "raw angle brackets never land in text"
        );
    }

    #[test]
    fn rendering_is_byte_deterministic() {
        let history = vec![live_report("unix 1000"), replay_report("unix 2000")];
        assert_eq!(
            render_svg(&history),
            render_svg(&history),
            "the same input renders byte-identically"
        );
    }

    #[test]
    fn beyond_max_points_only_latest_are_drawn() {
        let history: Vec<EvalReport> = (0..HISTORY_SVG_MAX_POINTS + 20)
            .map(|index| with_commit(&replay_report("unix 1000"), format!("c{index:03}")))
            .collect();
        let svg = render_svg(&history);
        assert!(
            svg.contains("（100 点）"),
            "the title counts rendered points"
        );
        assert!(svg.contains(">c119<"), "the newest point is drawn");
        assert!(
            !svg.contains(">c019<"),
            "the oldest point falls out of the window"
        );
    }

    #[test]
    fn x_ticks_sample_at_most_four_points() {
        let eight: Vec<EvalReport> = (0..8)
            .map(|index| with_commit(&replay_report("unix 1000"), format!("c{index}")))
            .collect();
        let svg = render_svg(&eight);
        for tick in ["c0", "c2", "c5", "c7"] {
            assert!(svg.contains(&format!(">{tick}<")), "tick {tick} expected");
        }
        assert!(!svg.contains(">c1<"), "intermediate points stay unlabeled");

        let two = render_svg(&[
            with_commit(&replay_report("unix 1000"), "c0".to_owned()),
            with_commit(&replay_report("unix 2000"), "c1".to_owned()),
        ]);
        assert!(
            two.contains(">c0<") && two.contains(">c1<"),
            "short history labels all points"
        );
    }

    #[test]
    fn commit_labels_win_over_dates() {
        let committed = with_commit(&live_report("unix 0"), "ec96217".to_owned());
        let local = replay_report("unix 0");
        let svg = render_svg(&[committed, local]);
        assert!(svg.contains(">ec96217<"), "commit labels win");
        assert!(
            svg.contains(">01-01 replay<"),
            "local points fall back to the UTC date plus mode"
        );
    }

    #[test]
    fn a_panel_with_some_empty_series_hides_those_series() {
        let stats = ClassifyStats {
            metrics: classify_metrics(),
            accuracy: classify_metrics().accuracy(),
            fallback_rate: classify_metrics().fallback_rate(),
            confusion_rows: Vec::new(),
        };
        let svg = render_svg(&[manual_report("replay", "unix 1000", Some(stats))]);
        assert!(svg.contains("分类轨判定质量"), "the classify panel draws");
        assert!(
            !svg.contains("契约 JSON 率"),
            "the task series hides without task samples"
        );
        assert!(
            !svg.contains("任务轨契约达成"),
            "a panel whose every series is empty stays hidden"
        );
    }
}
