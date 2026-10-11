//! 完成态解析：模型原始回复 → [`TaskOutcome`]（缓存站点与产物卡的唯一入口）。
//!
//! 契约校验收敛于 `gloss_core::schema`（wire 契约见该模块文档），这里只有
//! **两分支**：
//! 1. `validate_task` Ok → 正常产物（note + 按 kind 的疏证变体，
//!    `degraded = false`）。
//! 2. `Err`（空回复 / 非 JSON / 缺 note）→ 降级产物：`note` 空串、该 kind
//!    空变体、`degraded = true`，warn 留痕（只记违约类别，不携带回复原文）。
//!    降级产物不进缓存（桥侧按 `degraded` 分流）、UI 出降级卡（原文垃圾
//!    不展示不留存）。
//!
//! 流式期间 UI 按纯 JSON 渐进提取 `note`（见 `crate::ui::popup`），完成态
//! 以本模块的产物为准。

use gloss_core::log::{thread, warn};
use gloss_core::schema;
use gloss_core::task::{TaskKind, TaskOutcome};

/// 完成态产物组装（契约纯函数）：校验过出正常产物，违约落降级产物。
/// 引擎失败的任务到不了这里，因此产物恒可回填缓存——但降级产物由调用方
/// 按 `degraded` 分流（不进缓存，见 `crate::runtime::pipeline`）。
pub fn complete(kind: TaskKind, raw: &str) -> TaskOutcome {
    match schema::validate_task(kind, raw) {
        Ok((note, structured)) => TaskOutcome {
            kind,
            note,
            structured,
            degraded: false,
        },
        Err(violation) => {
            warn!(
                thread = thread::TOKIO,
                reason = %violation,
                "task reply violated the contract, degrading the outcome"
            );
            TaskOutcome {
                kind,
                note: String::new(),
                structured: schema::empty_structured(kind),
                degraded: true,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use gloss_core::log;
    use gloss_core::task::{OutcomeStructured, TaskKind};

    use super::complete;

    #[test]
    fn a_contract_reply_builds_the_normal_outcome() {
        let outcome = complete(
            TaskKind::TranslateWord,
            r#"{"note":"**gloss** 的义释","structured":{"phonetic":"/ɡlɒs/","examples":["a gloss of silk","光泽的例句"]}}"#,
        );
        assert_eq!(outcome.kind, TaskKind::TranslateWord);
        assert_eq!(outcome.note, "**gloss** 的义释");
        assert!(!outcome.degraded);
        assert_eq!(
            outcome.structured,
            OutcomeStructured::WordCard {
                phonetic: Some("/ɡlɒs/".into()),
                examples: vec!["a gloss of silk".into(), "光泽的例句".into()],
            }
        );

        let code = complete(
            TaskKind::ExplainCode,
            r#"{"note":"讲解","structured":{"examples":["展开一"],"code_language":"rust"}}"#,
        );
        assert_eq!(
            code.structured,
            OutcomeStructured::Code {
                examples: vec!["展开一".into()],
                code_language: Some("rust".into()),
            }
        );

        let sentence = complete(
            TaskKind::TranslateSentence,
            r#"{"note":"译文","structured":{"examples":[]}}"#,
        );
        assert_eq!(
            sentence.structured,
            OutcomeStructured::Plain {
                examples: Vec::new()
            }
        );
    }

    #[test]
    fn soft_field_gaps_stay_on_the_normal_path() {
        // structured 缺失与坏条目是软约束：note 在场即正常产物。
        let outcome = complete(
            TaskKind::TranslateWord,
            r#"{"note":"只有义","structured":{"phonetic":null,"examples":[null,"好例句",42]}}"#,
        );
        assert!(!outcome.degraded);
        assert_eq!(
            outcome.structured,
            OutcomeStructured::WordCard {
                phonetic: None,
                examples: vec!["好例句".into()],
            },
            "missing subprov fields fall back per the soft rules"
        );
    }

    #[test]
    fn an_image_reply_lands_on_the_plain_variant() {
        let outcome = complete(
            TaskKind::ImageExplain,
            r#"{"note":"一张折线图","structured":{"examples":["九月起访问量加速增长","增长主要由分享渠道带来"]}}"#,
        );
        assert_eq!(outcome.kind, TaskKind::ImageExplain);
        assert_eq!(outcome.note, "一张折线图");
        assert!(!outcome.degraded);
        assert_eq!(
            outcome.structured,
            OutcomeStructured::Plain {
                examples: vec![
                    "九月起访问量加速增长".into(),
                    "增长主要由分享渠道带来".into()
                ],
            }
        );
    }

    #[test]
    fn contract_violations_degrade_to_an_empty_marked_outcome() {
        for (raw, reason) in [
            ("", "empty"),
            ("整段正文，不是 JSON", "prose"),
            ("```gloss\n{\"note\":\"围栏也违约\"}\n```", "fenced"),
            (r#"{"structured":{"examples":[]}}"#, "missing note"),
            (r#"{"note":"   "}"#, "blank note"),
        ] {
            let outcome = complete(TaskKind::ExplainCode, raw);
            assert!(
                outcome.degraded,
                "{reason}: a violation must degrade, got {outcome:?}"
            );
            assert_eq!(outcome.note, "", "{reason}: the note slot stays empty");
            assert_eq!(
                outcome.structured,
                OutcomeStructured::Code {
                    examples: Vec::new(),
                    code_language: None,
                },
                "{reason}: the empty code variant"
            );
        }
    }

    #[test]
    fn the_degrade_warn_carries_the_category_only() {
        let text = log::capture(|| {
            complete(TaskKind::ExplainCode, "整段正文，不是 JSON");
        });
        let lines: Vec<&str> = text
            .lines()
            .filter(|line| line.contains("violated the contract"))
            .collect();
        assert!(!lines.is_empty(), "the degrade must leave a trace: {text}");
        for line in &lines {
            assert!(
                !line.contains("整段正文"),
                "the warn must carry the category only, never reply content: {line}"
            );
            assert!(
                line.contains("reply is not a bare JSON object"),
                "the warn carries the fixed violation wording: {line}"
            );
        }
    }
}
