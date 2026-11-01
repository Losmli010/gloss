//! 指标总览渲染：柱状图先行——每类型一组竖柱，契约有效、回归通过、
//! 标注合格三项合格率与模型均分各一柱（均 0–100 直读），0 画短桩、无
//! 数据不画柱、合计组以虚线分隔；其后为单数字指标表，每格只放一个数
//! 字、明细进悬停。全部单元格只读报告分组聚合，不重算。

use super::{ViewInput, esc, fmt};
use crate::report::{GroupStats, QualityStats};

const DISPLAY_ORDER: [&str; 5] = ["分类", "词卡", "句译", "代码", "通用"];
const TOTAL: &str = "合计";

pub(super) fn render(input: &ViewInput<'_>) -> String {
    let groups = ordered_groups(input);
    let mut out = String::from("<section class=\"dash\">\n");
    out.push_str(&bar_chart(&groups));
    out.push_str(
        "  <p class=\"vnote\">柱高按比例：三项合格率与模型均分均为 0–100；柱下标签对应指标；0 画短桩，无数据不画柱；悬停看口径</p>\n",
    );
    out.push_str(&metric_table(&groups, input));
    out.push_str("</section>\n");
    out
}

fn ordered_groups<'a>(input: &ViewInput<'a>) -> Vec<(String, &'a GroupStats)> {
    let mut groups: Vec<(String, &GroupStats)> = Vec::new();
    for name in DISPLAY_ORDER {
        if let Some(stats) = input.report.groups.get(name) {
            groups.push((name.to_owned(), stats));
        }
    }
    for (name, stats) in &input.report.groups {
        if !DISPLAY_ORDER.contains(&name.as_str()) {
            groups.push((name.clone(), stats));
        }
    }
    groups.push((TOTAL.to_owned(), &input.report.totals));
    groups
}

fn bar_chart(groups: &[(String, &GroupStats)]) -> String {
    let mut out = String::from("  <div class=\"vchart\">\n");
    for (name, stats) in groups {
        let sum = if name == TOTAL { " sum" } else { "" };
        out.push_str(&format!(
            "    <div class=\"vgroup{sum}\"><div class=\"vbars\">\n"
        ));
        out.push_str(&bar(
            "有效",
            "契约有效",
            Some(stats.valid_rate() * 100.0),
            false,
        ));
        out.push_str(&bar(
            "通过",
            "回归通过",
            Some(stats.regression_pass_rate() * 100.0),
            false,
        ));
        out.push_str(&bar(
            "合格",
            "标注合格",
            Some(stats.label_pass_rate() * 100.0),
            false,
        ));
        out.push_str(&bar(
            "模型",
            "模型均分",
            stats.quality.map(|quality| quality.overall.mean),
            true,
        ));
        out.push_str(&format!(
            "    </div><span class=\"vlabel\">{}</span></div>\n",
            esc(name)
        ));
    }
    out.push_str("  </div>\n");
    out
}

fn bar(short: &str, metric: &str, value: Option<f64>, gray: bool) -> String {
    let mut out = String::from("      <div class=\"vcol\">");
    match value {
        Some(value) => {
            let height = if value <= 0.0 {
                2
            } else {
                (value * 0.8).round().max(2.0) as i64
            };
            let tone = if gray { " gray" } else { "" };
            let hover = if gray {
                fmt(value)
            } else {
                format!("{}%", fmt(value))
            };
            out.push_str(&format!(
                "<span class=\"vnum\">{}</span><i class=\"vbar{tone}\" style=\"height:{height}px\" title=\"{metric} {hover}\"></i>",
                fmt(value)
            ));
        }
        None => out.push_str("<span class=\"vnum\">—</span>"),
    }
    out.push_str(&format!("<span class=\"vsub\">{short}</span></div>\n"));
    out
}

fn metric_table(groups: &[(String, &GroupStats)], input: &ViewInput<'_>) -> String {
    let mut out = String::from("  <div class=\"tchart\">\n    <div class=\"trow head\">");
    for head in [
        "类型",
        "例数",
        "契约有效",
        "回归通过",
        "标注合格",
        "模型",
        "准确",
        "完整",
        "表达",
        "呈现",
        "评审覆盖",
        "人机一致",
        "违约",
        "基线",
        "轮次",
    ] {
        out.push_str(&format!(
            "<span class=\"thead\" style=\"text-align:right\">{head}</span>"
        ));
    }
    out.push_str("</div>\n");
    for (name, stats) in groups {
        out.push_str(&format!(
            "    <div class=\"trow{}\">{}</div>\n",
            if name == TOTAL { " sum" } else { "" },
            table_row(name, stats, input)
        ));
    }
    out.push_str("  </div>\n");
    out
}

fn table_row(name: &str, stats: &GroupStats, input: &ViewInput<'_>) -> String {
    let mut out = format!("<span class=\"tname\">{}</span>", esc(name));
    out.push_str(&cell(stats.cases.to_string(), None));
    out.push_str(&rate_cells(stats));
    out.push_str(&quality_cells(stats));
    out.push_str(&cell(stats.contract.violations_total.to_string(), None));
    out.push_str(&cell(
        stats.golden_count.to_string(),
        Some(format!(
            "基线 {} · 标注 {} · 固化 {}",
            stats.golden_count,
            stats.label_count,
            latest_accepted_at(name, input).unwrap_or_default()
        )),
    ));
    out.push_str(&cell(fmt(stats.contract.repairs_mean), None));
    out
}

fn rate_cells(stats: &GroupStats) -> String {
    let mut out = cell(
        percent(stats.valid_rate()),
        Some(format!(
            "有效 {} · 软缺口 {} · 降级 {} · 故障 {}",
            stats.contract.valid,
            stats.contract.softgap,
            stats.contract.degraded,
            stats.contract.engine_error
        )),
    );
    out.push_str(&cell(
        percent(stats.regression_pass_rate()),
        Some(format!(
            "通过 {} · 回归 {} · 待复核 {} · 未改善 {} · 待比 {}",
            stats.rules.pass,
            stats.rules.regress,
            stats.rules.changed,
            stats.rules.unchanged,
            stats.rules.pending
        )),
    ));
    out.push_str(&cell(
        percent(stats.label_pass_rate()),
        Some(format!(
            "合格 {} · 不合格 {} · 未标注 {}",
            stats.labels.pass, stats.labels.fail, stats.labels.unlabeled
        )),
    ));
    out
}

fn quality_cells(stats: &GroupStats) -> String {
    let mut out = String::new();
    match &stats.quality {
        Some(quality) => {
            out.push_str(&cell(fmt(quality.overall.mean), Some(dims_note(quality))));
            for dim in &quality.dims {
                out.push_str(&cell(fmt(dim.mean), None));
            }
            out.push_str(&cell(
                percent(quality.coverage_rate),
                Some(format!("评分 {}/{}", quality.overall.count, stats.cases)),
            ));
            out.push_str(&cell(percent(quality.agree_rate), None));
        }
        None => {
            for _ in 0..7 {
                out.push_str(&cell("—".to_owned(), None));
            }
        }
    }
    out
}

fn dims_note(quality: &QualityStats) -> String {
    format!(
        "四维：准确 {} · 完整 {} · 表达 {} · 呈现 {}",
        fmt(quality.dims[0].mean),
        fmt(quality.dims[1].mean),
        fmt(quality.dims[2].mean),
        fmt(quality.dims[3].mean)
    )
}

fn latest_accepted_at(name: &str, input: &ViewInput<'_>) -> Option<String> {
    input
        .golden
        .values()
        .filter(|row| {
            input.cases.iter().any(|pair| {
                pair.detail.id == row.id
                    && (name == TOTAL || pair.detail.kind.display_name() == name)
            })
        })
        .map(|row| row.accepted_at.as_str())
        .max()
        .map(str::to_owned)
}

fn cell(value: String, title: Option<String>) -> String {
    match title {
        Some(title) => format!(
            "<span class=\"tnum\" title=\"{}\">{value}</span>",
            super::esc(&title)
        ),
        None => format!("<span class=\"tnum\">{value}</span>"),
    }
}

fn percent(rate: f64) -> String {
    format!("{}%", fmt(rate * 100.0))
}

#[cfg(test)]
mod tests {
    use gloss_core::task::TaskKind;

    use super::super::tests::{case, detail, golden_row, input, pair, scored};
    use crate::asset::{GoldenMap, LabelMap};
    use crate::detail::{ContractState, EvalKind, LabelVerdict};
    use crate::report::assemble;

    #[test]
    fn bars_render_present_groups_totals_and_zero_stubs() {
        let classify = EvalKind::Classify;
        let word = EvalKind::Task(TaskKind::TranslateWord);
        let softgap = case("cls-1", &classify);
        let mut softgap_detail = detail("cls-1", &classify);
        softgap_detail.contract = super::super::tests::outcome(ContractState::Softgap);
        let scored_case = case("task-word-01", &word);
        let mut scored_detail = detail("task-word-01", &word);
        scored(&mut scored_detail, [90, 80, 70, 60]);
        let pairs = vec![
            pair(&softgap, &softgap_detail),
            pair(&scored_case, &scored_detail),
        ];
        let html = super::render(&input(
            &assemble(
                &pairs
                    .iter()
                    .map(|pair| pair.detail.clone())
                    .collect::<Vec<_>>(),
                &GoldenMap::new(),
                &LabelMap::new(),
                None,
            ),
            &pairs,
            &LabelMap::new(),
            &GoldenMap::new(),
        ));
        assert!(
            html.contains("<span class=\"vlabel\">分类</span>")
                && html.contains("<span class=\"vlabel\">词卡</span>"),
            "present groups render in the fixed order: {html}"
        );
        assert!(
            html.contains("class=\"vgroup sum\"")
                && html.contains("<span class=\"vlabel\">合计</span>"),
            "the totals group trails behind a dashed separator"
        );
        assert!(
            html.contains("style=\"height:2px\" title=\"契约有效 0%\""),
            "a zero rate draws a stub: {html}"
        );
        assert!(
            html.contains("style=\"height:80px\" title=\"契约有效 100%\""),
            "a full rate draws the full column"
        );
        assert!(
            html.contains("vbar gray") && html.contains("title=\"模型均分 75\""),
            "the model column draws gray with the group mean"
        );
    }

    #[test]
    fn model_column_holds_a_placeholder_without_quality() {
        let classify = EvalKind::Classify;
        let eval_case = case("cls-1", &classify);
        let classify_detail = detail("cls-1", &classify);
        let pairs = vec![pair(&eval_case, &classify_detail)];
        let html = super::render(&input(
            &assemble(
                std::slice::from_ref(&classify_detail),
                &GoldenMap::new(),
                &LabelMap::new(),
                None,
            ),
            &pairs,
            &LabelMap::new(),
            &GoldenMap::new(),
        ));
        assert!(
            html.contains("<span class=\"vnum\">—</span><span class=\"vsub\">模型</span>"),
            "no quality stats, no model bar: {html}"
        );
    }

    fn hover_fixture_html() -> String {
        let word = EvalKind::Task(TaskKind::TranslateWord);
        let classify = EvalKind::Classify;
        let mut word_detail = detail("task-word-01", &word);
        scored(&mut word_detail, [90, 80, 70, 60]);
        word_detail.rule = Some(super::super::tests::rule(
            crate::detail::RuleVerdict::Pass,
            None,
        ));
        let classify_detail = detail("cls-1", &classify);
        let word_case = case("task-word-01", &word);
        let classify_case = case("cls-1", &classify);
        let pairs = vec![
            pair(&word_case, &word_detail),
            pair(&classify_case, &classify_detail),
        ];
        let golden = GoldenMap::from([golden_row("task-word-01")]);
        let labels = LabelMap::from([super::super::tests::labeled(
            "task-word-01",
            LabelVerdict::Pass,
            None,
        )]);
        let details: Vec<_> = pairs.iter().map(|pair| pair.detail.clone()).collect();
        let report = assemble(&details, &golden, &labels, None);
        super::render(&input(&report, &pairs, &labels, &golden))
    }

    #[test]
    fn table_cells_carry_hovers_placeholders_and_baseline_time() {
        let html = hover_fixture_html();
        assert!(
            html.contains("title=\"有效 1 · 软缺口 0 · 降级 0 · 故障 0\""),
            "contract rates carry their tally: {html}"
        );
        assert!(
            html.contains("title=\"通过 1 · 回归 0 · 待复核 0 · 未改善 0 · 待比 1\""),
            "regression rates carry the five-state tally"
        );
        assert!(
            html.contains("title=\"合格 1 · 不合格 0 · 未标注 1\""),
            "label rates carry the three-state tally"
        );
        assert!(
            html.contains("title=\"四维：准确 90 · 完整 80 · 表达 70 · 呈现 60\""),
            "the model cell hover lists the four dimension means"
        );
        assert!(
            html.contains("title=\"评分 1/1\""),
            "coverage hover carries the judged-over-total tally"
        );
        assert!(
            html.contains("title=\"基线 1 · 标注 1 · 固化 2026-11-01T10:00:00Z\""),
            "the baseline cell hover carries counts and acceptance time"
        );
        assert!(
            html.contains("<span class=\"tnum\">—</span>"),
            "qualityless groups show placeholders"
        );
    }

    #[test]
    fn totals_row_folds_the_groups_under_a_dashed_rule() {
        let word = EvalKind::Task(TaskKind::TranslateWord);
        let eval_case = case("task-word-01", &word);
        let word_detail = detail("task-word-01", &word);
        let pairs = vec![pair(&eval_case, &word_detail)];
        let html = super::render(&input(
            &assemble(
                std::slice::from_ref(&word_detail),
                &GoldenMap::new(),
                &LabelMap::new(),
                None,
            ),
            &pairs,
            &LabelMap::new(),
            &GoldenMap::new(),
        ));
        let totals = html
            .split("<div class=\"trow sum\">")
            .nth(1)
            .expect("totals row");
        assert!(totals.contains("<span class=\"tname\">合计</span>"));
        assert!(
            totals.contains("<span class=\"tnum\">1</span>"),
            "cases fold in: {totals}"
        );
    }
}
