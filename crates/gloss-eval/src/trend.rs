//! 评测趋势矢量图（SVG）渲染：曲线渲染唯一出口——检视页历史区块与
//! 命令行历史重绘（eval history）共用本入口，序列清单只有一份：契约
//! 有效率、回归通过率、标注合格率、模型总体均分（指标汇总，0–100 定
//! 标）与中位延迟、95 分位延迟（延迟），各取近五次迭代运行，不足五次
//! 全绘。
//!
//! 渲染入口是纯函数——入参只认报告切片，不读环境、不落盘；输出确定
//! 性字节（无渲染时刻的时间戳、浮点坐标固定两位小数、遍历次序稳定），
//! 同输入两次渲染逐字节一致，入库后拉取请求（PR）的差异即数据差异。
//! 图面：白底单画布、两面板纵向堆叠（小节标题与内联图例、25% 步进网
//! 格、外框、折线与圆点、横轴日期刻度）；每个数据点带圆点，最新一轮
//! 圆点放大并标注末值；模型线缺该轮（评审未跑）断线跳段，延迟缺样本
//! 同样不造点。曲线用色与检视页徽标四色同源，色值常量在本文件定义。
//!
//! 分工：本文件只留画布组装（头部、高度、窗口）与发射原语，坐标轴
//! 子模块管值域与网格口径，面板子模块管面板与序列的数据建模，落墨
//! 子模块按图形元素落墨。

mod axis;
mod draw;
mod panel;

use crate::report::{EvalReport, ITERATE_MODE, format_utc};

pub const TREND_MAX_RUNS: usize = 5;

pub const WIDTH: usize = 960;
const PLOT_LEFT: f64 = 46.0;
const PLOT_RIGHT: f64 = 946.0;
const X_INSET: f64 = 14.0;
const TITLE_Y: usize = 26;
const FIRST_TITLE_Y: usize = 56;
const PANEL_PITCH: usize = 188;
const FRAME_TOP_OFFSET: usize = 8;
const FRAME_HEIGHT: usize = 132;
const TICK_LABEL_DY: usize = 16;
const BOTTOM_PAD: usize = 14;
const EMPTY_HEIGHT: usize = 80;
const MAX_X_TICKS: usize = TREND_MAX_RUNS;
const LEGEND_FONT_SIZE: f64 = 12.0;
const SWATCH_SIZE: usize = 10;
const SWATCH_BASELINE_OFFSET: usize = 9;
const SWATCH_TEXT_GAP: f64 = 4.0;
const LEGEND_ITEM_GAP: f64 = 30.0;

pub const COLOR_GREEN: &str = "#2f9e57";
pub const COLOR_AMBER: &str = "#b98212";
pub const COLOR_RED: &str = "#d64545";
pub const COLOR_GRAY: &str = "#7a828c";
pub const COLOR_TEXT: &str = "#24292f";
pub const COLOR_MUTED: &str = "#5b6470";

const COLOR_GRID: &str = "#e7eaee";
const COLOR_FRAME: &str = "#cdd2da";
const FONT_STACK: &str = "-apple-system,'Segoe UI',sans-serif";
const EMPTY_NOTE: &str = "暂无评测历史";

pub fn render_svg(history: &[EvalReport]) -> String {
    let points = latest_runs(history);
    let data = draw::PointData::new(&points);
    let panels = panel::build_panels(&points);
    let height = svg_height(panels.len());
    let mut out = String::new();
    write_header(
        &mut out,
        height,
        points.len(),
        points.last().copied(),
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
            "<text x=\"46\" y=\"{TITLE_Y}\" font-size=\"14\" font-weight=\"600\" fill=\"{COLOR_TEXT}\">评测趋势（{count} 轮）</text>"
        ),
    );
    if let Some(latest) = latest {
        line(
            out,
            format!(
                "<text x=\"946\" y=\"{TITLE_Y}\" font-size=\"11\" fill=\"{COLOR_MUTED}\" text-anchor=\"end\">{}</text>",
                esc(&latest_note(latest))
            ),
        );
    }
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
    let mut note = format!("最近一轮 {}", format_utc(report.generated_at));
    if let Some(commit) = report
        .context
        .as_ref()
        .and_then(|context| context.commit.as_ref())
    {
        note.push_str(&format!(" ({commit})"));
    }
    note
}

fn latest_runs(history: &[EvalReport]) -> Vec<&EvalReport> {
    let runs: Vec<&EvalReport> = history
        .iter()
        .filter(|run| run.mode == ITERATE_MODE)
        .collect();
    let skip = runs.len().saturating_sub(TREND_MAX_RUNS);
    runs.into_iter().skip(skip).collect()
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

    use crate::report::{
        ContractStats, DimStat, EvalContext, EvalReport, GroupStats, LabelStats, LatencySummary,
        QualityStats, RuleCounts,
    };

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
                p50_ms: 1000.0,
                p95_ms: 3200.0,
            }),
            ..report(mode, generated_at)
        }
    }

    fn rates_report(generated_at: i64) -> EvalReport {
        let mut owned = report("iterate", generated_at);
        owned.totals = GroupStats {
            cases: 8,
            contract: ContractStats {
                valid: 5,
                ..Default::default()
            },
            rules: RuleCounts {
                pass: 3,
                ..Default::default()
            },
            labels: LabelStats {
                pass: 4,
                ..Default::default()
            },
            ..Default::default()
        };
        owned
    }

    fn with_quality(base: EvalReport, mean: f64) -> EvalReport {
        let mut owned = base;
        let dim = DimStat {
            count: 1,
            mean,
            buckets: [0; 5],
            pass_rate: 1.0,
        };
        owned.totals.quality = Some(QualityStats {
            coverage_rate: 1.0,
            agree_rate: 1.0,
            dims: std::array::from_fn(|_| dim),
            overall: dim,
        });
        owned
    }

    fn with_commit(base: &EvalReport, commit: String) -> EvalReport {
        let mut owned = base.clone();
        owned.context = Some(EvalContext {
            commit: Some(commit),
            source: None,
        });
        owned
    }

    #[test]
    fn empty_history_renders_note_without_series() {
        let empty = super::render_svg(&[]);
        assert!(!empty.contains("<polyline"), "no series without points");
        assert!(empty.contains("暂无评测历史"), "empty history gets a note");
        assert!(
            !empty.contains("最近一轮"),
            "no latest-run note without points"
        );

        let unsampled = super::render_svg(&[report("iterate", 1000)]);
        assert!(
            !unsampled.contains(">延迟</text>"),
            "the latency panel stays hidden without samples"
        );
    }

    #[test]
    fn summary_panel_draws_the_three_rates_on_a_percent_scale() {
        let svg = super::render_svg(&[rates_report(1000)]);
        assert!(svg.contains("指标汇总"), "summary panel title present");
        assert!(
            svg.contains(">契约有效率<")
                && svg.contains(">回归通过率<")
                && svg.contains(">标注合格率<"),
            "the three rate series legends are present: {svg}"
        );
        for label in ["100%", "75%", "50%", "25%", "0%"] {
            assert!(
                svg.contains(&format!(">{label}<")),
                "gridline {label} expected"
            );
        }
        assert!(
            svg.contains(">62.5%<") && svg.contains(">37.5%<") && svg.contains(">50%<"),
            "the newest rates carry end labels: {svg}"
        );
    }

    #[test]
    fn model_series_rides_judged_runs_only() {
        let first = with_quality(rates_report(1000), 78.0);
        let second = with_quality(rates_report(1500), 60.0);
        let dashed = super::render_svg(&[first, second]);
        assert!(dashed.contains(">模型总体<"), "model series legend present");
        assert!(
            dashed.contains("stroke-dasharray=\"4 3\""),
            "the derived-mean series draws dashed: {dashed}"
        );

        let gap_at_end =
            super::render_svg(&[with_quality(rates_report(1000), 78.0), rates_report(2000)]);
        assert!(
            !gap_at_end.contains(">78<") && !gap_at_end.contains(">60<"),
            "a series gap at the newest run carries no end label"
        );

        let tail = super::render_svg(&[rates_report(1000), with_quality(rates_report(2000), 78.0)]);
        assert!(tail.contains(">78<"), "the newest judged mean is labeled");

        let unjudged = super::render_svg(&[rates_report(1000)]);
        assert!(
            !unjudged.contains(">模型总体<"),
            "a history without judged runs draws no model series"
        );
    }

    #[test]
    fn latency_panel_draws_only_with_samples() {
        let sampled = super::render_svg(&[sampled_report("iterate", 2000)]);
        assert!(
            sampled.contains(">延迟</text>"),
            "latency panel title present"
        );
        assert!(sampled.contains(">p50<") && sampled.contains(">p95<"));
        assert!(sampled.contains(">3.2s<"), "the newest p95 is labeled");
    }

    #[test]
    fn missing_samples_break_the_series_instead_of_inventing_points() {
        let svg = super::render_svg(&[sampled_report("iterate", 1000), report("iterate", 2000)]);
        assert_eq!(
            svg.matches("<circle").count(),
            8,
            "three rate series dot both runs, latency dots only the sampled run: {svg}"
        );
    }

    #[test]
    fn newest_point_is_enlarged_and_labeled() {
        let svg = super::render_svg(&[rates_report(1000), rates_report(2000)]);
        assert!(
            svg.contains("r=\"4\""),
            "the newest run draws an enlarged dot"
        );
        assert_eq!(
            svg.matches("r=\"4\"").count(),
            3,
            "three always-present rates label their newest value: {svg}"
        );
    }

    #[test]
    fn only_recent_five_iterate_runs_are_drawn() {
        let history: Vec<EvalReport> = (0..7)
            .map(|index| {
                with_commit(
                    &rates_report(1000 * index as i64 + 1),
                    format!("c{index:03}"),
                )
            })
            .collect();
        let svg = super::render_svg(&history);
        assert!(
            svg.contains("评测趋势（5 轮）"),
            "the title counts drawn runs"
        );
        assert!(
            svg.contains(" (c006)"),
            "the newest commit is drawn in hovers"
        );
        assert!(
            !svg.contains(" (c001)"),
            "the oldest run falls out of the window"
        );
    }

    #[test]
    fn non_iterate_rows_stay_out_of_the_window() {
        let history = vec![
            report("sidecar", 500),
            rates_report(1000),
            sampled_report("sidecar", 1500),
            rates_report(2000),
        ];
        let svg = super::render_svg(&history);
        assert!(svg.contains("评测趋势（2 轮）"), "only iterate runs count");
        assert!(
            !svg.contains("sidecar"),
            "other modes neither draw nor annotate"
        );
    }

    #[test]
    fn latency_axis_picks_a_quarter_divisible_ceiling() {
        let mut sampled = sampled_report("iterate", 1000);
        sampled.latency = Some(LatencySummary {
            count: 2,
            p50_ms: 900.0,
            p95_ms: 1500.0,
        });
        let svg = super::render_svg(&[sampled]);
        for label in ["1.6s", "1.2s", "800ms", "400ms", "0ms"] {
            assert!(
                svg.contains(&format!(">{label}<")),
                "gridline {label} expected: {svg}"
            );
        }
    }

    #[test]
    fn text_is_xml_escaped() {
        let committed = with_commit(&rates_report(0), "x&<y".to_owned());
        let svg = super::render_svg(&[committed]);
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
        let history = vec![rates_report(1000), sampled_report("iterate", 2000)];
        assert_eq!(
            super::render_svg(&history),
            super::render_svg(&history),
            "the same input renders byte-identically"
        );
    }
}
