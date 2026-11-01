//! 评测趋势矢量图（SVG）渲染：渲染入口是纯函数——入参只认报告切片，
//! 不读环境、不落盘；输出确定性字节（无渲染时刻的时间戳、浮点坐标
//! 固定两位小数、遍历次序稳定），同输入两次渲染逐字节一致，入库后
//! 拉取请求（PR）的差异即数据差异。
//!
//! 图面：960 宽白底、面板纵向堆叠（标题与内联图例、细网格与刻度、
//! 外框、折线与圆点、横轴稀疏刻度）；横轴刻度是各点的协调世界时
//! （UTC）“月日时分”，模式与提交标识进悬浮提示，右上角固定标注
//! 最近一次运行；延迟分位各一面板，历史无样本不画。缺样本断线跳段
//! 不造点。
//!
//! 分工：本文件只留画布组装（头部、高度、点窗）与发射原语，坐标轴
//! 子模块管值域与网格口径，面板子模块管面板与序列的数据建模，落墨
//! 子模块按图形元素落墨。

mod axis;
mod draw;
mod panel;

use crate::report::{EvalReport, format_utc};

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

const FONT_STACK: &str = "-apple-system,'Segoe UI',sans-serif";
const FOOTNOTE: &str = "Scope: the latency series records end-to-end percentiles from evaluation runs with live samples. Data source: history.jsonl, last 100 points only, timestamps in UTC.";
const EMPTY_NOTE: &str = "No eval history yet.";

pub fn render_svg(history: &[EvalReport]) -> String {
    let points = latest_points(history);
    let data = draw::PointData::new(points);
    let panels = panel::build_panels(points);
    let height = svg_height(panels.len());
    let mut out = String::new();
    write_header(
        &mut out,
        height,
        points.len(),
        points.last(),
        panels.is_empty(),
    );
    for (index, panel) in panels.iter().enumerate() {
        draw::draw_panel(&mut out, index, panel, &data);
    }
    line(&mut out, "</svg>".to_owned());
    out
}

fn write_header(
    out: &mut String,
    height: usize,
    count: usize,
    latest: Option<&EvalReport>,
    empty: bool,
) {
    line(
        out,
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{WIDTH}\" height=\"{height}\" viewBox=\"0 0 {WIDTH} {height}\" font-family=\"{FONT_STACK}\">"
        ),
    );
    line(
        out,
        format!("<rect x=\"0\" y=\"0\" width=\"{WIDTH}\" height=\"{height}\" fill=\"#ffffff\"/>"),
    );
    line(
        out,
        format!(
            "<text x=\"46\" y=\"26\" font-size=\"14\" font-weight=\"600\" fill=\"{COLOR_TEXT}\">Prompt eval trend ({count} points)</text>"
        ),
    );
    if let Some(latest) = latest {
        line(
            out,
            format!(
                "<text x=\"946\" y=\"26\" font-size=\"11\" fill=\"{COLOR_MUTED}\" text-anchor=\"end\">{}</text>",
                esc(&latest_note(latest))
            ),
        );
    }
    line(
        out,
        format!(
            "<text x=\"46\" y=\"46\" font-size=\"11\" fill=\"{COLOR_MUTED}\">{FOOTNOTE}</text>"
        ),
    );
    if empty {
        line(
            out,
            format!(
                "<text x=\"46\" y=\"66\" font-size=\"12\" fill=\"{COLOR_MUTED}\">{EMPTY_NOTE}</text>"
            ),
        );
    }
}

fn latest_note(report: &EvalReport) -> String {
    let mut note = format!(
        "latest {} UTC · {}",
        format_utc(report.generated_at),
        report.mode
    );
    if let Some(commit) = report
        .context
        .as_ref()
        .and_then(|context| context.commit.as_ref())
    {
        note.push_str(&format!(" ({commit})"));
    }
    note
}

fn latest_points(history: &[EvalReport]) -> &[EvalReport] {
    if history.len() > HISTORY_SVG_MAX_POINTS {
        &history[history.len() - HISTORY_SVG_MAX_POINTS..]
    } else {
        history
    }
}

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

fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn f2(value: f64) -> String {
    format!("{value:.2}")
}

fn line(out: &mut String, content: String) {
    out.push_str(&content);
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::report::{EvalContext, EvalReport, GroupStats, LatencySummary};
    use crate::svg::{HISTORY_SVG_MAX_POINTS, render_svg};

    fn report(mode: &str, generated_at: i64) -> EvalReport {
        EvalReport {
            mode: mode.to_owned(),
            generated_at,
            latency: None,
            context: None,
            groups: BTreeMap::new(),
            totals: GroupStats::default(),
        }
    }

    fn sampled_report(mode: &str, generated_at: i64) -> EvalReport {
        EvalReport {
            latency: Some(LatencySummary {
                count: 2,
                p50_ms: 100.0,
                p95_ms: 200.0,
            }),
            ..report(mode, generated_at)
        }
    }

    fn with_commit(sampled: &EvalReport, commit: String) -> EvalReport {
        let mut owned = sampled.clone();
        owned.context = Some(EvalContext {
            commit: Some(commit),
            source: None,
        });
        owned
    }

    #[test]
    fn panels_follow_available_latency_samples() {
        let empty = render_svg(&[]);
        assert!(!empty.contains("<polyline"), "no series without points");
        assert!(
            empty.contains("No eval history yet"),
            "empty history gets a note"
        );
        assert!(
            !empty.contains("latest"),
            "no latest-run note without points"
        );

        let unsampled = render_svg(&[report("iterate", 1000)]);
        assert!(
            !unsampled.contains("end-to-end latency (ms)"),
            "the latency panel stays hidden without samples"
        );

        let sampled = render_svg(&[sampled_report("iterate", 2000)]);
        assert!(
            sampled.contains("end-to-end latency (ms)"),
            "the latency panel draws with samples"
        );
        assert!(sampled.contains(">p50<") && sampled.contains(">p95<"));
    }

    #[test]
    fn panels_render_polylines_ticks_and_legends() {
        let svg = render_svg(&[
            sampled_report("iterate", 1000),
            sampled_report("iterate", 2000),
        ]);
        assert!(svg.contains("<polyline"), "polylines drawn");
        assert!(svg.contains("<circle"), "point dots drawn");
        assert!(
            svg.contains("<title>p50 · 01-01 00:16 iterate: "),
            "hover titles carry the UTC time and mode: {svg}"
        );
        assert!(svg.contains("100.00 ms"), "hover values carry two decimals");
        assert!(svg.contains("stroke=\"#e7eaee\""), "grid lines present");
        assert!(
            svg.contains("text-anchor=\"middle\""),
            "x axis ticks present"
        );
        assert!(svg.contains(">p50<") && svg.contains(">p95<"));
        assert!(
            svg.contains("fill=\"none\" stroke=\"#cdd2da\""),
            "panel frames present"
        );
        assert!(
            svg.contains("latest 1970-01-01 00:33 UTC · iterate"),
            "the header names the latest run"
        );
    }

    #[test]
    fn missing_samples_break_the_series_instead_of_inventing_points() {
        let svg = render_svg(&[sampled_report("iterate", 1000), report("iterate", 2000)]);
        assert_eq!(
            svg.matches("<circle").count(),
            2,
            "each series dots only its sampled point: {svg}"
        );
        assert!(
            !svg.contains("<polyline"),
            "a single-point segment draws no line"
        );
    }

    #[test]
    fn text_is_xml_escaped() {
        let mode_report = sampled_report("a<b>&c", 0);
        let committed = with_commit(&sampled_report("iterate", 0), "x&<y".to_owned());
        let svg = render_svg(&[mode_report, committed]);
        assert!(
            svg.contains("a&lt;b&gt;&amp;c"),
            "mode text escapes to entities"
        );
        assert!(
            svg.contains(" (x&amp;&lt;y)"),
            "commit text escapes to entities"
        );
        assert!(
            !svg.contains(">x&<y"),
            "raw angle brackets never land in text"
        );
    }

    #[test]
    fn rendering_is_byte_deterministic() {
        let history = vec![sampled_report("iterate", 1000), report("iterate", 2000)];
        assert_eq!(
            render_svg(&history),
            render_svg(&history),
            "the same input renders byte-identically"
        );
    }

    #[test]
    fn beyond_max_points_only_latest_are_drawn() {
        let history: Vec<EvalReport> = (0..HISTORY_SVG_MAX_POINTS + 20)
            .map(|index| with_commit(&sampled_report("iterate", 1000), format!("c{index:03}")))
            .collect();
        let svg = render_svg(&history);
        assert!(
            svg.contains("(100 points)"),
            "the title counts rendered points"
        );
        assert!(
            svg.contains(" (c119)"),
            "the newest point is drawn in hovers"
        );
        assert!(
            !svg.contains(" (c019)"),
            "the oldest point falls out of the window"
        );
    }

    #[test]
    fn x_ticks_sample_at_most_four_points() {
        let eight: Vec<EvalReport> = (0..8)
            .map(|index| sampled_report("iterate", 1000 * (index as i64 + 1)))
            .collect();
        let svg = render_svg(&eight);
        for tick in ["01-01 00:16", "01-01 00:50", "01-01 01:40", "01-01 02:13"] {
            assert!(svg.contains(&format!(">{tick}<")), "tick {tick} expected");
        }
        assert!(
            !svg.contains(">01-01 00:33<"),
            "intermediate points stay unlabeled"
        );

        let two = render_svg(&[
            sampled_report("iterate", 1000),
            sampled_report("iterate", 2000),
        ]);
        assert!(
            two.contains(">01-01 00:16<") && two.contains(">01-01 00:33<"),
            "short history labels all points"
        );
    }

    #[test]
    fn ticks_carry_timestamps_and_hover_carries_commit() {
        let committed = with_commit(&sampled_report("iterate", 0), "ec96217".to_owned());
        let local = sampled_report("local-run", 0);
        let svg = render_svg(&[committed, local]);
        assert!(svg.contains(">01-01 00:00<"), "ticks are UTC timestamps");
        assert!(
            svg.contains("01-01 00:00 iterate (ec96217):"),
            "committed hovers carry the SHA"
        );
        assert!(
            svg.contains("01-01 00:00 local-run:"),
            "local hovers carry mode only"
        );
        assert!(
            !svg.contains("local-run ("),
            "local points never fake a commit"
        );
        assert!(
            svg.contains("latest 1970-01-01 00:00 UTC · local-run"),
            "the header names the latest run"
        );
    }

    #[test]
    fn header_shows_latest_run_timestamp() {
        let svg = render_svg(&[report("iterate", 1000), sampled_report("iterate", 2000)]);
        assert!(
            svg.contains("latest 1970-01-01 00:33 UTC · iterate"),
            "the header names the latest run"
        );

        let committed = with_commit(
            &sampled_report("iterate", 1_791_763_200),
            "fc244bc".to_owned(),
        );
        assert!(
            render_svg(&[committed]).contains("latest 2026-10-12 00:00 UTC · iterate (fc244bc)"),
            "the header carries full date and commit"
        );
    }
}
