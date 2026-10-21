//! 输出契约（wire schema）与校验器：模型回复的 JSON 契约、违约类别与
//! 校验入口，生产（完成态解析）与评测共用同一份。
//!
//! 任务按 kind 三行同构（`note` 恒为首字段，疏证字段收敛在 `structured`
//! 对象里，与 [`OutcomeStructured`] 同构）：
//!
//! ```json
//! {"note":"…","structured":{"phonetic":"…或 null","examples":["…"]}}       // TranslateWord → WordCard
//! {"note":"…","structured":{"examples":["…"]}}                              // TranslateSentence、ImageExplain、General → Plain
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
//! [`validate_classify`] 校验分类回复：`{"kind":"…"}`，kind 取分类模板的
//! 四个类别标识之一（`Fallback` 是模型可选的兜底类别，映射到
//! [`TaskKind::General`]——通用任务模板就是它的执行落点）。类别集由
//! classify 模板固定，不再由调用方传清单。所有 [`Violation`] 的措辞固定，
//! 不携带回复原文——违约信息进日志与修复指令都只有类别。
//!
//! 内容红线：本模块的错误与日志不携带输入内容与模型回复原文。

use crate::task::{OutcomeStructured, TaskKind};

/// 契约违约类别：措辞固定（[`Display`](std::fmt::Display)），永不携带
/// 回复原文——类别进日志、降级产物与修复指令。serde 形态是变体名本身
/// （评测报告的违约类别计数键）；`Ord` 供按类别的计数表稳定迭代。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum Violation {
    /// 回复为空（trim 后零长度）。
    EmptyReply,
    /// 回复不是合法的 JSON 对象（含围栏输出——严格裸 JSON，无围栏容错）。
    NotJson,
    /// JSON 对象缺 `note` 或 `note` trim 后为空。
    MissingNote,
    /// 分类回复的 kind 不是 classify 模板的类别标识（含缺失）。
    KindNotAllowed,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let wording = match self {
            Violation::EmptyReply => "empty reply",
            Violation::NotJson => "reply is not a bare JSON object",
            Violation::MissingNote => "reply is missing a non-empty note",
            Violation::KindNotAllowed => "reply kind is not one of the classify categories",
        };
        f.write_str(wording)
    }
}

/// 分类回复的判定：模型输出的原始类别与执行落点 kind。模板类别
/// `Fallback` 映射到 [`TaskKind::General`]（兜底类别的执行落点就是通用
/// 任务模板），其余三个类别与 kind 同名直映。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifyChoice {
    /// 模型输出的原始类别标识（classify 模板四个类别之一）。
    pub model_kind: String,
    /// 执行落点：`Fallback` 落 [`TaskKind::General`]，其余原样。
    pub kind: TaskKind,
}

/// 校验分类回复：裸 JSON 对象携带 `kind`，取值为 classify 模板的四个
/// 类别标识之一（`TranslateWord` / `TranslateSentence` / `ExplainCode` /
/// `Fallback`；`General` 不在其中——它是执行落点，不是模型可选类别）。
/// 返回 [`ClassifyChoice`]，失守给 [`Violation`]。
pub fn validate_classify(raw: &str) -> Result<ClassifyChoice, Violation> {
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
    let Some(name) = value.get("kind").and_then(|v| v.as_str()) else {
        return Err(Violation::KindNotAllowed);
    };
    let kind = match name {
        "TranslateWord" => TaskKind::TranslateWord,
        "TranslateSentence" => TaskKind::TranslateSentence,
        "ExplainCode" => TaskKind::ExplainCode,
        "Fallback" => TaskKind::General,
        _ => return Err(Violation::KindNotAllowed),
    };
    Ok(ClassifyChoice {
        model_kind: name.to_owned(),
        kind,
    })
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
        TaskKind::TranslateSentence | TaskKind::ImageExplain | TaskKind::General => {
            OutcomeStructured::Plain {
                examples: Vec::new(),
            }
        }
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
        TaskKind::TranslateSentence | TaskKind::ImageExplain | TaskKind::General => {
            OutcomeStructured::Plain {
                examples: examples_of(structured),
            }
        }
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
    use super::{ClassifyChoice, Violation, empty_structured, validate_classify, validate_task};
    use crate::task::{OutcomeStructured, TaskKind};

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
            "reply kind is not one of the classify categories"
        );
    }

    #[test]
    fn classify_violation_matrix_covers_empty_not_json_and_kind() {
        assert_eq!(validate_classify(""), Err(Violation::EmptyReply));
        assert_eq!(validate_classify("   \n\t"), Err(Violation::EmptyReply));
        assert_eq!(validate_classify("我觉得这是翻译"), Err(Violation::NotJson));
        assert_eq!(
            validate_classify("{\"kind\":\"TranslateWord\""),
            Err(Violation::NotJson),
            "truncated JSON is a violation"
        );
        assert_eq!(
            validate_classify("```json\n{\"kind\":\"ExplainCode\"}\n```"),
            Err(Violation::NotJson),
            "fenced output violates the bare-JSON contract"
        );
        assert_eq!(
            validate_classify("\"TranslateWord\""),
            Err(Violation::NotJson),
            "a bare JSON scalar is not an object"
        );
        assert_eq!(
            validate_classify("{}"),
            Err(Violation::KindNotAllowed),
            "a missing kind names no template category"
        );
        assert_eq!(
            validate_classify("{\"kind\":null}"),
            Err(Violation::KindNotAllowed)
        );
        assert_eq!(
            validate_classify("{\"kind\":\"Nonsense\"}"),
            Err(Violation::KindNotAllowed)
        );
        assert_eq!(
            validate_classify("{\"kind\":\"ImageExplain\"}"),
            Err(Violation::KindNotAllowed),
            "an image kind is not a classify category"
        );
        assert_eq!(
            validate_classify("{\"kind\":\"General\"}"),
            Err(Violation::KindNotAllowed),
            "the general kind is the execution landing, never a model category"
        );
        assert_eq!(
            validate_classify("{\"note\":\"正文\"}"),
            Err(Violation::KindNotAllowed)
        );
    }

    #[test]
    fn classify_accepts_the_template_categories_and_maps_fallback() {
        assert_eq!(
            validate_classify("{\"kind\":\"TranslateWord\"}"),
            Ok(ClassifyChoice {
                model_kind: "TranslateWord".into(),
                kind: TaskKind::TranslateWord,
            })
        );
        assert_eq!(
            validate_classify("  {\"kind\":\"ExplainCode\"}  \n"),
            Ok(ClassifyChoice {
                model_kind: "ExplainCode".into(),
                kind: TaskKind::ExplainCode,
            }),
            "surrounding whitespace is trimmed"
        );
        assert_eq!(
            validate_classify("{\"kind\":\"Fallback\"}"),
            Ok(ClassifyChoice {
                model_kind: "Fallback".into(),
                kind: TaskKind::General,
            }),
            "the model-facing fallback category lands on the general kind"
        );
        assert_eq!(
            validate_classify("{\"kind\":\"TranslateSentence\"}").unwrap(),
            ClassifyChoice {
                model_kind: "TranslateSentence".into(),
                kind: TaskKind::TranslateSentence,
            },
            "the raw category rides the choice for the log"
        );
    }

    #[test]
    fn task_violation_matrix_covers_empty_not_json_and_missing_note() {
        for kind in [
            TaskKind::TranslateWord,
            TaskKind::TranslateSentence,
            TaskKind::ExplainCode,
            TaskKind::ImageExplain,
            TaskKind::General,
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

        for kind in [
            TaskKind::TranslateSentence,
            TaskKind::ImageExplain,
            TaskKind::General,
        ] {
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
        let (_, structured) = validate_task(TaskKind::TranslateWord, r#"{"note":"只有义"}"#)
            .expect("note-only reply is contractually fine");
        assert_eq!(structured, empty_structured(TaskKind::TranslateWord));

        let (_, structured) = validate_task(
            TaskKind::ExplainCode,
            r#"{"note":"义","structured":"不是对象"}"#,
        )
        .expect("non-object structured falls back");
        assert_eq!(structured, empty_structured(TaskKind::ExplainCode));

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
        assert_eq!(
            empty_structured(TaskKind::General),
            OutcomeStructured::Plain {
                examples: Vec::new(),
            },
            "the general kind lands on the plain variant"
        );
    }
}
