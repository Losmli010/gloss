//! 输出契约（wire schema）与校验器：模型回复的 JSON 契约、违约类别与
//! 校验入口，生产（完成态解析）与评测共用同一份。
//!
//! 任务按 kind 三行同构（`note` 恒为首字段，疏证字段收敛在 `structured`
//! 对象里，与 [`OutcomeStructured`] 同构）：
//!
//! ```json
//! {"note":"…","structured":{"phonetic":"…或 null","examples":["…"]}}       // TranslateWord → WordCard
//! {"note":"…","structured":{"examples":["…"]}}                              // TranslateSentence、ImageExplain → Plain
//! {"note":"…","structured":{"examples":["…"],"code_language":"…或 null"}}  // ExplainCode → Code
//! ```
//!
//! 校验分级：**硬约束**只有两条——整段是合法 JSON 对象、`note` 非空
//! （trim 后），任一失守即 [`Violation`] 进降级（全字段严格校验会把重试
//! 率推高几个量级）；**软约束**就地容错——`structured` 缺失或非对象回落
//! 该 kind 空变体，变体内 `phonetic`/`examples`/`code_language` 类型错当
//! 缺省、坏条目跳过。解析是严格的裸 JSON（无围栏容错：围栏输出即
//! [`Violation::NotJson`]）；`note` 恒为首字段，流式渐进提取不受嵌套影响。
//!
//! [`validate_classify`] 校验分类回复：`{"kind":"…"}`，kind 必须是
//! [`TaskKind`] 的 serde 名且在 `allowed` 清单内。所有 [`Violation`] 的
//! 措辞固定，不携带回复原文——违约信息进日志与修复指令都只有类别。
//!
//! 内容红线：本模块的错误与日志不携带输入内容与模型回复原文。

use crate::task::{OutcomeStructured, TaskKind};

/// 契约违约类别：措辞固定（[`Display`](std::fmt::Display)），永不携带
/// 回复原文——类别进日志、降级产物与（后续步骤的）修复指令。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Violation {
    /// 回复为空（trim 后零长度）。
    EmptyReply,
    /// 回复不是合法的 JSON 对象（含围栏输出——严格裸 JSON，无围栏容错）。
    NotJson,
    /// JSON 对象缺 `note` 或 `note` trim 后为空。
    MissingNote,
    /// 分类回复的 kind 不在允许清单内（含缺失或不是合法 kind 标识）。
    KindNotAllowed,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let wording = match self {
            Violation::EmptyReply => "empty reply",
            Violation::NotJson => "reply is not a bare JSON object",
            Violation::MissingNote => "reply is missing a non-empty note",
            Violation::KindNotAllowed => "reply kind is outside the allowed list",
        };
        f.write_str(wording)
    }
}

/// 校验分类回复：裸 JSON 对象携带 `kind`，且是 [`TaskKind`] 的 serde 名、
/// 在 `allowed` 清单内。返回判定的 kind，失守给 [`Violation`]。
pub fn validate_classify(raw: &str, allowed: &[TaskKind]) -> Result<TaskKind, Violation> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(Violation::EmptyReply);
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return Err(Violation::NotJson);
    };
    if !value.is_object() {
        return Err(Violation::NotJson);
    }
    let kind = value
        .get("kind")
        .and_then(|v| v.as_str())
        .map(|name| serde_json::Value::String(name.to_owned()))
        .and_then(|name| serde_json::from_value::<TaskKind>(name).ok());
    match kind {
        Some(kind) if allowed.contains(&kind) => Ok(kind),
        _ => Err(Violation::KindNotAllowed),
    }
}

/// 校验任务回复：[`Violation::EmptyReply`] / [`Violation::NotJson`] /
/// [`Violation::MissingNote`] 之外的失守按软约束就地容错（见模块文档）。
/// 返回 `(note, structured)`；`structured` 缺失或非对象回落
/// [`empty_structured`]。
pub fn validate_task(kind: TaskKind, raw: &str) -> Result<(String, OutcomeStructured), Violation> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(Violation::EmptyReply);
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return Err(Violation::NotJson);
    };
    if !value.is_object() {
        return Err(Violation::NotJson);
    }
    let Some(note) = value
        .get("note")
        .and_then(|v| v.as_str())
        .filter(|note| !note.trim().is_empty())
    else {
        return Err(Violation::MissingNote);
    };
    Ok((note.to_owned(), structured_of(kind, &value)))
}

/// kind 的空疏证变体：软约束（`structured` 缺失或非对象）与降级产物的
/// 回落形态。
pub fn empty_structured(kind: TaskKind) -> OutcomeStructured {
    match kind {
        TaskKind::TranslateWord => OutcomeStructured::WordCard {
            phonetic: None,
            examples: Vec::new(),
        },
        TaskKind::ExplainCode => OutcomeStructured::Code {
            examples: Vec::new(),
            code_language: None,
        },
        TaskKind::TranslateSentence | TaskKind::ImageExplain => OutcomeStructured::Plain {
            examples: Vec::new(),
        },
    }
}

/// 按 kind 解析 `structured` 对象的疏证字段（软约束：对象缺失或非对象
/// 整体回落空变体；变体内字段类型错当缺省、数组坏条目跳过）。
fn structured_of(kind: TaskKind, value: &serde_json::Value) -> OutcomeStructured {
    let structured = value.get("structured").filter(|value| value.is_object());
    match kind {
        TaskKind::TranslateWord => OutcomeStructured::WordCard {
            phonetic: text_field(structured, "phonetic"),
            examples: examples_of(structured),
        },
        TaskKind::ExplainCode => OutcomeStructured::Code {
            examples: examples_of(structured),
            code_language: text_field(structured, "code_language"),
        },
        TaskKind::TranslateSentence | TaskKind::ImageExplain => OutcomeStructured::Plain {
            examples: examples_of(structured),
        },
    }
}

/// 取字符串字段；JSON null 与缺失同样返回 `None`（phonetic/code_language
/// 允许缺省）。
fn text_field(value: Option<&serde_json::Value>, key: &str) -> Option<String> {
    value
        .and_then(|value| value.get(key))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

/// 解析字符串数组（best-effort）：非字符串的坏条目跳过、好条目保留；
/// 字段整体缺失或非数组回落空表。
fn examples_of(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(|value| value.get("examples"))
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|entry| entry.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{Violation, empty_structured, validate_classify, validate_task};
    use crate::task::{OutcomeStructured, TaskKind};

    fn allowed() -> Vec<TaskKind> {
        vec![
            TaskKind::TranslateWord,
            TaskKind::TranslateSentence,
            TaskKind::ExplainCode,
        ]
    }

    #[test]
    fn violations_carry_fixed_wording_without_reply_content() {
        for violation in [
            Violation::EmptyReply,
            Violation::NotJson,
            Violation::MissingNote,
            Violation::KindNotAllowed,
        ] {
            let wording = violation.to_string();
            assert!(!wording.is_empty(), "{violation:?} must carry wording");
            for marker in ["选区", "回复原文", "sk-", "note 正文"] {
                assert!(
                    !wording.contains(marker),
                    "{violation:?} wording must never quote reply content: {wording}"
                );
            }
        }
        assert_eq!(Violation::EmptyReply.to_string(), "empty reply");
        assert_eq!(
            Violation::KindNotAllowed.to_string(),
            "reply kind is outside the allowed list"
        );
    }

    #[test]
    fn classify_violation_matrix_covers_empty_not_json_and_kind() {
        assert_eq!(
            validate_classify("", &allowed()),
            Err(Violation::EmptyReply)
        );
        assert_eq!(
            validate_classify("   \n\t", &allowed()),
            Err(Violation::EmptyReply)
        );
        assert_eq!(
            validate_classify("我觉得这是翻译", &allowed()),
            Err(Violation::NotJson)
        );
        assert_eq!(
            validate_classify("{\"kind\":\"TranslateWord\"", &allowed()),
            Err(Violation::NotJson),
            "truncated JSON is a violation"
        );
        assert_eq!(
            validate_classify("```json\n{\"kind\":\"ExplainCode\"}\n```", &allowed()),
            Err(Violation::NotJson),
            "fenced output violates the bare-JSON contract"
        );
        assert_eq!(
            validate_classify("\"TranslateWord\"", &allowed()),
            Err(Violation::NotJson),
            "a bare JSON scalar is not an object"
        );
        assert_eq!(
            validate_classify("{}", &allowed()),
            Err(Violation::KindNotAllowed),
            "a missing kind names no allowed kind"
        );
        assert_eq!(
            validate_classify("{\"kind\":null}", &allowed()),
            Err(Violation::KindNotAllowed)
        );
        assert_eq!(
            validate_classify("{\"kind\":\"Nonsense\"}", &allowed()),
            Err(Violation::KindNotAllowed)
        );
        assert_eq!(
            validate_classify("{\"kind\":\"ImageExplain\"}", &allowed()),
            Err(Violation::KindNotAllowed),
            "an image kind is outside the classify list"
        );
        assert_eq!(
            validate_classify("{\"note\":\"正文\"}", &allowed()),
            Err(Violation::KindNotAllowed)
        );
    }

    #[test]
    fn classify_accepts_allowed_kinds_only_from_the_list() {
        assert_eq!(
            validate_classify("{\"kind\":\"TranslateWord\"}", &allowed()),
            Ok(TaskKind::TranslateWord)
        );
        assert_eq!(
            validate_classify("  {\"kind\":\"ExplainCode\"}  \n", &allowed()),
            Ok(TaskKind::ExplainCode),
            "surrounding whitespace is trimmed"
        );
        assert_eq!(
            validate_classify(
                "{\"kind\":\"TranslateSentence\"}",
                &[TaskKind::TranslateSentence]
            ),
            Ok(TaskKind::TranslateSentence),
            "the allowed list is the caller's decision, not a global one"
        );
    }

    #[test]
    fn task_violation_matrix_covers_empty_not_json_and_missing_note() {
        for kind in [
            TaskKind::TranslateWord,
            TaskKind::TranslateSentence,
            TaskKind::ExplainCode,
            TaskKind::ImageExplain,
        ] {
            assert_eq!(
                validate_task(kind, ""),
                Err(Violation::EmptyReply),
                "{kind:?}"
            );
            assert_eq!(
                validate_task(kind, "整段正文，不是 JSON"),
                Err(Violation::NotJson),
                "{kind:?}"
            );
            assert_eq!(
                validate_task(kind, "[\"数组也不是对象\"]"),
                Err(Violation::NotJson),
                "{kind:?}: a JSON array is not an object"
            );
            assert_eq!(
                validate_task(kind, "{\"note\":\"义\",\"structured\""),
                Err(Violation::NotJson),
                "{kind:?}: truncated JSON is a violation"
            );
            assert_eq!(
                validate_task(kind, "{\"structured\":{\"examples\":[]}}"),
                Err(Violation::MissingNote),
                "{kind:?}"
            );
            assert_eq!(
                validate_task(kind, "{\"note\":null}"),
                Err(Violation::MissingNote),
                "{kind:?}"
            );
            assert_eq!(
                validate_task(kind, "{\"note\":\"   \"}"),
                Err(Violation::MissingNote),
                "{kind:?}: blank note is empty after trim"
            );
            assert_eq!(
                validate_task(kind, "{\"note\":42}"),
                Err(Violation::MissingNote),
                "{kind:?}: a non-string note is no note"
            );
        }
    }

    #[test]
    fn task_ok_builds_the_three_contract_variants() {
        let (note, structured) = validate_task(
            TaskKind::TranslateWord,
            r#"{"note":"义释","structured":{"phonetic":"/ɡlɒs/","examples":["例句一",42,"例句二"]}}"#,
        )
        .expect("word card contract");
        assert_eq!(note, "义释");
        assert_eq!(
            structured,
            OutcomeStructured::WordCard {
                phonetic: Some("/ɡlɒs/".into()),
                examples: vec!["例句一".into(), "例句二".into()],
            },
            "soft rules skip bad entries"
        );

        let (note, structured) = validate_task(
            TaskKind::ExplainCode,
            r#"{"note":"讲解","structured":{"examples":["展开一"],"code_language":"rust"}}"#,
        )
        .expect("code contract");
        assert_eq!(note, "讲解");
        assert_eq!(
            structured,
            OutcomeStructured::Code {
                examples: vec!["展开一".into()],
                code_language: Some("rust".into()),
            }
        );

        for kind in [TaskKind::TranslateSentence, TaskKind::ImageExplain] {
            let (note, structured) = validate_task(
                kind,
                r#"{"note":"义","structured":{"examples":["条目一","条目二"]}}"#,
            )
            .expect("plain contract");
            assert_eq!(note, "义");
            assert_eq!(
                structured,
                OutcomeStructured::Plain {
                    examples: vec!["条目一".into(), "条目二".into()],
                },
                "{kind:?} lands on the plain variant"
            );
        }
    }

    #[test]
    fn task_soft_rules_fall_back_field_by_field() {
        // structured 整体缺失/非对象：空变体，note 保留（硬约束只在 note）。
        let (_, structured) = validate_task(TaskKind::TranslateWord, r#"{"note":"只有义"}"#)
            .expect("note-only reply is contractually fine");
        assert_eq!(structured, empty_structured(TaskKind::TranslateWord));

        let (_, structured) = validate_task(
            TaskKind::ExplainCode,
            r#"{"note":"义","structured":"不是对象"}"#,
        )
        .expect("non-object structured falls back");
        assert_eq!(structured, empty_structured(TaskKind::ExplainCode));

        // 变体内字段类型错当缺省、null 当缺省。
        let (_, structured) = validate_task(
            TaskKind::ExplainCode,
            r#"{"note":"义","structured":{"examples":"不是数组","code_language":null}}"#,
        )
        .expect("bad field types fall back");
        assert_eq!(
            structured,
            OutcomeStructured::Code {
                examples: Vec::new(),
                code_language: None,
            }
        );

        // kind 决定变体：清单外的疏证键就地忽略。
        let (_, structured) = validate_task(
            TaskKind::TranslateSentence,
            r#"{"note":"义","structured":{"phonetic":"/ɡ/","examples":["例句"],"code_language":"rust"}}"#,
        )
        .expect("extra keys are ignored");
        assert_eq!(
            structured,
            OutcomeStructured::Plain {
                examples: vec!["例句".into()],
            }
        );
    }

    #[test]
    fn empty_structured_covers_every_kind() {
        assert_eq!(
            empty_structured(TaskKind::TranslateWord),
            OutcomeStructured::WordCard {
                phonetic: None,
                examples: Vec::new(),
            }
        );
        assert_eq!(
            empty_structured(TaskKind::ExplainCode),
            OutcomeStructured::Code {
                examples: Vec::new(),
                code_language: None,
            }
        );
        assert_eq!(
            empty_structured(TaskKind::TranslateSentence),
            OutcomeStructured::Plain {
                examples: Vec::new(),
            }
        );
        assert_eq!(
            empty_structured(TaskKind::ImageExplain),
            OutcomeStructured::Plain {
                examples: Vec::new(),
            },
            "image explain lands on the plain variant"
        );
    }
}
