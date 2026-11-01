//! 测试用例区渲染：区标题行右侧常驻导出标注按钮（计数实时更新），标
//! 题下为色块图例（契约、回归、标注三组全部枚举，色板与徽标共享），
//! 筛选分类型、契约、回归三行与运算组合，列表固定按最差优先排序；列
//! 表七列，徽标悬停带逐态注释，展开态三栏并排、参考答案整行、结算明
//! 细三行、标注按钮组与逐轮轨迹折叠。只渲染四路输入，不重算结算。

use std::cmp::Ordering;

use gloss_core::task::TaskKind;
use serde_json::json;

use super::{CasePair, ViewInput};
use crate::contract::derive_state;
use crate::detail::{
    CaseDetail, ContractCheck, ContractOutcome, ContractState, EvalKind, JudgeScores, LabelVerdict,
    RuleOutcome, RuleVerdict,
};

pub(super) fn render(input: &ViewInput<'_>) -> String {
    let mut out =
        String::from("<section>\n  <div class=\"secthead\">\n    <h2>测试用例</h2>\n    ");
    out.push_str(&format!(
        "<button class=\"export\" id=\"exportBtn\">导出标注 <span class=\"count\" id=\"exportCount\">{}</span></button>\n  </div>\n",
        marked_count(input)
    ));
    out.push_str(LEGEND);
    out.push_str(FILTERS);
    out.push_str("  <div class=\"empty\" id=\"empty\" hidden>该筛选下无用例</div>\n");
    out.push_str(
        "  <div class=\"cases-head\"><span>用例 ID</span><span>输入</span><span class=\"hd-kind\">任务类型</span><span class=\"hd-rounds\">轮次</span><span>契约</span><span>回归</span><span class=\"hd-overall\">模型评分</span></div>\n",
    );
    out.push_str("  <div class=\"cases\" id=\"cases\">\n");
    for pair in sorted(input) {
        out.push_str(&card(pair, input));
    }
    out.push_str("  </div>\n</section>\n");
    out
}

const LEGEND: &str = r#"  <div class="legend-row">
    <span class="lgroup"><b>契约</b><span class="sw g" title="有效"></span>有效<span class="sw a" title="软缺口"></span>软缺口<span class="sw r" title="降级"></span>降级<span class="sw gr" title="故障"></span>故障</span>
    <span class="lgroup"><b>回归</b><span class="sw g" title="通过"></span>通过<span class="sw r" title="回归"></span>回归<span class="sw a" title="待复核"></span>待复核<span class="sw gr" title="未改善"></span>未改善<span class="sw" title="待比"></span>待比</span>
    <span class="lgroup"><b>标注</b><span class="sw g" title="合格"></span>合格<span class="sw r" title="不合格"></span>不合格<span class="sw" title="未标注"></span>未标注</span>
  </div>
"#;

const FILTERS: &str = r#"  <div id="filters">
    <div class="filter-row"><span class="rname">类型</span>
      <button class="group on" data-v="all">全部</button>
      <button class="group" data-v="classify">分类</button>
      <button class="group" data-v="word">词卡</button>
      <button class="group" data-v="sentence">句译</button>
      <button class="group" data-v="code">代码</button>
      <button class="group" data-v="general">通用</button>
    </div>
    <div class="filter-row"><span class="rname">契约</span>
      <button class="contract on" data-v="all">全部</button>
      <button class="contract" data-v="valid">有效</button>
      <button class="contract" data-v="softgap">软缺口</button>
      <button class="contract" data-v="degraded">降级</button>
      <button class="contract" data-v="error">故障</button>
    </div>
    <div class="filter-row"><span class="rname">回归</span>
      <button class="reg on" data-v="all">全部</button>
      <button class="reg" data-v="pass">通过</button>
      <button class="reg" data-v="regress">回归</button>
      <button class="reg" data-v="review">待复核</button>
      <button class="reg" data-v="unchanged">未改善</button>
      <button class="reg" data-v="pending">待比</button>
    </div>
  </div>
"#;

fn sorted<'a>(input: &ViewInput<'a>) -> Vec<&'a CasePair<'a>> {
    let mut rows: Vec<&CasePair> = input.cases.iter().collect();
    rows.sort_by(|a, b| match order_key(a.detail).cmp(&order_key(b.detail)) {
        Ordering::Equal => overall_of(a.detail).total_cmp(&overall_of(b.detail)),
        other => other,
    });
    rows
}

fn order_key(detail: &CaseDetail) -> (u8, u8, u8) {
    let state = derive_state(detail.engine_error, &detail.contract);
    let badge = badge_rank(detail);
    let tier = if detail.engine_error {
        0
    } else if state == ContractState::Degraded {
        1
    } else if badge.is_some() {
        2
    } else {
        3
    };
    let dim = match &detail.quality {
        Some(scores) => min_dim(scores),
        None => u8::MAX,
    };
    let dim = if tier == 3 { dim } else { 0 };
    (tier, badge.unwrap_or(0), dim)
}

fn overall_of(detail: &CaseDetail) -> f64 {
    match &detail.quality {
        Some(scores) => overall_mean(scores),
        None => f64::INFINITY,
    }
}

fn badge_rank(detail: &CaseDetail) -> Option<u8> {
    match detail.rule.as_ref().map(|rule| rule.verdict) {
        Some(RuleVerdict::Regress) => Some(0),
        Some(RuleVerdict::Changed) => Some(1),
        Some(RuleVerdict::Unchanged) => Some(2),
        _ => None,
    }
}

fn min_dim(scores: &JudgeScores) -> u8 {
    scores
        .accuracy
        .min(scores.completeness)
        .min(scores.fluency)
        .min(scores.presentation)
}

fn overall_mean(scores: &JudgeScores) -> f64 {
    (scores.accuracy as f64
        + scores.completeness as f64
        + scores.fluency as f64
        + scores.presentation as f64)
        / 4.0
}

fn marked_count(input: &ViewInput<'_>) -> usize {
    input
        .cases
        .iter()
        .filter(|pair| input.labels.contains_key(&pair.detail.id))
        .count()
}

fn card(pair: &CasePair<'_>, input: &ViewInput<'_>) -> String {
    let detail = pair.detail;
    let state = derive_state(detail.engine_error, &detail.contract);
    let mut out = String::from("    <details class=\"case\"");
    out.push_str(&format!(" data-id=\"{}\"", super::esc(&detail.id)));
    out.push_str(&format!(" data-contract=\"{}\"", contract_key(state)));
    out.push_str(&format!(
        " data-regression=\"{}\"",
        regression_key(detail.rule.as_ref())
    ));
    out.push_str(&format!(
        " data-group=\"{}\">\n      <summary>\n",
        group_key(&detail.kind)
    ));
    out.push_str(&format!(
        "        <span class=\"cid\">{}</span>\n",
        super::esc(&detail.id)
    ));
    out.push_str(&format!(
        "        <span class=\"ctext\">{}</span>\n",
        super::esc(&detail.text)
    ));
    out.push_str(&format!(
        "        <span class=\"ckind\">{}</span>\n",
        detail.kind.display_name()
    ));
    out.push_str(&format!(
        "        <span class=\"crounds\">{}</span>\n",
        detail.rounds.len()
    ));
    out.push_str(&format!(
        "        {}\n        {}\n",
        contract_badge(state),
        regression_badge(detail.rule.as_ref())
    ));
    let overall = match &detail.quality {
        Some(scores) => super::fmt(overall_mean(scores)),
        None => "—".to_owned(),
    };
    out.push_str(&format!(
        "        <span class=\"coverall\">{overall}</span>\n      </summary>\n"
    ));
    out.push_str(&body(pair, input, state));
    out.push_str("    </details>\n");
    out
}

fn body(pair: &CasePair<'_>, input: &ViewInput<'_>, state: ContractState) -> String {
    let detail = pair.detail;
    let baseline = input.golden.get(&detail.id);
    let mut out = String::from("      <div class=\"case-body\">\n");
    out.push_str(&columns(detail, baseline.map(|row| &row.rounds)));
    out.push_str(&reference(pair));
    out.push_str(&settle(pair, input, state));
    out.push_str(&markline(detail, input));
    out.push_str(&rounds(detail));
    out.push_str("      </div>\n");
    out
}

fn columns(detail: &CaseDetail, baseline: Option<&Vec<Vec<String>>>) -> String {
    let current = if detail.engine_error {
        "请求失败，无回复内容；与模型行为无关。".to_owned()
    } else {
        detail
            .rounds
            .last()
            .map(|round| round.text.clone())
            .unwrap_or_default()
    };
    let mut out = String::from(&format!(
        "        <div class=\"cols{}\">\n",
        if baseline.is_some() { "" } else { " two" }
    ));
    out.push_str(&format!(
        "          <div><div class=\"colhead\">输入</div><pre>{}</pre></div>\n",
        super::esc(&detail.text)
    ));
    out.push_str(&format!(
        "          <div><div class=\"colhead\">本轮输出（模型）</div><pre>{}</pre></div>\n",
        super::esc(&current)
    ));
    if let Some(rounds) = baseline {
        let text = rounds
            .last()
            .map(|round| round.join(""))
            .unwrap_or_default();
        out.push_str(&format!(
            "          <div><div class=\"colhead\">基线输出（固化）</div><pre>{}</pre></div>\n",
            super::esc(&text)
        ));
    }
    out.push_str("        </div>\n");
    out
}

fn reference(pair: &CasePair<'_>) -> String {
    let (head, text) = match &pair.case.reference {
        crate::dataset::Reference::Classify { expected } => (
            "参考答案（期望类别）",
            json!({ "expected": expected }).to_string(),
        ),
        crate::dataset::Reference::Task { .. } => ("参考答案", pair.case.reference.reply_json()),
    };
    format!(
        "        <div class=\"reference\"><div class=\"colhead\">{head}</div><pre>{}</pre></div>\n",
        super::esc(&text)
    )
}

fn settle(pair: &CasePair<'_>, input: &ViewInput<'_>, state: ContractState) -> String {
    let detail = pair.detail;
    let mut out = String::from("        <div class=\"settle\">\n");
    out.push_str(&format!(
        "          <div class=\"srow\"><span class=\"sname\">契约</span>{}<span class=\"sval\">{}</span></div>\n",
        contract_badge(state),
        contract_text(state, &detail.contract)
    ));
    out.push_str(&format!(
        "          <div class=\"srow\"><span class=\"sname\">回归</span>{}<span class=\"sval\">{}</span></div>\n",
        regression_badge(detail.rule.as_ref()),
        regression_text(detail, input)
    ));
    out.push_str(&format!(
        "          <div class=\"srow\"><span class=\"sname\">模型评分</span><span class=\"sval\">{}</span></div>\n",
        score_text(detail, state)
    ));
    out.push_str("        </div>\n");
    out
}

fn contract_key(state: ContractState) -> &'static str {
    match state {
        ContractState::Valid => "valid",
        ContractState::Softgap => "softgap",
        ContractState::Degraded => "degraded",
        ContractState::EngineError => "error",
    }
}

fn contract_badge(state: ContractState) -> String {
    let (class, name, note) = match state {
        ContractState::Valid => ("green", "有效", "契约·有效：JSON 可解析，检查项全部通过"),
        ContractState::Softgap => (
            "amber",
            "软缺口",
            "契约·软缺口：未降级但检查项未过——结构化字段缺失，或分类判定与期望不符",
        ),
        ContractState::Degraded => (
            "red",
            "降级",
            "契约·降级：生产校验器失败（非 JSON、空回复、注缺失或修复未收敛）",
        ),
        ContractState::EngineError => ("gray", "故障", "契约·故障：请求本身失败，与模型行为无关"),
    };
    format!("<span class=\"badge {class}\" title=\"{note}\">{name}</span>")
}

fn regression_key(rule: Option<&RuleOutcome>) -> &'static str {
    match rule.map(|rule| rule.verdict) {
        Some(RuleVerdict::Pass) => "pass",
        Some(RuleVerdict::Regress) => "regress",
        Some(RuleVerdict::Changed) => "review",
        Some(RuleVerdict::Unchanged) => "unchanged",
        _ => "pending",
    }
}

fn regression_badge(rule: Option<&RuleOutcome>) -> String {
    let Some(rule) = rule else {
        return String::from("<span></span>");
    };
    let (class, name, note) = match rule.verdict {
        RuleVerdict::Pass => (
            "green",
            "通过",
            "回归·通过：标注合格，与基线输出一致且契约有效",
        ),
        RuleVerdict::Regress => (
            "red",
            "回归",
            "回归·回归：此前标注合格，本轮降级或与基线不一致",
        ),
        RuleVerdict::Changed => (
            "amber",
            "待复核",
            "回归·待复核：基线曾判不合格，本轮输出已变化，改善与否由你裁决",
        ),
        RuleVerdict::Unchanged => (
            "gray",
            "未改善",
            "回归·未改善：基线曾判不合格，本轮输出与基线一致，无新变化",
        ),
        RuleVerdict::Pending => return String::from("<span></span>"),
    };
    format!("<span class=\"badge {class}\" title=\"{note}\">{name}</span>")
}

fn contract_text(state: ContractState, outcome: &ContractOutcome) -> String {
    let mut parts = vec![if outcome.json_object {
        "JSON 可解析".to_owned()
    } else {
        "非 JSON".to_owned()
    }];
    for (check, passed) in &outcome.checks {
        let label = match (check, passed) {
            (ContractCheck::FieldsComplete, true) => "字段齐".to_owned(),
            (ContractCheck::FieldsComplete, false) => {
                "<span class=\"warn\">字段缺——结构化字段缺失</span>".to_owned()
            }
            (ContractCheck::KindMatch, true) => "分类判定与期望一致".to_owned(),
            (ContractCheck::KindMatch, false) => {
                "<span class=\"warn\">分类判定与期望不符</span>".to_owned()
            }
        };
        parts.push(label);
    }
    if state == ContractState::Degraded {
        let mut tail: Vec<String> = Vec::new();
        if outcome.repairs > 0 {
            tail.push(format!("{} 轮未收敛", outcome.repairs));
        }
        for (violation, count) in &outcome.violations {
            tail.push(format!("违约 {violation:?} ×{count}"));
        }
        if !tail.is_empty() {
            parts.push(format!("<span class=\"bad\">{}</span>", tail.join(" · ")));
        }
    }
    parts.join(" · ")
}

fn regression_text(detail: &CaseDetail, input: &ViewInput<'_>) -> String {
    if detail.engine_error {
        return "待比——无输出可与基线比较".to_owned();
    }
    let Some(rule) = &detail.rule else {
        return "待比——无输出可与基线比较".to_owned();
    };
    match &rule.verdict {
        RuleVerdict::Pass => "<span class=\"ok\">与基线一致</span>".to_owned(),
        RuleVerdict::Regress => format!(
            "<span class=\"bad\">{}</span>",
            super::esc(rule.summary.as_deref().unwrap_or("与基线不一致"))
        ),
        RuleVerdict::Changed => {
            let mut text = String::from(
                "<span class=\"warn\">基线曾判不合格，输出已变化——改善与否由你裁决</span>",
            );
            if let Some(summary) = &rule.summary {
                text.push_str(&format!("；差异：{}", super::esc(summary)));
            }
            text
        }
        RuleVerdict::Unchanged => "与基线一致，无新变化".to_owned(),
        RuleVerdict::Pending => {
            if input.golden.contains_key(&detail.id) {
                "待比——基线在场，等待标注裁决".to_owned()
            } else {
                "待比——新增用例尚未固化基线".to_owned()
            }
        }
    }
}

fn score_text(detail: &CaseDetail, state: ContractState) -> String {
    let Some(scores) = &detail.quality else {
        let reason = if matches!(detail.kind, EvalKind::Classify) {
            "未评分——分类用例不打质量分"
        } else if detail.engine_error {
            "未评分——契约故障跳过质量打分"
        } else if state == ContractState::Degraded {
            "未评分——契约降级跳过质量打分"
        } else {
            "未评分——评审未开启"
        };
        return reason.to_owned();
    };
    format!(
        "准确 {} · 完整 {} · 表达 {} · 呈现 {} · 总体 {}（四维均值）<br>理由：{}",
        scores.accuracy,
        scores.completeness,
        scores.fluency,
        scores.presentation,
        super::fmt(overall_mean(scores)),
        super::esc(&scores.reason)
    )
}

fn markline(detail: &CaseDetail, input: &ViewInput<'_>) -> String {
    let label = input.labels.get(&detail.id);
    let (pass_on, fail_on) = match label.map(|row| row.verdict) {
        Some(LabelVerdict::Pass) => (" on", ""),
        Some(LabelVerdict::Fail) => ("", " on"),
        None => ("", ""),
    };
    let note = label
        .and_then(|row| row.note.as_deref())
        .unwrap_or_default();
    format!(
        "        <div class=\"markline\">\n          <button class=\"markbtn pass{pass_on}\" data-v=\"pass\">合格</button>\n          <button class=\"markbtn fail{fail_on}\" data-v=\"fail\">不合格</button>\n          <input type=\"text\" placeholder=\"备注（可选）\" value=\"{}\">\n          <span class=\"hint\">备注记录判断依据，随标注一并导出落库、长期留存</span>\n        </div>\n",
        super::esc(note)
    )
}

fn rounds(detail: &CaseDetail) -> String {
    if detail.rounds.is_empty() {
        return String::new();
    }
    let mut out = format!(
        "        <details class=\"rounds\"><summary>逐轮轨迹（{} 轮）</summary>\n",
        detail.rounds.len()
    );
    for (index, round) in detail.rounds.iter().enumerate() {
        let tag = round
            .violation
            .map(|violation| format!(" <span class=\"tag\">{violation:?}</span>"))
            .unwrap_or_default();
        out.push_str(&format!(
            "          <div class=\"round\"><div class=\"round-head\">轮 {}{tag}</div><pre>{}</pre></div>\n",
            index + 1,
            super::esc(&round.text)
        ));
    }
    out.push_str("        </details>\n");
    out
}

fn group_key(kind: &EvalKind) -> &'static str {
    match kind {
        EvalKind::Classify => "classify",
        EvalKind::Task(TaskKind::TranslateWord) => "word",
        EvalKind::Task(TaskKind::TranslateSentence) => "sentence",
        EvalKind::Task(TaskKind::ExplainCode) => "code",
        EvalKind::Task(TaskKind::General) => "general",
        EvalKind::Task(TaskKind::ImageExplain) => "image",
    }
}

#[cfg(test)]
mod tests {
    use gloss_core::task::TaskKind;
    use std::collections::BTreeMap;

    use super::super::tests::{
        case, detail, golden_row, input, labeled, outcome, pair, rule, scored,
    };
    use crate::asset::{GoldenMap, LabelMap};
    use crate::dataset::EvalCase;
    use crate::detail::{
        CaseDetail, ContractCheck, ContractOutcome, ContractState, EvalKind, LabelVerdict,
        RoundDetail, RuleVerdict,
    };
    use crate::report::assemble;
    use crate::view::CasePair;

    fn word() -> EvalKind {
        EvalKind::Task(TaskKind::TranslateWord)
    }

    fn pairs_of(specs: &[(EvalCase, CaseDetail)]) -> Vec<CasePair<'_>> {
        specs
            .iter()
            .map(|(eval_case, detail)| pair(eval_case, detail))
            .collect()
    }

    fn page_of(pairs: &[CasePair<'_>], golden: &GoldenMap, labels: &LabelMap) -> String {
        let details: Vec<CaseDetail> = pairs.iter().map(|pair| pair.detail.clone()).collect();
        let report = assemble(&details, golden, labels, None);
        super::render(&input(&report, pairs, labels, golden))
    }

    fn spec(
        id: &str,
        kind: &EvalKind,
        state: ContractState,
        verdict: RuleVerdict,
    ) -> (EvalCase, CaseDetail) {
        let eval_case = case(id, kind);
        let mut owned = detail(id, kind);
        owned.contract = outcome(state);
        owned.rule = Some(rule(verdict, None));
        (eval_case, owned)
    }

    fn ids_in_order(html: &str, ids: &[&str]) -> bool {
        let mut cursor = 0;
        for id in ids {
            let needle = format!("data-id=\"{id}\"");
            match html[cursor..].find(&needle) {
                Some(at) => cursor += at + needle.len(),
                None => return false,
            }
        }
        true
    }

    #[test]
    fn worst_first_orders_failures_then_badges_then_scores() {
        let kind = word();
        let mut specs = Vec::new();
        let mut scored_high = detail("a-scored-high", &kind);
        scored(&mut scored_high, [90, 80, 70, 60]);
        scored_high.rule = Some(rule(RuleVerdict::Pass, None));
        specs.push((case("a-scored-high", &kind), scored_high));
        let mut scored_low = detail("b-scored-low", &kind);
        scored(&mut scored_low, [50, 50, 50, 50]);
        scored_low.rule = Some(rule(RuleVerdict::Pass, None));
        specs.push((case("b-scored-low", &kind), scored_low));
        specs.push((case("c-unscored", &kind), detail("c-unscored", &kind)));
        let mut degraded = detail("d-degraded", &kind);
        degraded.contract = outcome(ContractState::Degraded);
        degraded.rule = Some(rule(RuleVerdict::Unchanged, None));
        specs.push((case("d-degraded", &kind), degraded));
        let mut engine = detail("e-engine", &kind);
        engine.engine_error = true;
        engine.rule = None;
        specs.push((case("e-engine", &kind), engine));
        let mut regress = detail("f-regress", &kind);
        regress.rule = Some(rule(RuleVerdict::Regress, Some("examples 较基线差 1 条")));
        specs.push((case("f-regress", &kind), regress));
        let mut review = detail("g-review", &kind);
        review.rule = Some(rule(RuleVerdict::Changed, None));
        specs.push((case("g-review", &kind), review));
        let mut unchanged = detail("h-unchanged", &kind);
        unchanged.rule = Some(rule(RuleVerdict::Unchanged, None));
        specs.push((case("h-unchanged", &kind), unchanged));
        let pairs = pairs_of(&specs);
        let html = page_of(&pairs, &GoldenMap::new(), &LabelMap::new());
        assert!(
            ids_in_order(
                &html,
                &[
                    "e-engine",
                    "d-degraded",
                    "f-regress",
                    "g-review",
                    "h-unchanged",
                    "b-scored-low",
                    "a-scored-high",
                    "c-unscored",
                ]
            ),
            "engine errors first, then degraded, then the regression badges, then lowest dim ascending with unscored last: {html}"
        );
    }

    fn engine_spec(id: &str, kind: &EvalKind) -> (EvalCase, CaseDetail) {
        let mut owned = detail(id, kind);
        owned.engine_error = true;
        owned.rule = None;
        (case(id, kind), owned)
    }

    fn status_matrix_specs() -> Vec<(EvalCase, CaseDetail)> {
        let kind = word();
        let classify_kind = EvalKind::Classify;
        let mut classify = detail("cls-1", &classify_kind);
        classify.contract = ContractOutcome {
            json_object: true,
            checks: BTreeMap::from([(ContractCheck::KindMatch, false)]),
            degraded: false,
            violations: BTreeMap::new(),
            repairs: 0,
        };
        let mut degraded = detail("task-deg", &kind);
        degraded.contract = outcome(ContractState::Degraded);
        degraded.contract.repairs = 3;
        let mut engine = detail("task-eng", &kind);
        engine.engine_error = true;
        engine.rounds.clear();
        engine.rule = None;
        vec![
            (case("cls-1", &classify_kind), classify),
            (case("task-deg", &kind), degraded),
            (case("task-eng", &kind), engine),
            (case("task-fresh", &kind), detail("task-fresh", &kind)),
            (case("task-wait", &kind), detail("task-wait", &kind)),
        ]
    }

    #[test]
    fn badges_cover_every_contract_and_regression_state() {
        let kind = word();
        let table = [
            ("s-valid", ContractState::Valid, RuleVerdict::Pass),
            ("s-soft", ContractState::Softgap, RuleVerdict::Pending),
            ("s-deg", ContractState::Degraded, RuleVerdict::Pass),
            ("s-regr", ContractState::Valid, RuleVerdict::Regress),
            ("s-changed", ContractState::Valid, RuleVerdict::Changed),
            ("s-still", ContractState::Valid, RuleVerdict::Unchanged),
        ];
        let mut specs: Vec<_> = table
            .iter()
            .map(|(id, state, verdict)| spec(id, &kind, *state, *verdict))
            .collect();
        specs.push(engine_spec("s-eng", &kind));
        let html = page_of(&pairs_of(&specs), &GoldenMap::new(), &LabelMap::new());
        let badges = [
            ("契约·有效：JSON 可解析，检查项全部通过", "有效"),
            (
                "契约·软缺口：未降级但检查项未过——结构化字段缺失，或分类判定与期望不符",
                "软缺口",
            ),
            (
                "契约·降级：生产校验器失败（非 JSON、空回复、注缺失或修复未收敛）",
                "降级",
            ),
            ("契约·故障：请求本身失败，与模型行为无关", "故障"),
            ("回归·通过：标注合格，与基线输出一致且契约有效", "通过"),
            ("回归·回归：此前标注合格，本轮降级或与基线不一致", "回归"),
            (
                "回归·待复核：基线曾判不合格，本轮输出已变化，改善与否由你裁决",
                "待复核",
            ),
            (
                "回归·未改善：基线曾判不合格，本轮输出与基线一致，无新变化",
                "未改善",
            ),
        ];
        for (note, name) in badges {
            assert!(
                html.contains(&format!("{note}\">{name}</span>")),
                "badge {name} missing: {html}"
            );
        }
        assert!(
            html.contains("title=\"合格\"></span>合格")
                && html.contains("title=\"待比\"></span>待比"),
            "the legend covers the label trio and the badge-less pending state: {html}"
        );
    }

    #[test]
    fn a_pending_case_wears_no_regression_badge() {
        let kind = word();
        let specs = vec![spec(
            "s-pending",
            &kind,
            ContractState::Valid,
            RuleVerdict::Pending,
        )];
        let html = page_of(&pairs_of(&specs), &GoldenMap::new(), &LabelMap::new());
        assert_eq!(
            html.matches("<span class=\"badge").count(),
            2,
            "only the contract badge appears (summary plus settle): {html}"
        );
        assert_eq!(
            html.matches("data-regression=\"pending\"").count(),
            1,
            "a pending case stays on the pending filter: {html}"
        );
    }

    #[test]
    fn expanded_body_follows_the_status_matrix() {
        let golden = GoldenMap::from([golden_row("task-wait")]);
        let html = page_of(&pairs_of(&status_matrix_specs()), &golden, &LabelMap::new());
        assert!(
            html.contains("未评分——分类用例不打质量分"),
            "classify cases never enter the judge: {html}"
        );
        assert!(
            html.contains("参考答案（期望类别）")
                && html.contains("{&quot;expected&quot;:&quot;TranslateWord&quot;}"),
            "classify reference shows the expected category: {html}"
        );
        assert!(
            html.contains("<span class=\"warn\">分类判定与期望不符</span>"),
            "a failed kind match turns the contract row amber"
        );
        assert!(
            html.contains("未评分——契约降级跳过质量打分") && html.contains("3 轮未收敛"),
            "degraded cases skip scoring and report the repair tail"
        );
        assert!(
            html.contains("违约 NotJson ×1"),
            "violations render with their tally"
        );
        assert!(
            html.contains("请求失败，无回复内容；与模型行为无关。")
                && html.contains("待比——无输出可与基线比较")
                && html.contains("未评分——契约故障跳过质量打分"),
            "engine failures show the failure note and stay pending"
        );
        assert!(
            html.contains("待比——新增用例尚未固化基线") && html.contains("未评分——评审未开启"),
            "fresh cases stay pending and show the judge-off placeholder"
        );
        assert!(
            html.contains("待比——基线在场，等待标注裁决"),
            "a baseline without a label waits for the verdict"
        );
        let two_col = html.matches("class=\"cols two\"").count();
        let three_col = html.matches("<div class=\"cols\">\n").count();
        assert_eq!(
            (two_col, three_col),
            (4, 1),
            "cases without a baseline collapse to two columns: {html}"
        );
        assert!(html.contains("基线输出（固化）"));
    }

    #[test]
    fn scored_cases_show_dimensions_overall_and_reason() {
        let kind = word();
        let mut owned = detail("task-word-01", &kind);
        scored(&mut owned, [90, 80, 70, 60]);
        let specs = vec![(case("task-word-01", &kind), owned)];
        let html = page_of(&pairs_of(&specs), &GoldenMap::new(), &LabelMap::new());
        assert!(
            html.contains("准确 90 · 完整 80 · 表达 70 · 呈现 60 · 总体 75（四维均值）"),
            "the score line lists four dims plus the mean: {html}"
        );
        assert!(html.contains("理由：锚点理由"));
        assert!(html.contains("<span class=\"coverall\">75</span>"));
    }

    #[test]
    fn marks_prefill_from_labels_and_drive_the_export_count() {
        let kind = word();
        let specs = vec![
            (case("task-pass", &kind), detail("task-pass", &kind)),
            (case("task-fail", &kind), detail("task-fail", &kind)),
            (case("task-open", &kind), detail("task-open", &kind)),
        ];
        let labels = LabelMap::from([
            labeled("task-pass", LabelVerdict::Pass, Some("例句全")),
            labeled("task-fail", LabelVerdict::Fail, None),
        ]);
        let html = page_of(&pairs_of(&specs), &GoldenMap::new(), &labels);
        assert!(
            html.contains("markbtn pass on") && html.contains("value=\"例句全\""),
            "a stored pass label prefills the button and note: {html}"
        );
        assert!(
            html.contains("markbtn fail on"),
            "a stored fail label prefills too"
        );
        assert!(
            html.contains("id=\"exportCount\">2</span>"),
            "the export count equals the labeled cases: {html}"
        );
        assert!(html.contains("逐轮轨迹（1 轮）"));
    }

    #[test]
    fn rounds_carry_violation_tags() {
        let kind = word();
        let mut owned = detail("task-word-01", &kind);
        owned.rounds.insert(
            0,
            RoundDetail {
                text: "这句话讲的是".to_owned(),
                violation: Some(gloss_core::schema::Violation::NotJson),
            },
        );
        let specs = vec![(case("task-word-01", &kind), owned)];
        let html = page_of(&pairs_of(&specs), &GoldenMap::new(), &LabelMap::new());
        assert!(
            html.contains("轮 1 <span class=\"tag\">NotJson</span>"),
            "a violating round wears its tag: {html}"
        );
        assert!(html.contains("逐轮轨迹（2 轮）"));
    }
}
