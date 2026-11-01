//! 评测报告：聚合类型与聚合计算的唯一归属——逐例明细按类型展示名分组
//! 聚合、合计行按全体用例一次重算，连同报告渲染（Markdown）与报告落盘。
//! 三率分母恒为组内全部用例（待比、未标注、故障不剔除，跨轮可比）；评
//! 审覆盖率与人机一致率各按自身分母派生；四维分数按五档分桶；质量聚合
//! 在组内无已评用例时置空。历史行即本类型一行一份（JSON），序列化形状
//! 为（serde）默认命名。

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::asset::{GoldenMap, LabelMap};
use crate::contract::derive_state;
use crate::detail::{CaseDetail, ContractState, JudgeScores, LabelVerdict, RuleVerdict};

pub const ITERATE_MODE: &str = "iterate";
const PASS_LINE: f64 = 80.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalReport {
    pub mode: String,
    pub generated_at: i64,
    #[serde(default)]
    pub context: Option<EvalContext>,
    pub groups: BTreeMap<String, GroupStats>,
    pub totals: GroupStats,
    pub latency: Option<LatencySummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalContext {
    pub commit: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LatencySummary {
    pub count: usize,
    pub p50_ms: f64,
    pub p95_ms: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupStats {
    pub cases: usize,
    pub golden_count: usize,
    pub label_count: usize,
    pub contract: ContractStats,
    pub rules: RuleCounts,
    pub labels: LabelStats,
    pub quality: Option<QualityStats>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct ContractStats {
    pub valid: usize,
    pub softgap: usize,
    pub degraded: usize,
    pub engine_error: usize,
    pub violations_total: usize,
    pub repairs_mean: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleCounts {
    pub pending: usize,
    pub pass: usize,
    pub regress: usize,
    pub changed: usize,
    pub unchanged: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelStats {
    pub pass: usize,
    pub fail: usize,
    pub unlabeled: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct QualityStats {
    pub coverage_rate: f64,
    pub agree_rate: f64,
    pub dims: [DimStat; 4],
    pub overall: DimStat,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DimStat {
    pub count: usize,
    pub mean: f64,
    pub buckets: [usize; 5],
    pub pass_rate: f64,
}

impl GroupStats {
    pub fn valid_rate(&self) -> f64 {
        ratio(self.contract.valid as f64, self.cases as f64)
    }

    pub fn regression_pass_rate(&self) -> f64 {
        ratio(self.rules.pass as f64, self.cases as f64)
    }

    pub fn label_pass_rate(&self) -> f64 {
        ratio(self.labels.pass as f64, self.cases as f64)
    }
}

pub fn assemble(
    cases: &[CaseDetail],
    golden: &GoldenMap,
    labels: &LabelMap,
    latency: Option<LatencySummary>,
) -> EvalReport {
    let mut grouped: BTreeMap<&str, Vec<&CaseDetail>> = BTreeMap::new();
    for case in cases {
        grouped
            .entry(case.kind.display_name())
            .or_default()
            .push(case);
    }
    let groups = grouped
        .into_iter()
        .map(|(name, group)| (name.to_owned(), group_stats(&group, golden, labels)))
        .collect();
    let all: Vec<&CaseDetail> = cases.iter().collect();
    EvalReport {
        mode: ITERATE_MODE.to_owned(),
        generated_at: now_secs(),
        context: None,
        groups,
        totals: group_stats(&all, golden, labels),
        latency,
    }
}

fn group_stats(cases: &[&CaseDetail], golden: &GoldenMap, labels: &LabelMap) -> GroupStats {
    let mut stats = GroupStats {
        cases: cases.len(),
        golden_count: 0,
        label_count: 0,
        contract: ContractStats::default(),
        rules: RuleCounts::default(),
        labels: LabelStats::default(),
        quality: None,
    };
    for case in cases {
        tally_contract(case, &mut stats.contract);
        if let Some(rule) = &case.rule {
            tally_rule(rule.verdict, &mut stats.rules);
        }
        if golden.contains_key(&case.id) {
            stats.golden_count += 1;
        }
        tally_label(
            labels.get(&case.id).map(|row| row.verdict),
            &mut stats.labels,
        );
    }
    stats.label_count = stats.labels.pass + stats.labels.fail;
    let repairs: usize = cases
        .iter()
        .map(|case| case.contract.repairs as usize)
        .sum();
    stats.contract.repairs_mean = ratio(repairs as f64, cases.len() as f64);
    stats.quality = quality_stats(cases, labels, stats.cases);
    stats
}

fn tally_contract(case: &CaseDetail, stats: &mut ContractStats) {
    match derive_state(case.engine_error, &case.contract) {
        ContractState::Valid => stats.valid += 1,
        ContractState::Softgap => stats.softgap += 1,
        ContractState::Degraded => stats.degraded += 1,
        ContractState::EngineError => stats.engine_error += 1,
    }
    stats.violations_total += case.contract.violations.values().sum::<usize>();
}

fn tally_rule(verdict: RuleVerdict, counts: &mut RuleCounts) {
    match verdict {
        RuleVerdict::Pending => counts.pending += 1,
        RuleVerdict::Pass => counts.pass += 1,
        RuleVerdict::Regress => counts.regress += 1,
        RuleVerdict::Changed => counts.changed += 1,
        RuleVerdict::Unchanged => counts.unchanged += 1,
    }
}

fn tally_label(verdict: Option<LabelVerdict>, stats: &mut LabelStats) {
    match verdict {
        Some(LabelVerdict::Pass) => stats.pass += 1,
        Some(LabelVerdict::Fail) => stats.fail += 1,
        None => stats.unlabeled += 1,
    }
}

fn quality_stats(cases: &[&CaseDetail], labels: &LabelMap, total: usize) -> Option<QualityStats> {
    let scored: Vec<(&JudgeScores, Option<LabelVerdict>)> = cases
        .iter()
        .filter_map(|case| {
            let scores = case.quality.as_ref()?;
            Some((scores, labels.get(&case.id).map(|row| row.verdict)))
        })
        .collect();
    if scored.is_empty() {
        return None;
    }
    let agreed = scored
        .iter()
        .filter(|(scores, label)| agree(scores, *label))
        .count();
    let comparable = scored.iter().filter(|(_, label)| label.is_some()).count();
    let dim = |pick: fn(&JudgeScores) -> u8| {
        dim_stat(scored.iter().map(|(scores, _)| pick(scores) as f64))
    };
    Some(QualityStats {
        coverage_rate: ratio(scored.len() as f64, total as f64),
        agree_rate: ratio(agreed as f64, comparable as f64),
        dims: [
            dim(|scores| scores.accuracy),
            dim(|scores| scores.completeness),
            dim(|scores| scores.fluency),
            dim(|scores| scores.presentation),
        ],
        overall: dim_stat(scored.iter().map(|(scores, _)| overall(scores))),
    })
}

fn agree(scores: &JudgeScores, label: Option<LabelVerdict>) -> bool {
    let overall = overall(scores);
    match label {
        Some(LabelVerdict::Pass) => overall >= PASS_LINE,
        Some(LabelVerdict::Fail) => overall < PASS_LINE,
        None => false,
    }
}

fn overall(scores: &JudgeScores) -> f64 {
    (scores.accuracy as f64
        + scores.completeness as f64
        + scores.fluency as f64
        + scores.presentation as f64)
        / 4.0
}

fn dim_stat(scores: impl Iterator<Item = f64>) -> DimStat {
    let scores: Vec<f64> = scores.collect();
    let mut buckets = [0usize; 5];
    scores
        .iter()
        .for_each(|score| buckets[bucket_of(*score)] += 1);
    DimStat {
        count: scores.len(),
        mean: ratio(scores.iter().sum(), scores.len() as f64),
        buckets,
        pass_rate: ratio(
            scores.iter().filter(|score| **score >= PASS_LINE).count() as f64,
            scores.len() as f64,
        ),
    }
}

fn bucket_of(score: f64) -> usize {
    if score < 60.0 {
        0
    } else if score < 70.0 {
        1
    } else if score < 80.0 {
        2
    } else if score < 90.0 {
        3
    } else {
        4
    }
}

fn ratio(numerator: f64, denominator: f64) -> f64 {
    if denominator == 0.0 {
        0.0
    } else {
        numerator / denominator
    }
}

pub(crate) fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

const METRICS_TABLE_HEADER: &str = "| 类型 | 例数 | 契约有效 | 回归通过 | 标注合格 | 模型均分 | 评审覆盖 | 人机一致 | 违约 | 基线 | 轮次均值 |\n\
    | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n";

impl EvalReport {
    pub fn render_markdown(&self) -> String {
        let mut out = String::new();
        line(
            &mut out,
            format!(
                "# 评测报告：{}（{} UTC）",
                self.mode,
                format_utc(self.generated_at)
            ),
        );
        out.push('\n');
        out.push_str(METRICS_TABLE_HEADER);
        for (name, stats) in &self.groups {
            render_group_row(&mut out, name, stats);
        }
        render_group_row(&mut out, "合计", &self.totals);
        self.render_latency(&mut out);
        out
    }

    fn render_latency(&self, out: &mut String) {
        let Some(latency) = &self.latency else {
            return;
        };
        out.push('\n');
        out.push_str("| 分位 | 延迟（ms） |\n| --- | --- |\n");
        line(out, format!("| p50 | {:.2} |", latency.p50_ms));
        line(out, format!("| p95 | {:.2} |", latency.p95_ms));
    }
}

fn render_group_row(out: &mut String, name: &str, stats: &GroupStats) {
    let (model, coverage, agree) = match &stats.quality {
        Some(quality) => (
            fixed(quality.overall.mean),
            percent(quality.coverage_rate),
            percent(quality.agree_rate),
        ),
        None => ("—".to_owned(), "—".to_owned(), "—".to_owned()),
    };
    line(
        out,
        format!(
            "| {name} | {} | {} | {} | {} | {model} | {coverage} | {agree} | {} | {} | {} |",
            stats.cases,
            percent(stats.valid_rate()),
            percent(stats.regression_pass_rate()),
            percent(stats.label_pass_rate()),
            stats.contract.violations_total,
            stats.golden_count,
            fixed(stats.contract.repairs_mean),
        ),
    );
}

fn percent(rate: f64) -> String {
    format!("{:.1}%", rate * 100.0)
}

fn fixed(value: f64) -> String {
    format!("{value:.1}")
}

fn line(out: &mut String, content: String) {
    out.push_str(&content);
    out.push('\n');
}

pub fn format_utc(secs: i64) -> String {
    let (date, hour, minute) = utc_parts(secs);
    format!("{date} {hour:02}:{minute:02}")
}

pub fn format_utc_short(secs: i64) -> String {
    let (date, hour, minute) = utc_parts(secs);
    format!("{} {hour:02}:{minute:02}", &date[5..])
}

pub fn format_utc_rfc3339(secs: i64) -> String {
    let (date, hour, minute, second) = utc_stamp(secs);
    format!("{date}T{hour:02}:{minute:02}:{second:02}Z")
}

fn utc_parts(secs: i64) -> (String, u32, u32) {
    let (date, hour, minute, _) = utc_stamp(secs);
    (date, hour, minute)
}

fn utc_stamp(secs: i64) -> (String, u32, u32, u32) {
    let (year, month, day) = civil_from_days(secs.div_euclid(86_400));
    let day_secs = secs.rem_euclid(86_400);
    (
        format!("{year:04}-{month:02}-{day:02}"),
        (day_secs / 3_600) as u32,
        ((day_secs % 3_600) / 60) as u32,
        (day_secs % 60) as u32,
    )
}

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

pub fn write(report: &EvalReport, dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    serde_json::to_writer_pretty(
        std::io::BufWriter::new(std::fs::File::create(
            dir.join(format!("{}.json", report.mode)),
        )?),
        report,
    )
    .map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use gloss_core::schema::Violation;
    use gloss_core::task::TaskKind;

    use super::{EvalReport, assemble};
    use crate::asset::{GoldenMap, GoldenRow, LabelMap, LabelRow};
    use crate::detail::{
        CaseDetail, ContractCheck, ContractOutcome, ContractState, EvalKind, JudgeScores,
        LabelVerdict, RuleOutcome, RuleVerdict,
    };

    fn outcome(state: ContractState) -> ContractOutcome {
        ContractOutcome {
            json_object: state != ContractState::Degraded,
            checks: BTreeMap::from([(
                ContractCheck::FieldsComplete,
                state == ContractState::Valid,
            )]),
            degraded: state == ContractState::Degraded,
            violations: BTreeMap::new(),
            repairs: 0,
        }
    }

    fn detail(id: &str, kind: EvalKind, state: ContractState) -> CaseDetail {
        CaseDetail {
            id: id.to_owned(),
            kind,
            text: "一".to_owned(),
            rounds: vec![],
            contract: outcome(state),
            quality: None,
            rule: None,
            engine_error: state == ContractState::EngineError,
        }
    }

    fn rule(verdict: RuleVerdict) -> RuleOutcome {
        RuleOutcome {
            verdict,
            structured_match: None,
            summary: None,
        }
    }

    fn scores(four: [u8; 4]) -> JudgeScores {
        JudgeScores {
            accuracy: four[0],
            completeness: four[1],
            fluency: four[2],
            presentation: four[3],
            reason: "锚点理由".to_owned(),
        }
    }

    fn judged(id: &str, four: [u8; 4]) -> CaseDetail {
        let mut case = detail(
            id,
            EvalKind::Task(TaskKind::TranslateWord),
            ContractState::Valid,
        );
        case.quality = Some(scores(four));
        case
    }

    fn labeled(id: &str, verdict: LabelVerdict) -> (String, LabelRow) {
        (
            id.to_owned(),
            LabelRow {
                id: id.to_owned(),
                verdict,
                note: None,
            },
        )
    }

    fn empty_run() -> (Vec<CaseDetail>, GoldenMap, LabelMap) {
        (Vec::new(), GoldenMap::new(), LabelMap::new())
    }

    fn mixed_fixture() -> (Vec<CaseDetail>, GoldenMap, LabelMap) {
        let mut classify = detail("cls-1", EvalKind::Classify, ContractState::Softgap);
        classify.rule = Some(rule(RuleVerdict::Changed));
        let mut engine = detail("cls-2", EvalKind::Classify, ContractState::Valid);
        engine.engine_error = true;
        let mut valid = judged("task-word-1", [90, 80, 70, 60]);
        valid.rule = Some(rule(RuleVerdict::Pass));
        let mut degraded = detail(
            "task-sent-1",
            EvalKind::Task(TaskKind::TranslateSentence),
            ContractState::Degraded,
        );
        degraded.contract.violations = BTreeMap::from([(Violation::NotJson, 2)]);
        degraded.contract.repairs = 2;
        degraded.rule = Some(rule(RuleVerdict::Pending));
        let golden = GoldenMap::from([golden_row("task-word-1")]);
        let labels = LabelMap::from([
            labeled("cls-1", LabelVerdict::Fail),
            labeled("task-word-1", LabelVerdict::Pass),
        ]);
        (vec![classify, engine, valid, degraded], golden, labels)
    }

    fn assert_groups_fold_into_totals(report: &EvalReport) {
        let fold = |field: fn(&super::GroupStats) -> usize| {
            report.groups.values().map(field).sum::<usize>()
        };
        assert_eq!(fold(|g| g.cases), report.totals.cases);
        assert_eq!(fold(|g| g.golden_count), report.totals.golden_count);
        assert_eq!(fold(|g| g.label_count), report.totals.label_count);
        assert_eq!(fold(|g| g.contract.valid), report.totals.contract.valid);
        assert_eq!(fold(|g| g.contract.softgap), report.totals.contract.softgap);
        assert_eq!(
            fold(|g| g.contract.degraded),
            report.totals.contract.degraded
        );
        assert_eq!(
            fold(|g| g.contract.engine_error),
            report.totals.contract.engine_error
        );
        assert_eq!(
            fold(|g| g.contract.violations_total),
            report.totals.contract.violations_total
        );
        assert_eq!(fold(|g| g.rules.pending), report.totals.rules.pending);
        assert_eq!(fold(|g| g.rules.pass), report.totals.rules.pass);
        assert_eq!(fold(|g| g.rules.changed), report.totals.rules.changed);
        assert_eq!(fold(|g| g.labels.pass), report.totals.labels.pass);
        assert_eq!(fold(|g| g.labels.fail), report.totals.labels.fail);
        assert_eq!(fold(|g| g.labels.unlabeled), report.totals.labels.unlabeled);
        let judged: usize = report
            .groups
            .values()
            .filter_map(|group| group.quality.as_ref())
            .map(|quality| quality.overall.count)
            .sum();
        let total_judged = report
            .totals
            .quality
            .as_ref()
            .map(|quality| quality.overall.count)
            .unwrap_or(0);
        assert_eq!(judged, total_judged, "judged counts fold the same way");
    }

    #[test]
    fn group_aggregates_fold_into_totals() {
        let (cases, golden, labels) = mixed_fixture();
        let report = assemble(&cases, &golden, &labels, None);
        assert_groups_fold_into_totals(&report);

        assert_eq!(report.groups.len(), 3);
        assert_eq!(report.totals.cases, 4);
        assert_eq!(report.totals.contract.valid, 1);
        assert_eq!(report.totals.contract.softgap, 1);
        assert_eq!(report.totals.contract.degraded, 1);
        assert_eq!(report.totals.contract.engine_error, 1);
        assert_eq!(report.totals.contract.violations_total, 2);
        assert_eq!(
            report.totals.rules.pending, 1,
            "only the degraded case stays pending; engine errors carry no rule"
        );
        assert_eq!(report.totals.rules.pass, 1);
        assert_eq!(report.totals.rules.changed, 1);
        assert_eq!(report.totals.golden_count, 1);
        assert_eq!(report.totals.label_count, 2);
        assert_eq!(
            report.totals.labels,
            super::LabelStats {
                pass: 1,
                fail: 1,
                unlabeled: 2
            }
        );
        let quality = report
            .totals
            .quality
            .expect("a judged task case fills quality");
        assert_eq!(quality.overall.count, 1);
        assert_eq!(report.groups["分类"].quality, None);
        assert_eq!(
            report.totals.contract.repairs_mean, 0.5,
            "the mean divides the total repair rounds by every group case"
        );
        assert_eq!(report.groups["句译"].contract.repairs_mean, 2.0);
    }

    #[test]
    fn rate_denominators_are_all_group_cases() {
        let word = EvalKind::Task(TaskKind::TranslateWord);
        let mut valid = detail("task-1", word.clone(), ContractState::Valid);
        valid.rule = Some(rule(RuleVerdict::Pass));
        let pending = detail("task-2", word.clone(), ContractState::Softgap);
        let degraded = detail("task-3", word.clone(), ContractState::Degraded);
        let engine = detail("task-4", word, ContractState::EngineError);
        let labels = LabelMap::from([labeled("task-1", LabelVerdict::Pass)]);
        let report = assemble(
            &[valid, pending, degraded, engine],
            &GoldenMap::new(),
            &labels,
            None,
        );
        let stats = &report.totals;
        assert_eq!(stats.cases, 4);
        assert_eq!(
            stats.valid_rate(),
            0.25,
            "one valid case over all four, softgap and worse stay out of the numerator"
        );
        assert_eq!(
            stats.regression_pass_rate(),
            0.25,
            "pending and engine-error cases stay in the denominator"
        );
        assert_eq!(
            stats.label_pass_rate(),
            0.25,
            "unlabeled cases stay in the denominator"
        );
    }

    #[test]
    fn bucket_edges_split_at_59_60_69_70_79_80_89_90() {
        let edges = [59u8, 60, 69, 70, 79, 80, 89, 90];
        let cases: Vec<CaseDetail> = edges
            .iter()
            .enumerate()
            .map(|(index, accuracy)| {
                let mut case = detail(
                    &format!("task-{index}"),
                    EvalKind::Task(TaskKind::General),
                    ContractState::Valid,
                );
                case.quality = Some(scores([*accuracy, 50, 50, 50]));
                case
            })
            .collect();
        let report = assemble(&cases, &GoldenMap::new(), &LabelMap::new(), None);
        let quality = report.totals.quality.expect("judged cases fill quality");
        let accuracy = &quality.dims[0];
        assert_eq!(
            accuracy.buckets,
            [1, 2, 2, 2, 1],
            "59|60, 69|70, 79|80 and 89|90 each split adjacent tiers"
        );
        assert_eq!(accuracy.count, 8);
        assert_eq!(accuracy.mean, 74.5);
        assert_eq!(
            accuracy.pass_rate,
            3.0 / 8.0,
            "80, 89 and 90 clear the line, 79 does not"
        );
        let untouched = &quality.dims[1];
        assert_eq!(untouched.count, 8);
        assert_eq!(untouched.mean, 50.0);
        assert_eq!(untouched.buckets, [8, 0, 0, 0, 0]);
    }

    #[test]
    fn coverage_and_agreement_rates_use_their_own_denominators() {
        let cases = vec![
            judged("task-a", [90, 90, 90, 90]),
            judged("task-b", [90, 90, 90, 90]),
            judged("task-c", [50, 50, 50, 50]),
            judged("task-d", [50, 50, 50, 50]),
            detail(
                "task-e",
                EvalKind::Task(TaskKind::TranslateWord),
                ContractState::Valid,
            ),
        ];
        let labels = LabelMap::from([
            labeled("task-a", LabelVerdict::Pass),
            labeled("task-b", LabelVerdict::Fail),
            labeled("task-c", LabelVerdict::Fail),
            labeled("task-d", LabelVerdict::Pass),
            labeled("task-e", LabelVerdict::Pass),
        ]);
        let report = assemble(&cases, &GoldenMap::new(), &labels, None);
        let quality = report
            .totals
            .quality
            .expect("four judged cases fill quality");
        assert_eq!(
            quality.coverage_rate, 0.8,
            "coverage divides the four judged cases by all five group cases"
        );
        assert_eq!(
            quality.agree_rate, 0.5,
            "agreement divides by the labeled-and-judged four; the unlabeled-judged and labeled-unjudged cases stay out"
        );
    }

    #[test]
    fn quality_stays_empty_without_judged_cases() {
        let (_, golden, labels) = empty_run();
        let judge_off = vec![detail(
            "task-a",
            EvalKind::Task(TaskKind::TranslateWord),
            ContractState::Valid,
        )];
        let report = assemble(&judge_off, &golden, &labels, None);
        assert!(
            report.totals.quality.is_none(),
            "a judgeless run leaves quality empty"
        );

        let classify_only = vec![
            detail("cls-a", EvalKind::Classify, ContractState::Valid),
            detail("cls-b", EvalKind::Classify, ContractState::Valid),
        ];
        let report = assemble(&classify_only, &golden, &labels, None);
        assert!(
            report.groups["分类"].quality.is_none() && report.totals.quality.is_none(),
            "classify-only groups never enter the judge"
        );

        let mixed = vec![
            detail("cls-a", EvalKind::Classify, ContractState::Valid),
            judged("task-a", [90, 90, 90, 90]),
        ];
        let report = assemble(&mixed, &golden, &labels, None);
        assert_eq!(report.groups["分类"].quality, None);
        assert!(report.groups["词卡"].quality.is_some());
    }

    #[test]
    fn overall_score_is_the_four_dimension_mean_and_respects_the_pass_line() {
        let below = judged("task-a", [90, 80, 70, 60]);
        let at_line = judged("task-b", [80, 80, 80, 80]);
        let report = assemble(&[below, at_line], &GoldenMap::new(), &LabelMap::new(), None);
        let overall = &report.totals.quality.expect("judged").overall;
        assert_eq!(overall.count, 2);
        assert_eq!(overall.mean, 77.5, "75 and 80 average to 77.5");
        assert_eq!(
            overall.pass_rate, 0.5,
            "exactly 80 clears the line, 75 does not"
        );
        assert_eq!(overall.buckets, [0, 0, 1, 1, 0]);
    }

    #[test]
    fn markdown_renders_group_rows_and_totals() {
        let mut valid = judged("task-a", [90, 90, 90, 90]);
        valid.rule = Some(rule(RuleVerdict::Pass));
        let cases = vec![
            valid,
            detail("cls-a", EvalKind::Classify, ContractState::Valid),
        ];
        let labels = LabelMap::from([labeled("cls-a", LabelVerdict::Pass)]);
        let mut report = assemble(&cases, &GoldenMap::new(), &labels, None);
        report.generated_at = 1_791_763_200;
        let markdown = report.render_markdown();
        assert!(
            markdown.contains("# 评测报告：iterate（2026-10-12 00:00 UTC）"),
            "{markdown}"
        );
        assert!(
            markdown.contains("| 分类 | 1 | 100.0% | 0.0% | 100.0% | — | — | — | 0 | 0 | 0.0 |"),
            "a qualityless group renders placeholders: {markdown}"
        );
        assert!(
            markdown.contains(
                "| 词卡 | 1 | 100.0% | 100.0% | 0.0% | 90.0 | 100.0% | 0.0% | 0 | 0 | 0.0 |"
            ),
            "rates render as percentages and the model mean as a bare score: {markdown}"
        );
        assert!(markdown.contains("| 合计 | 2 |"), "{markdown}");
    }

    #[test]
    fn latency_only_shows_when_sampled() {
        let (cases, golden, labels) = empty_run();
        let mut unsampled = assemble(&cases, &golden, &labels, None);
        unsampled.generated_at = 0;
        let markdown = unsampled.render_markdown();
        assert!(
            !markdown.contains("延迟"),
            "no latency samples, no latency rows: {markdown}"
        );

        let (cases, golden, labels) = empty_run();
        let mut sampled = assemble(
            &cases,
            &golden,
            &labels,
            Some(super::LatencySummary {
                count: 2,
                p50_ms: 12.5,
                p95_ms: 40.0,
            }),
        );
        sampled.generated_at = 0;
        let markdown = sampled.render_markdown();
        assert!(
            markdown.contains("| p50 | 12.50 |") && markdown.contains("| p95 | 40.00 |"),
            "sampled percentiles render as table rows: {markdown}"
        );
    }

    #[test]
    fn generated_at_is_a_numeric_unix_timestamp() {
        let (cases, golden, labels) = empty_run();
        let mut owned = assemble(&cases, &golden, &labels, None);
        owned.generated_at = 1_791_763_200;
        let json = serde_json::to_string(&owned).expect("serialize");
        assert!(
            json.contains(r#""generated_at":1791763200"#),
            "the timestamp serializes as a bare number: {json}"
        );
        let parsed: EvalReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.generated_at, 1_791_763_200);
    }

    #[test]
    fn utc_formatting_is_fixed_and_timezone_free() {
        assert_eq!(super::format_utc(0), "1970-01-01 00:00");
        assert_eq!(super::format_utc(1_791_763_200), "2026-10-12 00:00");
        assert_eq!(
            super::format_utc(1_791_763_200 + 86_399),
            "2026-10-12 23:59"
        );
        assert_eq!(super::format_utc(-1), "1969-12-31 23:59");
        assert_eq!(super::format_utc_short(-86_400), "12-31 00:00");
        assert_eq!(super::format_utc_short(1_000), "01-01 00:16");
        assert_eq!(
            super::format_utc_rfc3339(1_791_763_200 + 3_723),
            "2026-10-12T01:02:03Z"
        );
    }

    #[test]
    fn legacy_rows_without_aggregates_fail_parsing_for_tolerant_skip() {
        let legacy = r#"{"mode":"replay","generated_at":1000,"classify":null,"tasks":[],"latency":null,"judge":null,"skipped":0}"#;
        assert!(
            serde_json::from_str::<EvalReport>(legacy).is_err(),
            "rows without group aggregates are rejected; history load skips them as bad rows"
        );
    }

    #[test]
    fn report_round_trips_through_serde() {
        let mut case = judged("task-a", [90, 90, 90, 90]);
        case.rule = Some(rule(RuleVerdict::Pass));
        let golden = GoldenMap::from([golden_row("task-a")]);
        let mut report = assemble(
            &[case],
            &golden,
            &LabelMap::new(),
            Some(super::LatencySummary {
                count: 1,
                p50_ms: 10.0,
                p95_ms: 20.0,
            }),
        );
        report.generated_at = 1000;
        let json = serde_json::to_string(&report).expect("serialize");
        let back: EvalReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.groups, report.groups);
        assert_eq!(back.totals, report.totals);
        assert_eq!(back.latency, report.latency);
        assert!(json.contains(r#""p50_ms":10.0"#), "{json}");
        assert!(json.contains(r#""quality":{"#), "{json}");
    }

    fn golden_row(id: &str) -> (String, GoldenRow) {
        (
            id.to_owned(),
            GoldenRow {
                id: id.to_owned(),
                rounds: vec![vec![r#"{"note":"义"}"#.to_owned()]],
                accepted_at: "1970-01-01T00:00:00Z".to_owned(),
            },
        )
    }
}
