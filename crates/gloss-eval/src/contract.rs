//! 契约结算：逐轮原文经生产校验器（核心层输出契约）折算成契约明细与
//! 逐轮轨迹，并从明细派生契约四态。评测与生产共用同一份校验器，评测
//! 数字度量的才是生产行为；本模块是纯函数集，执行编排与单测共用。
//! 末轮定降级与检查项，逐轮还原违约类别与修复轮次；按子类分派——
//! 任务用例走任务校验器加字段完整检查，分类用例走分类校验器加类别
//! 符合检查。

use std::collections::BTreeMap;

use gloss_core::schema::{self, Violation};
use gloss_core::task::TaskKind;

use crate::detail::{ContractCheck, ContractOutcome, ContractState, RoundDetail};

pub fn settle_task(kind: TaskKind, rounds: &[String]) -> (ContractOutcome, Vec<RoundDetail>) {
    let (mut outcome, detail) =
        track_rounds(|raw| schema::validate_task(kind, raw).map(|_| ()), rounds);
    let reply = last_reply(rounds);
    outcome.degraded = schema::validate_task(kind, reply).is_err();
    outcome.json_object = parses_as_object(reply);
    outcome.checks =
        BTreeMap::from([(ContractCheck::FieldsComplete, fields_complete(kind, reply))]);
    (outcome, detail)
}

pub fn settle_classify(
    expected: TaskKind,
    rounds: &[String],
) -> (ContractOutcome, Vec<RoundDetail>) {
    let (mut outcome, detail) =
        track_rounds(|raw| schema::validate_classify(raw).map(|_| ()), rounds);
    let reply = last_reply(rounds);
    let parsed = schema::validate_classify(reply).ok();
    outcome.degraded = parsed.is_none();
    outcome.json_object = parses_as_object(reply);
    outcome.checks = BTreeMap::from([(
        ContractCheck::KindMatch,
        parsed.is_some_and(|choice| choice.kind == expected),
    )]);
    (outcome, detail)
}

pub fn derive_state(engine_error: bool, outcome: &ContractOutcome) -> ContractState {
    if engine_error {
        ContractState::EngineError
    } else if outcome.degraded {
        ContractState::Degraded
    } else if outcome.checks.values().any(|passed| !passed) {
        ContractState::Softgap
    } else {
        ContractState::Valid
    }
}

fn track_rounds(
    validate: impl Fn(&str) -> Result<(), Violation>,
    rounds: &[String],
) -> (ContractOutcome, Vec<RoundDetail>) {
    let mut violations = BTreeMap::new();
    let detail = rounds
        .iter()
        .map(|text| {
            let violation = validate(text).err();
            if let Some(violation) = &violation {
                *violations.entry(*violation).or_default() += 1;
            }
            RoundDetail {
                text: text.clone(),
                violation,
            }
        })
        .collect();
    let outcome = ContractOutcome {
        json_object: false,
        checks: BTreeMap::new(),
        degraded: false,
        violations,
        repairs: rounds.len().saturating_sub(1) as u32,
    };
    (outcome, detail)
}

fn last_reply(rounds: &[String]) -> &str {
    rounds.last().map(String::as_str).unwrap_or("")
}

fn parses_as_object(reply: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(reply.trim())
        .map(|value| value.is_object())
        .unwrap_or(false)
}

fn fields_complete(kind: TaskKind, reply: &str) -> bool {
    let parsed = serde_json::from_str::<serde_json::Value>(reply.trim()).ok();
    parsed
        .as_ref()
        .and_then(|value| value.get("structured"))
        .filter(|value| value.is_object())
        .is_some_and(|structured| {
            required_fields(kind)
                .iter()
                .all(|key| structured.get(*key).is_some())
        })
}

fn required_fields(kind: TaskKind) -> &'static [&'static str] {
    match kind {
        TaskKind::TranslateWord => &["phonetic", "examples"],
        TaskKind::ExplainCode => &["code_language", "examples"],
        TaskKind::TranslateSentence | TaskKind::ImageExplain | TaskKind::General => &["examples"],
    }
}

#[cfg(test)]
mod tests {
    use gloss_core::schema::Violation;
    use gloss_core::task::TaskKind;

    use super::{derive_state, settle_classify, settle_task};
    use crate::detail::{ContractCheck, ContractState};

    fn good_task_reply() -> String {
        r#"{"note":"义释","structured":{"phonetic":"yī","examples":["《道德经》“道生一”。"]}}"#
            .to_owned()
    }

    #[test]
    fn task_settlement_flags_json_note_and_field_checks() {
        let (outcome, rounds) = settle_task(TaskKind::TranslateWord, &[good_task_reply()]);
        assert!(outcome.json_object);
        assert!(!outcome.degraded);
        assert_eq!(
            outcome.checks.get(&ContractCheck::FieldsComplete),
            Some(&true)
        );
        assert_eq!(outcome.repairs, 0);
        assert!(outcome.violations.is_empty());
        assert_eq!(rounds.len(), 1);
        assert_eq!(rounds[0].violation, None);
    }

    #[test]
    fn task_settlement_degrades_on_prose_and_missing_note() {
        for (reply, violation) in [
            ("整段散文，不是 JSON".to_owned(), Violation::NotJson),
            (
                r#"{"structured":{"examples":[]}}"#.to_owned(),
                Violation::MissingNote,
            ),
            ("   ".to_owned(), Violation::EmptyReply),
        ] {
            let (outcome, rounds) =
                settle_task(TaskKind::TranslateWord, std::slice::from_ref(&reply));
            assert!(outcome.degraded, "{reply}");
            assert_eq!(rounds[0].violation, Some(violation), "{reply}");
            assert_eq!(
                outcome.violations.get(&violation),
                Some(&1),
                "the violation tallies once: {reply}"
            );
        }
        let (outcome, _) = settle_task(TaskKind::TranslateWord, &["整段散文".to_owned()]);
        assert!(!outcome.json_object, "prose is no JSON object");
        assert_eq!(
            outcome.checks.get(&ContractCheck::FieldsComplete),
            Some(&false)
        );
    }

    #[test]
    fn task_settlement_counts_repairs_and_violations_over_rounds() {
        let prose = "整段散文".to_owned();
        let (outcome, rounds) =
            settle_task(TaskKind::TranslateWord, &[prose.clone(), good_task_reply()]);
        assert_eq!(outcome.repairs, 1, "rounds minus one is the repair count");
        assert_eq!(outcome.violations.get(&Violation::NotJson), Some(&1));
        assert!(!outcome.degraded, "the last round settles the outcome");
        assert_eq!(rounds[0].violation, Some(Violation::NotJson));
        assert_eq!(rounds[1].violation, None);
    }

    #[test]
    fn fields_complete_requires_the_subtype_shape() {
        let word_ok = r#"{"note":"义","structured":{"phonetic":"yī","examples":["例"]}}"#;
        let word_no_phonetic = r#"{"note":"义","structured":{"examples":["例"]}}"#;
        let (outcome, _) = settle_task(TaskKind::TranslateWord, &[word_ok.to_owned()]);
        assert_eq!(
            outcome.checks.get(&ContractCheck::FieldsComplete),
            Some(&true)
        );
        let (outcome, _) = settle_task(TaskKind::TranslateWord, &[word_no_phonetic.to_owned()]);
        assert_eq!(
            outcome.checks.get(&ContractCheck::FieldsComplete),
            Some(&false)
        );

        let code_no_language = r#"{"note":"讲","structured":{"examples":["展开"]}}"#;
        let (outcome, _) = settle_task(TaskKind::ExplainCode, &[code_no_language.to_owned()]);
        assert_eq!(
            outcome.checks.get(&ContractCheck::FieldsComplete),
            Some(&false)
        );
        let code_ok = r#"{"note":"讲","structured":{"code_language":"rust","examples":["展开"]}}"#;
        let (outcome, _) = settle_task(TaskKind::ExplainCode, &[code_ok.to_owned()]);
        assert_eq!(
            outcome.checks.get(&ContractCheck::FieldsComplete),
            Some(&true)
        );

        let sentence_ok = r#"{"note":"译","structured":{"examples":["例"]}}"#;
        let (outcome, _) = settle_task(TaskKind::TranslateSentence, &[sentence_ok.to_owned()]);
        assert_eq!(
            outcome.checks.get(&ContractCheck::FieldsComplete),
            Some(&true)
        );
        let no_structured = r#"{"note":"义"}"#;
        let (outcome, _) = settle_task(TaskKind::General, &[no_structured.to_owned()]);
        assert_eq!(
            outcome.checks.get(&ContractCheck::FieldsComplete),
            Some(&false)
        );

        let null_phonetic = r#"{"note":"义","structured":{"phonetic":null,"examples":[]}}"#;
        let (outcome, _) = settle_task(TaskKind::TranslateWord, &[null_phonetic.to_owned()]);
        assert_eq!(
            outcome.checks.get(&ContractCheck::FieldsComplete),
            Some(&true),
            "an explicit null phonetic key counts as present; empty examples stay complete"
        );
        assert!(!outcome.degraded);
    }

    #[test]
    fn classify_settlement_separates_degradation_from_kind_mismatch() {
        let (outcome, _) = settle_classify(
            TaskKind::TranslateWord,
            &[r#"{"kind":"TranslateWord"}"#.to_owned()],
        );
        assert!(!outcome.degraded);
        assert_eq!(outcome.checks.get(&ContractCheck::KindMatch), Some(&true));

        let (outcome, _) = settle_classify(
            TaskKind::TranslateWord,
            &[r#"{"kind":"ExplainCode"}"#.to_owned()],
        );
        assert!(
            !outcome.degraded,
            "a parseable wrong category is no degradation"
        );
        assert_eq!(outcome.checks.get(&ContractCheck::KindMatch), Some(&false));

        for reply in [
            "整段散文".to_owned(),
            r#"{"kind":"Nonsense"}"#.to_owned(),
            r#"{}"#.to_owned(),
            String::new(),
        ] {
            let (outcome, _) =
                settle_classify(TaskKind::TranslateWord, std::slice::from_ref(&reply));
            assert!(outcome.degraded, "{reply}");
            assert_eq!(outcome.checks.get(&ContractCheck::KindMatch), Some(&false));
        }
        let (outcome, _) = settle_classify(
            TaskKind::TranslateSentence,
            &[r#"{"kind":"Fallback"}"#.to_owned()],
        );
        assert!(
            !outcome.degraded,
            "the fallback category lands on the general kind"
        );
        assert_eq!(outcome.checks.get(&ContractCheck::KindMatch), Some(&false));
    }

    #[test]
    fn contract_four_state_derivation_matrix_is_mutually_exclusive() {
        use crate::detail::ContractOutcome;
        use std::collections::BTreeMap;

        let mut checks = BTreeMap::new();
        checks.insert(ContractCheck::FieldsComplete, false);
        let softgap = ContractOutcome {
            json_object: true,
            checks,
            degraded: false,
            violations: BTreeMap::new(),
            repairs: 0,
        };
        let mut degraded_checks = BTreeMap::new();
        degraded_checks.insert(ContractCheck::FieldsComplete, false);
        let degraded = ContractOutcome {
            json_object: false,
            checks: degraded_checks,
            degraded: true,
            violations: BTreeMap::from([(Violation::NotJson, 1)]),
            repairs: 0,
        };
        assert_eq!(derive_state(true, &softgap), ContractState::EngineError);
        assert_eq!(derive_state(true, &degraded), ContractState::EngineError);
        assert_eq!(derive_state(false, &degraded), ContractState::Degraded);
        assert_eq!(derive_state(false, &softgap), ContractState::Softgap);
        assert_eq!(
            derive_state(
                false,
                &crate::detail::ContractOutcome {
                    json_object: true,
                    checks: BTreeMap::from([(ContractCheck::KindMatch, true)]),
                    degraded: false,
                    violations: BTreeMap::new(),
                    repairs: 0,
                }
            ),
            ContractState::Valid
        );
    }
}
