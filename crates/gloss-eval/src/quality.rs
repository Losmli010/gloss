//! 评审结算：按评审提示词（评测侧资产）渲染消息、按严格整体契约解析
//! 评审回复，并给出单例的评审资格判定。评审只服务任务用例——分类子类
//! 的准确性由契约的类别符合检查承载；四维分数的均值派生、分桶与合格
//! 率住聚合层，本模块只管渲染与解析两端与打分前提。

use gloss_core::prompt::ChatMessage;

use crate::contract::derive_state;
use crate::dataset::EvalCase;
use crate::detail::{ContractOutcome, ContractState, EvalKind, JudgeScores};

pub const JUDGE_PROMPT: &str = include_str!("../prompts/judge.md");

pub const JUDGE_MAX_TOKENS: u32 = 512;

pub fn render_judge(case: &EvalCase, reply: &str) -> Vec<ChatMessage> {
    let system = JUDGE_PROMPT
        .replace("{{input}}", &case.text)
        .replace("{{output}}", reply)
        .replace("{{reference}}", &case.reference.reply_json())
        .replace("{{kind}}", case.kind.display_name());
    vec![
        ChatMessage::system(system),
        ChatMessage::user(case.text.clone()),
    ]
}

pub fn parse_scores(reply: &str) -> Option<JudgeScores> {
    let value = serde_json::from_str::<serde_json::Value>(reply.trim()).ok()?;
    let dim = |key: &str| -> Option<u8> {
        value
            .get(key)?
            .as_u64()
            .filter(|score| *score <= 100)
            .map(|score| score as u8)
    };
    Some(JudgeScores {
        accuracy: dim("accuracy")?,
        completeness: dim("completeness")?,
        fluency: dim("fluency")?,
        presentation: dim("presentation")?,
        reason: value.get("reason")?.as_str()?.to_owned(),
    })
}

pub fn judge_eligible(kind: &EvalKind, engine_error: bool, contract: &ContractOutcome) -> bool {
    if !matches!(kind, EvalKind::Task(_)) {
        return false;
    }
    matches!(
        derive_state(engine_error, contract),
        ContractState::Valid | ContractState::Softgap
    )
}

#[cfg(test)]
mod tests {
    use gloss_core::model::Lang;
    use gloss_core::prompt::{ChatMessage, MessageContent};
    use gloss_core::task::{OutcomeStructured, TaskKind};

    use super::{judge_eligible, parse_scores, render_judge};
    use crate::dataset::{EvalCase, Reference};
    use crate::detail::{ContractCheck, ContractOutcome, EvalKind};
    use std::collections::BTreeMap;

    fn outcome(degraded: bool) -> ContractOutcome {
        ContractOutcome {
            json_object: !degraded,
            checks: BTreeMap::from([(ContractCheck::FieldsComplete, !degraded)]),
            degraded,
            violations: BTreeMap::new(),
            repairs: 0,
        }
    }

    fn word_case() -> EvalCase {
        EvalCase {
            id: "task-word-01".to_owned(),
            kind: EvalKind::Task(TaskKind::TranslateWord),
            text: "一".to_owned(),
            target: Lang::Zh,
            reference: Reference::Task {
                note: "义".to_owned(),
                structured: OutcomeStructured::WordCard {
                    phonetic: Some("yī".to_owned()),
                    examples: vec!["例".to_owned()],
                },
            },
        }
    }

    fn system_text(messages: &[ChatMessage]) -> &str {
        match &messages[0].content {
            MessageContent::Text(text) => text,
            MessageContent::Parts(_) => panic!("judge system content must be plain text"),
        }
    }

    #[test]
    fn judge_reply_parses_only_complete_in_range_dimensions() {
        let reply = r#"{"accuracy":90,"completeness":80,"fluency":85,"presentation":75,"reason":"术语准确"}"#;
        let scores = parse_scores(reply).expect("a complete reply parses");
        assert_eq!(scores.accuracy, 90);
        assert_eq!(scores.completeness, 80);
        assert_eq!(scores.fluency, 85);
        assert_eq!(scores.presentation, 75);
        assert_eq!(scores.reason, "术语准确");

        let zero =
            r#"{"accuracy":0,"completeness":0,"fluency":0,"presentation":0,"reason":"下界"}"#;
        let top = r#"{"accuracy":100,"completeness":100,"fluency":100,"presentation":100,"reason":"上界"}"#;
        assert_eq!(parse_scores(zero).map(|scores| scores.accuracy), Some(0));
        assert_eq!(parse_scores(top).map(|scores| scores.accuracy), Some(100));

        for broken in [
            r#"{"completeness":80,"fluency":85,"presentation":75,"reason":"缺准确性"}"#,
            r#"{"accuracy":90,"fluency":85,"presentation":75,"reason":"缺完整性"}"#,
            r#"{"accuracy":101,"completeness":80,"fluency":85,"presentation":75,"reason":"越界"}"#,
            r#"{"accuracy":-1,"completeness":80,"fluency":85,"presentation":75,"reason":"负分"}"#,
            r#"{"accuracy":9007199254740993,"completeness":80,"fluency":85,"presentation":75,"reason":"超大"}"#,
            r#"{"accuracy":85.5,"completeness":80,"fluency":85,"presentation":75,"reason":"非整数"}"#,
            r#"{"accuracy":"90","completeness":80,"fluency":85,"presentation":75,"reason":"字符串分数"}"#,
            r#"{"accuracy":90,"completeness":80,"fluency":85,"presentation":75}"#,
            r#"[]"#,
            "整段散文，不是 JSON",
            "",
        ] {
            assert_eq!(parse_scores(broken), None, "{broken}");
        }
    }

    #[test]
    fn judge_prompt_renders_all_placeholders_and_keeps_input_as_user_turn() {
        let case = word_case();
        let messages = render_judge(&case, "模型产出");
        assert_eq!(messages.len(), 2);
        let system = system_text(&messages);
        assert!(
            system.contains("一") && system.contains(r#""phonetic":"yī""#),
            "the input and the product-shaped reference ride the prompt: {system}"
        );
        assert!(system.contains("模型产出"), "{system}");
        assert!(
            system.contains("词卡"),
            "the kind display name rides {{kind}}: {system}"
        );
        assert!(!system.contains("{{"), "no placeholder survives: {system}");
        match &messages[1].content {
            MessageContent::Text(text) => assert_eq!(text, "一"),
            MessageContent::Parts(_) => panic!("judge user content must be plain text"),
        }
    }

    #[test]
    fn judge_eligibility_skips_classify_degraded_and_engine_errors() {
        let task = EvalKind::Task(TaskKind::TranslateWord);
        let classify = EvalKind::Classify;
        let mut softgap = outcome(false);
        softgap.checks = BTreeMap::from([(ContractCheck::FieldsComplete, false)]);
        assert!(judge_eligible(&task, false, &outcome(false)));
        assert!(
            judge_eligible(&task, false, &softgap),
            "softgap stays eligible"
        );
        assert!(!judge_eligible(&classify, false, &outcome(false)));
        assert!(!judge_eligible(&task, true, &outcome(false)));
        assert!(!judge_eligible(&task, false, &outcome(true)));
        assert!(!judge_eligible(&classify, true, &outcome(true)));
    }
}
