//! 回归结算：把本轮输出对照基线加标注折算成回归五态与差异摘要。基线
//! 是上一轮被认可的本系统输出，回答“相对上次变好还是变坏”；参考答案
//! 不进来——它归评审。两侧原文都先过生产校验器解析成类型化值再规范
//! 化比较：字符串去首尾空白（trim）、例证排序，措辞与顺序噪声不产生
//! 回归误报；比较
//! 按子类分派——分类比类别相等，任务比结构化字段。本模块是纯函数集，
//! 执行编排与单测共用。

use std::collections::BTreeMap;

use gloss_core::schema;
use gloss_core::task::{OutcomeStructured, TaskKind};

use crate::detail::{CaseDetail, EvalKind, LabelVerdict, RuleOutcome, RuleVerdict};

pub fn settle(
    cases: &mut [CaseDetail],
    golden: &BTreeMap<String, Vec<Vec<String>>>,
    labels: &BTreeMap<String, LabelVerdict>,
) {
    for case in cases {
        case.rule = if case.engine_error {
            None
        } else {
            let baseline = golden.get(&case.id).map(|rounds| last_round_text(rounds));
            let label = labels.get(&case.id).copied();
            Some(decide(case, baseline.as_deref(), label))
        };
    }
}

pub fn normalize(structured: &OutcomeStructured) -> OutcomeStructured {
    match structured {
        OutcomeStructured::WordCard { phonetic, examples } => OutcomeStructured::WordCard {
            phonetic: trimmed(phonetic.as_deref()),
            examples: sorted_trimmed(examples),
        },
        OutcomeStructured::Plain { examples } => OutcomeStructured::Plain {
            examples: sorted_trimmed(examples),
        },
        OutcomeStructured::Code {
            examples,
            code_language,
        } => OutcomeStructured::Code {
            examples: sorted_trimmed(examples),
            code_language: trimmed(code_language.as_deref()),
        },
    }
}

enum Compared {
    Equal,
    Unequal,
    Incomparable,
}

fn decide(case: &CaseDetail, baseline: Option<&str>, label: Option<LabelVerdict>) -> RuleOutcome {
    let (Some(baseline), Some(label)) = (baseline, label) else {
        return RuleOutcome {
            verdict: RuleVerdict::Pending,
            structured_match: None,
            summary: None,
        };
    };
    match label {
        LabelVerdict::Pass => decide_pass(case, baseline),
        LabelVerdict::Fail => decide_fail(case, baseline),
    }
}

fn decide_pass(case: &CaseDetail, baseline: &str) -> RuleOutcome {
    if case.contract.degraded {
        return RuleOutcome {
            verdict: RuleVerdict::Regress,
            structured_match: None,
            summary: Some("本轮契约降级，无输出可比".to_owned()),
        };
    }
    match compare(case, baseline) {
        Compared::Equal => RuleOutcome {
            verdict: RuleVerdict::Pass,
            structured_match: Some(true),
            summary: None,
        },
        Compared::Unequal => RuleOutcome {
            verdict: RuleVerdict::Regress,
            structured_match: Some(false),
            summary: Some(diff_summary(case, baseline)),
        },
        Compared::Incomparable => RuleOutcome {
            verdict: RuleVerdict::Regress,
            structured_match: None,
            summary: Some(incomparable_summary(case)),
        },
    }
}

fn decide_fail(case: &CaseDetail, baseline: &str) -> RuleOutcome {
    match compare(case, baseline) {
        Compared::Equal => RuleOutcome {
            verdict: RuleVerdict::Unchanged,
            structured_match: Some(true),
            summary: None,
        },
        Compared::Unequal => RuleOutcome {
            verdict: RuleVerdict::Changed,
            structured_match: Some(false),
            summary: Some(diff_summary(case, baseline)),
        },
        Compared::Incomparable => RuleOutcome {
            verdict: RuleVerdict::Changed,
            structured_match: None,
            summary: Some(incomparable_summary(case)),
        },
    }
}

fn compare(case: &CaseDetail, baseline: &str) -> Compared {
    let current = current_text(case);
    let matched = match &case.kind {
        EvalKind::Classify => classify_equal(current, baseline),
        EvalKind::Task(kind) => structural_equal(*kind, current, baseline),
    };
    match matched {
        Some(true) => Compared::Equal,
        Some(false) => Compared::Unequal,
        None => Compared::Incomparable,
    }
}

fn structural_equal(kind: TaskKind, current: &str, baseline: &str) -> Option<bool> {
    let (_, current_structured) = schema::validate_task(kind, current).ok()?;
    let (_, baseline_structured) = schema::validate_task(kind, baseline).ok()?;
    Some(normalize(&current_structured) == normalize(&baseline_structured))
}

fn classify_equal(current: &str, baseline: &str) -> Option<bool> {
    let current_kind = schema::validate_classify(current).ok()?.kind;
    let baseline_kind = schema::validate_classify(baseline).ok()?.kind;
    Some(current_kind == baseline_kind)
}

fn diff_summary(case: &CaseDetail, baseline: &str) -> String {
    match &case.kind {
        EvalKind::Classify => "判定类别与基线不一致".to_owned(),
        EvalKind::Task(kind) => {
            let current = current_text(case);
            task_diff_summary(*kind, current, baseline)
                .unwrap_or_else(|| "输出与基线不一致".to_owned())
        }
    }
}

fn task_diff_summary(kind: TaskKind, current: &str, baseline: &str) -> Option<String> {
    let (_, current_structured) = schema::validate_task(kind, current).ok()?;
    let (_, baseline_structured) = schema::validate_task(kind, baseline).ok()?;
    let current = normalize(&current_structured);
    let baseline = normalize(&baseline_structured);
    if current.examples().len() != baseline.examples().len() {
        let delta = baseline.examples().len().abs_diff(current.examples().len());
        let word = if current.examples().len() > baseline.examples().len() {
            "多"
        } else {
            "少"
        };
        return Some(format!("例句较基线{word}{delta} 条"));
    }
    match (&current, &baseline) {
        (
            OutcomeStructured::WordCard {
                phonetic: current_phonetic,
                ..
            },
            OutcomeStructured::WordCard {
                phonetic: baseline_phonetic,
                ..
            },
        ) if current_phonetic != baseline_phonetic => Some("音标与基线不一致".to_owned()),
        (
            OutcomeStructured::Code {
                code_language: current_language,
                ..
            },
            OutcomeStructured::Code {
                code_language: baseline_language,
                ..
            },
        ) if current_language != baseline_language => Some("代码语言与基线不一致".to_owned()),
        _ => None,
    }
}

fn incomparable_summary(case: &CaseDetail) -> String {
    if case.contract.degraded {
        "本轮契约降级，无输出可比".to_owned()
    } else {
        "基线原文无法解析为可比的结构化值".to_owned()
    }
}

fn current_text(case: &CaseDetail) -> &str {
    case.rounds
        .last()
        .map(|round| round.text.as_str())
        .unwrap_or("")
}

fn last_round_text(rounds: &[Vec<String>]) -> String {
    rounds
        .last()
        .map(|chunks| chunks.concat())
        .unwrap_or_default()
}

fn trimmed(text: Option<&str>) -> Option<String> {
    text.map(str::trim).map(str::to_owned)
}

fn sorted_trimmed(examples: &[String]) -> Vec<String> {
    let mut out: Vec<String> = examples.iter().map(|text| text.trim().to_owned()).collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use gloss_core::task::{OutcomeStructured, TaskKind};

    use super::{normalize, settle};
    use crate::detail::{
        CaseDetail, ContractCheck, ContractOutcome, EvalKind, LabelVerdict, RoundDetail,
        RuleOutcome, RuleVerdict,
    };
    use std::collections::BTreeMap;

    fn outcome() -> ContractOutcome {
        ContractOutcome {
            json_object: true,
            checks: BTreeMap::from([(ContractCheck::FieldsComplete, true)]),
            degraded: false,
            violations: BTreeMap::new(),
            repairs: 0,
        }
    }

    fn detail(kind: EvalKind, reply: &str) -> CaseDetail {
        CaseDetail {
            id: "case-01".to_owned(),
            kind,
            text: "一".to_owned(),
            rounds: vec![RoundDetail {
                text: reply.to_owned(),
                violation: None,
            }],
            contract: outcome(),
            quality: None,
            rule: None,
            engine_error: false,
        }
    }

    fn reply(note: &str, phonetic: &str, examples: &[&str]) -> String {
        let items: Vec<String> = examples.iter().map(|text| format!(r#""{text}""#)).collect();
        format!(
            r#"{{"note":"{note}","structured":{{"phonetic":"{phonetic}","examples":[{}]}}}}"#,
            items.join(",")
        )
    }

    type Golden = BTreeMap<String, Vec<Vec<String>>>;
    type Labels = BTreeMap<String, LabelVerdict>;

    fn maps(baseline: Option<Vec<Vec<String>>>, label: Option<LabelVerdict>) -> (Golden, Labels) {
        let golden = baseline
            .map(|rounds| BTreeMap::from([("case-01".to_owned(), rounds)]))
            .unwrap_or_default();
        let labels = label
            .map(|verdict| BTreeMap::from([("case-01".to_owned(), verdict)]))
            .unwrap_or_default();
        (golden, labels)
    }

    fn settled(
        mut case: CaseDetail,
        baseline: Option<Vec<Vec<String>>>,
        label: Option<LabelVerdict>,
    ) -> CaseDetail {
        let (golden, labels) = maps(baseline, label);
        settle(std::slice::from_mut(&mut case), &golden, &labels);
        case
    }

    fn rule_of(case: &CaseDetail) -> &RuleOutcome {
        case.rule.as_ref().expect("non-engine cases keep a rule")
    }

    #[test]
    fn label_pass_passes_on_equal_and_regresses_on_mismatch() {
        let baseline = reply("义", "yī", &["例句一"]);
        let equal = settled(
            detail(EvalKind::Task(TaskKind::TranslateWord), &baseline),
            Some(golden_single(&baseline)),
            Some(LabelVerdict::Pass),
        );
        let rule = rule_of(&equal);
        assert_eq!(rule.verdict, RuleVerdict::Pass);
        assert_eq!(rule.structured_match, Some(true));
        assert_eq!(rule.summary, None);

        let mismatch = detail(
            EvalKind::Task(TaskKind::TranslateWord),
            &reply("义", "yī", &["例句一", "例句二"]),
        );
        let settled_mismatch = settled(
            mismatch,
            Some(golden_single(&baseline)),
            Some(LabelVerdict::Pass),
        );
        let rule = rule_of(&settled_mismatch);
        assert_eq!(rule.verdict, RuleVerdict::Regress);
        assert_eq!(rule.structured_match, Some(false));
        assert_eq!(rule.summary.as_deref(), Some("例句较基线多1 条"));
    }

    #[test]
    fn label_pass_with_degradation_regresses_without_structured_match() {
        let baseline = reply("义", "yī", &["例句一"]);
        let mut degraded = detail(EvalKind::Task(TaskKind::TranslateWord), "整段散文");
        degraded.contract.degraded = true;
        let settled_case = settled(
            degraded,
            Some(golden_single(&baseline)),
            Some(LabelVerdict::Pass),
        );
        let rule = rule_of(&settled_case);
        assert_eq!(rule.verdict, RuleVerdict::Regress);
        assert_eq!(rule.structured_match, None);
        assert_eq!(rule.summary.as_deref(), Some("本轮契约降级，无输出可比"));
    }

    #[test]
    fn label_fail_changes_on_mismatch_and_stays_unchanged_on_equal() {
        let baseline = reply("义", "yī", &["例句一"]);
        let equal = settled(
            detail(EvalKind::Task(TaskKind::TranslateWord), &baseline),
            Some(golden_single(&baseline)),
            Some(LabelVerdict::Fail),
        );
        let rule = rule_of(&equal);
        assert_eq!(rule.verdict, RuleVerdict::Unchanged);
        assert_eq!(rule.structured_match, Some(true));
        assert_eq!(rule.summary, None);

        let mismatch = settled(
            detail(
                EvalKind::Task(TaskKind::TranslateWord),
                &reply("义", "yì", &["例句一"]),
            ),
            Some(golden_single(&baseline)),
            Some(LabelVerdict::Fail),
        );
        let rule = rule_of(&mismatch);
        assert_eq!(rule.verdict, RuleVerdict::Changed);
        assert_eq!(rule.structured_match, Some(false));
        assert_eq!(rule.summary.as_deref(), Some("音标与基线不一致"));
    }

    #[test]
    fn missing_baseline_or_label_pends_and_engine_error_clears_the_rule() {
        let baseline = reply("义", "yī", &["例句一"]);
        let current = detail(EvalKind::Task(TaskKind::TranslateWord), &baseline);
        let pending = settled(current, None, Some(LabelVerdict::Pass));
        assert_eq!(rule_of(&pending).verdict, RuleVerdict::Pending);
        assert_eq!(rule_of(&pending).structured_match, None);
        assert_eq!(rule_of(&pending).summary, None);

        let no_label = settled(
            detail(EvalKind::Task(TaskKind::TranslateWord), &baseline),
            Some(golden_single(&baseline)),
            None,
        );
        assert_eq!(rule_of(&no_label).verdict, RuleVerdict::Pending);

        let mut engine_error = detail(EvalKind::Task(TaskKind::TranslateWord), &baseline);
        engine_error.engine_error = true;
        let (golden, labels) = maps(Some(golden_single(&baseline)), Some(LabelVerdict::Pass));
        settle(std::slice::from_mut(&mut engine_error), &golden, &labels);
        assert!(engine_error.rule.is_none(), "engine errors carry no rule");
    }

    #[test]
    fn normalization_ignores_whitespace_and_example_order() {
        let shuffled = reply("义", " yī ", &["例句二", "例句一"]);
        let ordered = reply("义", "yī", &["例句一", "例句二"]);
        let equal = settled(
            detail(EvalKind::Task(TaskKind::TranslateWord), &shuffled),
            Some(golden_single(&ordered)),
            Some(LabelVerdict::Pass),
        );
        assert_eq!(rule_of(&equal).verdict, RuleVerdict::Pass);

        let parse = |examples: &str| {
            let json = format!(r#"{{"note":"义","structured":{{"examples":[{examples}]}}}}"#);
            gloss_core::schema::validate_task(TaskKind::TranslateWord, &json).expect("valid reply")
        };
        let first_order = parse(r#""乙","甲""#);
        let second_order = parse(r#""甲","乙""#);
        assert_eq!(
            normalize(&first_order.1),
            normalize(&second_order.1),
            "example order is not a regression signal"
        );
        assert!(
            matches!(
                normalize(&first_order.1),
                OutcomeStructured::WordCard { ref examples, .. } if examples.len() == 2
            ),
            "the normalized value stays a word card"
        );
    }

    #[test]
    fn classify_regression_compares_parsed_categories() {
        let same_kind = settled(
            detail(EvalKind::Classify, r#"{"kind":"Fallback"}"#),
            Some(golden_single(r#"{"kind":"Fallback"}"#)),
            Some(LabelVerdict::Pass),
        );
        assert_eq!(rule_of(&same_kind).verdict, RuleVerdict::Pass);

        let other_kind = settled(
            detail(EvalKind::Classify, r#"{"kind":"ExplainCode"}"#),
            Some(golden_single(r#"{"kind":"TranslateSentence"}"#)),
            Some(LabelVerdict::Fail),
        );
        let rule = rule_of(&other_kind);
        assert_eq!(rule.verdict, RuleVerdict::Changed);
        assert_eq!(rule.structured_match, Some(false));
        assert_eq!(rule.summary.as_deref(), Some("判定类别与基线不一致"));
    }

    #[test]
    fn golden_baseline_rounds_concatenate_chunk_fragments() {
        let baseline = reply("义", "yī", &["例句一"]);
        let (head, tail) = baseline.split_at(baseline.len() / 2);
        let chunks = vec![head.to_owned(), tail.to_owned()];
        let joined = settled(
            detail(EvalKind::Task(TaskKind::TranslateWord), &baseline),
            Some(vec![chunks]),
            Some(LabelVerdict::Pass),
        );
        assert_eq!(rule_of(&joined).verdict, RuleVerdict::Pass);

        let two_rounds = vec![vec!["旧的违约轮".to_owned()], vec![baseline.clone()]];
        let last_round_wins = settled(
            detail(EvalKind::Task(TaskKind::TranslateWord), &baseline),
            Some(two_rounds),
            Some(LabelVerdict::Fail),
        );
        assert_eq!(rule_of(&last_round_wins).verdict, RuleVerdict::Unchanged);
    }

    #[test]
    fn unparsable_baseline_or_output_is_incomparable_not_equal() {
        let baseline = reply("义", "yī", &["例句一"]);
        let mut prose_case = detail(EvalKind::Task(TaskKind::TranslateWord), "整段散文");
        prose_case.contract.degraded = true;
        let prose = settled(
            prose_case,
            Some(golden_single(&baseline)),
            Some(LabelVerdict::Fail),
        );
        let rule = rule_of(&prose);
        assert_eq!(rule.verdict, RuleVerdict::Changed);
        assert_eq!(rule.structured_match, None);
        assert_eq!(rule.summary.as_deref(), Some("本轮契约降级，无输出可比"));

        let corrupt_baseline = settled(
            detail(EvalKind::Task(TaskKind::TranslateWord), &baseline),
            Some(golden_single("基线不是 JSON")),
            Some(LabelVerdict::Pass),
        );
        let rule = rule_of(&corrupt_baseline);
        assert_eq!(rule.verdict, RuleVerdict::Regress);
        assert_eq!(rule.structured_match, None);
        assert_eq!(
            rule.summary.as_deref(),
            Some("基线原文无法解析为可比的结构化值")
        );
    }

    fn golden_single(text: &str) -> Vec<Vec<String>> {
        vec![vec![text.to_owned()]]
    }
}
