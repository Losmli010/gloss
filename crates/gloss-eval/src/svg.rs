//! 评测趋势 SVG 渲染：`render_svg` 是纯函数——入参只认报告切片，不读
//! 环境、不落盘；输出确定性字节（无渲染时刻的时间戳、浮点坐标固定两位
//! 小数、遍历次序稳定），同输入两次渲染逐字节一致，入库后 PR diff 即数
//! 据 diff。
//!
//! 图面：960 宽白底、面板纵向堆叠（标题 + 内联图例、细网格与刻度、
//! frame、polyline + 圆点、x 轴稀疏刻度）；x 轴刻度是各点的 UTC 时间
//! （`MM-DD HH:MM`），mode 与 commit 进悬浮 `<title>`，右上角固定标注
//! 最近一次运行；任务轨按数据集各一面板，修复成本逐数据集出线。缺样
//! 本断线跳段不造点。

use crate::metrics::TaskMetrics;
use crate::report::{EvalReport, format_utc, format_utc_short};

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

/// 修复成本面板里各任务数据集的线色（分类轨固定蓝色）。
const COST_COLORS: [&str; 3] = [COLOR_ORANGE, COLOR_GREEN, COLOR_PINK];

const FONT_STACK: &str = "-apple-system,'Segoe UI',sans-serif";
const FOOTNOTE: &str = "口径：replay 轨度量生产校验器对固化夹具的判定，live 轨为模型现状；数据源 history.jsonl，仅绘最近 100 点，时间一律 UTC。";
const EMPTY_NOTE: &str = "暂无评测历史。";

/// 确定性渲染：超 [`HISTORY_SVG_MAX_POINTS`] 只画最近 100 点；面板数随
/// 数据条件变化（judge 与延迟仅历史有样本才画，任务面板随数据集出现）。
pub fn render_svg(history: &[EvalReport]) -> String {
    let points = latest_points(history);
    let hovers: Vec<String> = points.iter().map(hover_label).collect();
    let ticks: Vec<String> = points
        .iter()
        .map(|report| format_utc_short(report.generated_at))
        .collect();
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
    if let Some(latest) = points.last() {
        line(
            &mut out,
            format!(
                "<text x=\"946\" y=\"26\" font-size=\"11\" fill=\"{COLOR_MUTED}\" text-anchor=\"end\">{}</text>",
                esc(&latest_note(latest))
            ),
        );
    }
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
        draw_panel(&mut out, index, panel, &ticks, &hovers, &xs);
    }
    line(&mut out, "</svg>".to_owned());
    out
}

/// 右上角「最近一次」标注：UTC 时间 + mode（+ commit）。
fn latest_note(report: &EvalReport) -> String {
    let mut note = format!(
        "最近一次 {} UTC · {}",
        format_utc(report.generated_at),
        report.mode
    );
    if let Some(commit) = report
        .context
        .as_ref()
        .and_then(|context| context.commit.as_ref())
    {
        note.push_str(&format!("（{commit}）"));
    }
    note
}

/// 悬浮标签：UTC 时间 + mode（+ commit）。
fn hover_label(report: &EvalReport) -> String {
    let mut label = format!("{} {}", format_utc_short(report.generated_at), report.mode);
    if let Some(commit) = report
        .context
        .as_ref()
        .and_then(|context| context.commit.as_ref())
    {
        label.push_str(&format!("（{commit}）"));
    }
    label
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
                    label: format!("{value:.2}"),
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
    /// 百分比（两位小数 + 空格 + %）。
    Percent,
    /// 修复轮次（整数 + 轮）。
    Rounds,
    /// judge 分（两位小数 + 分）。
    Score,
    /// 毫秒（两位小数 + ms）。
    Milliseconds,
}

impl Unit {
    fn title_value(self, value: f64) -> String {
        match self {
            Unit::Percent => format!("{value:.2} %"),
            Unit::Rounds => format!("{} 轮", fmt_value(value)),
            Unit::Score => format!("{value:.2} 分"),
            Unit::Milliseconds => format!("{value:.2} ms"),
        }
    }
}

/// 面板内的一条序列：各点取值，缺样本为 `None`（断线跳段不造点）。
struct Series {
    label: String,
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

    for dataset in dataset_order(points) {
        let title = format!("任务轨 · {}（%）", dataset_label(&dataset));
        push_panel(
            &mut panels,
            &title,
            Axis::Percent,
            vec![
                Series {
                    label: "契约 JSON 率".to_owned(),
                    color: COLOR_BLUE,
                    unit: Unit::Percent,
                    values: dataset_rate(points, &dataset, |metrics| metrics.json_object),
                },
                Series {
                    label: "字段完整率".to_owned(),
                    color: COLOR_GREEN,
                    unit: Unit::Percent,
                    values: dataset_rate(points, &dataset, |metrics| metrics.fields_complete),
                },
                Series {
                    label: "降级率".to_owned(),
                    color: COLOR_PINK,
                    unit: Unit::Percent,
                    values: dataset_rate(points, &dataset, |metrics| metrics.degraded),
                },
            ],
        );
    }

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
    for (index, dataset) in dataset_order(points).iter().enumerate() {
        cost.push(Series {
            label: dataset_label(dataset).to_owned(),
            color: COST_COLORS[index % COST_COLORS.len()],
            unit: Unit::Rounds,
            values: dataset_rounds(points, dataset),
        });
    }
    push_panel(&mut panels, "修复成本（轮）", Axis::Rounds, cost);

    let judge = points
        .iter()
        .map(|report| report.judge.map(|summary| summary.average))
        .collect();
    push_panel(
        &mut panels,
        "judge 均分（1–5）",
        Axis::Judge,
        vec![Series {
            label: "judge 均分".to_owned(),
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

/// 历史中出现过的数据集，按首次出现序排列（确定性遍历序）。
fn dataset_order(points: &[EvalReport]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for report in points {
        for stats in &report.tasks {
            if !seen.iter().any(|name| name == &stats.dataset) {
                seen.push(stats.dataset.clone());
            }
        }
    }
    seen
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

fn draw_panel(
    out: &mut String,
    index: usize,
    panel: &Panel,
    ticks: &[String],
    hovers: &[String],
    xs: &[f64],
) {
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
                f2(swatch_x),
                title_y - SWATCH_BASELINE_OFFSET,
                series.color
            ),
        );
        line(
            out,
            format!(
                "<text x=\"{}\" y=\"{title_y}\" font-size=\"12\" fill=\"{COLOR_MUTED}\" text-anchor=\"end\">{}</text>",
                f2(swatch_x - SWATCH_TEXT_GAP),
                esc(&series.label)
            ),
        );
        swatch_x -= SWATCH_TEXT_GAP + text_w(&series.label, LEGEND_FONT_SIZE) + LEGEND_ITEM_GAP;
    }
    let span = panel.hi - panel.lo;
    let y_of = |value: f64| frame_bottom as f64 - (value - panel.lo) / span * FRAME_HEIGHT as f64;
    for gridline in &panel.gridlines {
        let y = y_of(gridline.value);
        line(
            out,
            format!(
                "<line x1=\"46\" y1=\"{}\" x2=\"946\" y2=\"{}\" stroke=\"{COLOR_GRID}\" stroke-width=\"1\"/>",
                f2(y),
                f2(y)
            ),
        );
        line(
            out,
            format!(
                "<text x=\"40\" y=\"{}\" font-size=\"11\" fill=\"{COLOR_MUTED}\" text-anchor=\"end\">{}</text>",
                f2(y + 4.0),
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
                f2(xs[tick]),
                esc(&ticks[tick])
            ),
        );
    }
    for series in &panel.series {
        for segment in segments(&series.values) {
            if segment.len() >= 2 {
                let points = segment
                    .iter()
                    .map(|(point, value)| format!("{},{}", f2(xs[*point]), f2(y_of(*value))))
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
                    hovers[*point],
                    series.unit.title_value(*value)
                );
                line(
                    out,
                    format!(
                        "<circle cx=\"{}\" cy=\"{}\" r=\"2.5\" fill=\"{}\"><title>{}</title></circle>",
                        f2(xs[*point]),
                        f2(y_of(*value)),
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

/// 轴刻度数值文本：整值不带小数，其余固定两位小数。
fn fmt_value(value: f64) -> String {
    if (value - value.round()).abs() < 1e-9 {
        format!("{}", value.round() as i64)
    } else {
        format!("{value:.2}")
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

/// 浮点坐标固定两位小数。
fn f2(value: f64) -> String {
    format!("{value:.2}")
}

fn line(out: &mut String, content: String) {
    out.push_str(&content);
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use crate::metrics::{ClassifyMetrics, Latency, TaskMetrics};
    use crate::report::{
        ClassifyStats, EvalContext, EvalReport, JudgeSummary, TaskStats, assemble_report,
    };
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
        generated_at: i64,
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
        report.generated_at = generated_at;
        report
    }

    fn replay_report(generated_at: i64) -> EvalReport {
        report_with("replay", generated_at, None, None)
    }

    fn live_report(generated_at: i64) -> EvalReport {
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

    fn manual_report(mode: &str, generated_at: i64, classify: Option<ClassifyStats>) -> EvalReport {
        EvalReport {
            mode: mode.to_owned(),
            generated_at,
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
        assert!(
            !empty.contains("最近一次"),
            "no latest-run note without points"
        );

        let replay_only = render_svg(&[replay_report(1000)]);
        for title in ["分类轨判定质量", "任务轨 · 词卡", "修复成本"] {
            assert!(replay_only.contains(title), "{title} panel expected");
        }
        assert!(
            !replay_only.contains("judge 均分（1–5）") && !replay_only.contains("端到端延迟（ms）"),
            "judge and latency panels stay hidden without samples"
        );
        assert!(
            !replay_only.contains("任务轨 · 句译"),
            "datasets without samples get no panel"
        );

        let complete = render_svg(&[live_report(2000)]);
        for title in [
            "分类轨判定质量",
            "任务轨 · 词卡",
            "修复成本",
            "judge 均分（1–5）",
            "端到端延迟（ms）",
        ] {
            assert!(complete.contains(title), "{title} panel expected");
        }
    }

    #[test]
    fn panels_render_polylines_ticks_and_legends() {
        let svg = render_svg(&[live_report(1000), live_report(2000)]);
        assert!(svg.contains("<polyline"), "polylines drawn");
        assert!(svg.contains("<circle"), "point dots drawn");
        assert!(
            svg.contains("<title>accuracy · 01-01 00:16 live："),
            "hover titles carry the UTC time and mode: {svg}"
        );
        assert!(svg.contains("86.67 %"), "hover values carry two decimals");
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
        assert!(
            svg.contains("最近一次 1970-01-01 00:33 UTC · live"),
            "the header names the latest run"
        );
    }

    #[test]
    fn text_is_xml_escaped() {
        let mode_report = report_with("a<b>&c", 0, None, None);
        let committed = with_commit(&replay_report(0), "x&<y".to_owned());
        let svg = render_svg(&[mode_report, committed]);
        assert!(
            svg.contains("a&lt;b&gt;&amp;c"),
            "mode text escapes to entities"
        );
        assert!(
            svg.contains("（x&amp;&lt;y）"),
            "commit text escapes to entities"
        );
        assert!(
            !svg.contains(">x&<y"),
            "raw angle brackets never land in text"
        );
    }

    #[test]
    fn rendering_is_byte_deterministic() {
        let history = vec![live_report(1000), replay_report(2000)];
        assert_eq!(
            render_svg(&history),
            render_svg(&history),
            "the same input renders byte-identically"
        );
    }

    #[test]
    fn beyond_max_points_only_latest_are_drawn() {
        let history: Vec<EvalReport> = (0..HISTORY_SVG_MAX_POINTS + 20)
            .map(|index| with_commit(&replay_report(1000), format!("c{index:03}")))
            .collect();
        let svg = render_svg(&history);
        assert!(
            svg.contains("（100 点）"),
            "the title counts rendered points"
        );
        assert!(
            svg.contains("（c119）"),
            "the newest point is drawn in hovers"
        );
        assert!(
            !svg.contains("（c019）"),
            "the oldest point falls out of the window"
        );
    }

    #[test]
    fn x_ticks_sample_at_most_four_points() {
        let eight: Vec<EvalReport> = (0..8)
            .map(|index| replay_report(1000 * (index as i64 + 1)))
            .collect();
        let svg = render_svg(&eight);
        for tick in ["01-01 00:16", "01-01 00:50", "01-01 01:40", "01-01 02:13"] {
            assert!(svg.contains(&format!(">{tick}<")), "tick {tick} expected");
        }
        assert!(
            !svg.contains(">01-01 00:33<"),
            "intermediate points stay unlabeled"
        );

        let two = render_svg(&[replay_report(1000), replay_report(2000)]);
        assert!(
            two.contains(">01-01 00:16<") && two.contains(">01-01 00:33<"),
            "short history labels all points"
        );
    }

    #[test]
    fn ticks_carry_timestamps_and_hover_carries_commit() {
        let committed = with_commit(&live_report(0), "ec96217".to_owned());
        let local = replay_report(0);
        let svg = render_svg(&[committed, local]);
        assert!(svg.contains(">01-01 00:00<"), "ticks are UTC timestamps");
        assert!(
            svg.contains("01-01 00:00 live（ec96217）："),
            "committed hovers carry the SHA"
        );
        assert!(
            svg.contains("01-01 00:00 replay："),
            "local hovers carry mode only"
        );
        assert!(
            !svg.contains("replay（"),
            "local points never fake a commit"
        );
        assert!(
            svg.contains("最近一次 1970-01-01 00:00 UTC · replay"),
            "the header names the latest run"
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
        let svg = render_svg(&[manual_report("replay", 1000, Some(stats))]);
        assert!(svg.contains("分类轨判定质量"), "the classify panel draws");
        assert!(
            !svg.contains("契约 JSON 率"),
            "the task series hides without task samples"
        );
        assert!(
            !svg.contains("任务轨 · "),
            "panels whose every dataset is absent stay hidden"
        );
    }

    #[test]
    fn task_panels_split_per_dataset() {
        let mut both = live_report(1000);
        both.tasks.push(TaskStats {
            dataset: "task_translate_sentence".to_owned(),
            metrics: task_metrics(),
        });
        let mut sentence_only = live_report(2000);
        sentence_only.tasks.clear();
        sentence_only.tasks.push(TaskStats {
            dataset: "task_translate_sentence".to_owned(),
            metrics: task_metrics(),
        });
        let svg = render_svg(&[both, sentence_only]);
        assert!(
            svg.contains("任务轨 · 词卡"),
            "the word panel rides the points that carry it"
        );
        assert!(svg.contains("任务轨 · 句译"), "the sentence panel appears");
        for legend in [">分类轨<", ">词卡<", ">句译<"] {
            assert!(
                svg.contains(legend),
                "the repair cost legend carries {legend}"
            );
        }
        assert!(
            !svg.contains("任务轨 · 代码"),
            "datasets never seen get no panel"
        );
    }

    #[test]
    fn header_shows_latest_run_timestamp() {
        let svg = render_svg(&[replay_report(1000), live_report(2000)]);
        assert!(
            svg.contains("最近一次 1970-01-01 00:33 UTC · live"),
            "the header names the latest run"
        );

        let committed = with_commit(&live_report(1_791_763_200), "fc244bc".to_owned());
        assert!(
            render_svg(&[committed]).contains("最近一次 2026-10-12 00:00 UTC · live（fc244bc）"),
            "the header carries full date and commit"
        );
    }
}
