//! 评测趋势 SVG 渲染：渲染入口是纯函数——入参只认报告切片，不读
//! 环境、不落盘；输出确定性字节（无渲染时刻的时间戳、浮点坐标固定两位
//! 小数、遍历次序稳定），同输入两次渲染逐字节一致，入库后 PR 的差异即
//! 数据差异。
//!
//! 图面：960 宽白底、面板纵向堆叠（标题 + 内联图例、细网格与刻度、
//! 外框、折线 + 圆点、横轴稀疏刻度）；横轴刻度是各点的 UTC 时间
//! （月-日 时:分），模式与提交标识进悬浮提示，右上角固定标注
//! 最近一次运行；任务轨按数据集各一面板，修复成本逐数据集出线。缺样
//! 本断线跳段不造点。
//!
//! 分工：本文件只留画布组装（头部、高度、点窗）与发射原语，坐标轴子模块管
//! 值域与网格口径，面板子模块管面板与序列的数据建模，落墨子模块按图形元素
//! 落墨。

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
const COLOR_GREEN: &str = "#2f9e57";
const COLOR_PINK: &str = "#d64c7f";

const COST_COLORS: [&str; 3] = [COLOR_ORANGE, COLOR_GREEN, COLOR_PINK];

const FONT_STACK: &str = "-apple-system,'Segoe UI',sans-serif";
const FOOTNOTE: &str = "口径：replay 轨度量生产校验器对固化夹具的判定，live 轨为模型现状；数据源 history.jsonl，仅绘最近 100 点，时间一律 UTC。";
const EMPTY_NOTE: &str = "暂无评测历史。";

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
            "<text x=\"46\" y=\"26\" font-size=\"14\" font-weight=\"600\" fill=\"{COLOR_TEXT}\">Prompt 评测趋势（{count} 点）</text>"
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
    use crate::metrics::{ClassifyMetrics, Latency, TaskMetrics};
    use crate::report::{
        ClassifyStats, EvalContext, EvalReport, JudgeSummary, TaskStats, TrackOutputs,
        assemble_report,
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
        latency: Option<&mut Latency>,
        judge: Option<JudgeSummary>,
    ) -> EvalReport {
        let mut report = assemble_report(
            mode,
            TrackOutputs {
                classify: Some(classify_metrics()),
                tasks: vec![("task_translate_word", task_metrics())],
                latency,
                judge,
            },
            0,
        );
        report.generated_at = generated_at;
        report
    }

    fn replay_report(generated_at: i64) -> EvalReport {
        report_with("replay", generated_at, None, None)
    }

    fn live_report(generated_at: i64) -> EvalReport {
        let mut latency = latency();
        report_with(
            "live",
            generated_at,
            Some(&mut latency),
            Some(JudgeSummary {
                count: 2,
                attempted: 2,
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
