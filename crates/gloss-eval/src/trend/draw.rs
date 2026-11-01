//! 面板落墨：按图形元素分步——小节标题与内联图例、网格与纵轴标签、
//! 外框与横轴日期刻度、序列折线与圆点、最新一轮的放大圆点与末值标
//! 注。发射顺序即字节顺序，改动任何一步都会改写渲染产物。

use crate::report::{EvalReport, format_utc_short};

use super::panel::Panel;
use super::{
    COLOR_FRAME, COLOR_GRID, COLOR_MUTED, COLOR_TEXT, FIRST_TITLE_Y, FRAME_HEIGHT,
    FRAME_TOP_OFFSET, LEGEND_FONT_SIZE, LEGEND_ITEM_GAP, MAX_X_TICKS, PANEL_PITCH, PLOT_LEFT,
    PLOT_RIGHT, SWATCH_BASELINE_OFFSET, SWATCH_SIZE, SWATCH_TEXT_GAP, TICK_LABEL_DY, X_INSET, esc,
    f2, line,
};

pub(super) struct PointData {
    xs: Vec<f64>,
    ticks: Vec<String>,
    hovers: Vec<String>,
}

impl PointData {
    pub(super) fn new(points: &[&EvalReport]) -> Self {
        Self {
            xs: x_positions(points.len()),
            ticks: points.iter().map(|report| run_date(report)).collect(),
            hovers: points.iter().map(|report| hover_label(report)).collect(),
        }
    }
}

pub(super) fn draw_panel(out: &mut String, index: usize, panel: &Panel, points: &PointData) {
    let title_y = FIRST_TITLE_Y + index * PANEL_PITCH;
    let frame_top = title_y + FRAME_TOP_OFFSET;
    let frame_bottom = frame_top + FRAME_HEIGHT;
    let tick_y = frame_bottom + TICK_LABEL_DY;
    draw_title_and_legend(out, title_y, panel);
    draw_grid(out, panel, frame_bottom);
    draw_frame(out, frame_top);
    draw_x_ticks(out, tick_y, points);
    draw_series(out, panel, points, frame_bottom);
}

fn draw_title_and_legend(out: &mut String, title_y: usize, panel: &Panel) {
    line(
        out,
        format!(
            "<text x=\"46\" y=\"{title_y}\" font-size=\"13\" font-weight=\"600\" fill=\"{COLOR_TEXT}\">{}</text>",
            esc(panel.title)
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
                esc(series.label)
            ),
        );
        swatch_x -= SWATCH_TEXT_GAP + text_w(series.label, LEGEND_FONT_SIZE) + LEGEND_ITEM_GAP;
    }
}

fn draw_grid(out: &mut String, panel: &Panel, frame_bottom: usize) {
    for gridline in &panel.gridlines {
        let y = y_of(gridline.value, panel, frame_bottom);
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
}

fn y_of(value: f64, panel: &Panel, frame_bottom: usize) -> f64 {
    frame_bottom as f64 - (value - panel.lo) / (panel.hi - panel.lo) * FRAME_HEIGHT as f64
}

fn draw_frame(out: &mut String, frame_top: usize) {
    line(
        out,
        format!(
            "<rect x=\"46\" y=\"{frame_top}\" width=\"900\" height=\"{FRAME_HEIGHT}\" fill=\"none\" stroke=\"{COLOR_FRAME}\" stroke-width=\"1\"/>"
        ),
    );
}

fn draw_x_ticks(out: &mut String, tick_y: usize, points: &PointData) {
    for tick in tick_indices(points.xs.len()) {
        line(
            out,
            format!(
                "<text x=\"{}\" y=\"{tick_y}\" font-size=\"11\" fill=\"{COLOR_MUTED}\" text-anchor=\"middle\">{}</text>",
                f2(points.xs[tick]),
                esc(&points.ticks[tick])
            ),
        );
    }
}

fn draw_series(out: &mut String, panel: &Panel, points: &PointData, frame_bottom: usize) {
    for series in &panel.series {
        for segment in segments(&series.values) {
            if segment.len() >= 2 {
                let coordinates = segment
                    .iter()
                    .map(|(point, value)| {
                        format!(
                            "{},{}",
                            f2(points.xs[*point]),
                            f2(y_of(*value, panel, frame_bottom))
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                draw_polyline(out, series, &coordinates);
            }
            for (point, value) in &segment {
                let hover = format!(
                    "{} · {}: {}",
                    series.label,
                    points.hovers[*point],
                    series.unit.title_value(*value)
                );
                let newest = *point + 1 == series.values.len();
                let radius = if newest { "4" } else { "2.5" };
                line(
                    out,
                    format!(
                        "<circle cx=\"{}\" cy=\"{}\" r=\"{radius}\" fill=\"{}\"><title>{}</title></circle>",
                        f2(points.xs[*point]),
                        f2(y_of(*value, panel, frame_bottom)),
                        series.color,
                        esc(&hover)
                    ),
                );
                if newest {
                    let dot_y = y_of(*value, panel, frame_bottom);
                    let label_y = if series.label_above {
                        dot_y - 8.0
                    } else {
                        dot_y + 16.0
                    };
                    draw_end_label(out, series, points.xs[*point], label_y, *value);
                }
            }
        }
    }
}

fn draw_end_label(out: &mut String, series: &super::panel::Series, x: f64, y: f64, value: f64) {
    line(
        out,
        format!(
            "<text x=\"{}\" y=\"{}\" font-size=\"11\" font-weight=\"600\" fill=\"{}\" text-anchor=\"end\">{}</text>",
            f2(x - 8.0),
            f2(y),
            series.color,
            esc(&series.unit.title_value(value))
        ),
    );
}

fn draw_polyline(out: &mut String, series: &super::panel::Series, coordinates: &str) {
    let dash = if series.dashed {
        " stroke-dasharray=\"4 3\""
    } else {
        ""
    };
    line(
        out,
        format!(
            "<polyline points=\"{coordinates}\" fill=\"none\" stroke=\"{}\" stroke-width=\"2\" stroke-linejoin=\"round\" stroke-linecap=\"round\"{dash}/>",
            series.color
        ),
    );
}

fn hover_label(report: &EvalReport) -> String {
    let mut label = format!("{} {}", format_utc_short(report.generated_at), report.mode);
    if let Some(commit) = report
        .context
        .as_ref()
        .and_then(|context| context.commit.as_ref())
    {
        label.push_str(&format!(" ({commit})"));
    }
    label
}

fn run_date(report: &EvalReport) -> String {
    format_utc_short(report.generated_at)[..5].to_owned()
}

fn x_positions(count: usize) -> Vec<f64> {
    if count <= 1 {
        return vec![(PLOT_LEFT + PLOT_RIGHT) / 2.0];
    }
    let step = (PLOT_RIGHT - PLOT_LEFT - 2.0 * X_INSET) / (count - 1) as f64;
    (0..count)
        .map(|index| PLOT_LEFT + X_INSET + index as f64 * step)
        .collect()
}

fn tick_indices(count: usize) -> Vec<usize> {
    if count <= MAX_X_TICKS {
        return (0..count).collect();
    }
    let last = (count - 1) as f64;
    (0..MAX_X_TICKS)
        .map(|index| (index as f64 * last / (MAX_X_TICKS - 1) as f64).round() as usize)
        .collect()
}

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
