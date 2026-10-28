//! 输出契约（传输契约）与校验器：模型回复的格式契约（JSON）、违约类别与
//! 校验入口，生产（完成态解析）与评测共用同一份。
//!
//! 任务按类别三行同构（注恒为首字段，疏证字段收敛在疏证
//! 对象里，与产物疏证结构同构）：词卡落音标加例句、句译、图像、
//! 通用落展开条目、代码讲解另带模型判定的代码语言；输出示例以各提示词
//! 模板的输出契约为准（与提示词模块同源）。
//!
//! 校验分级：“硬约束”只有两条——整段是合法对象（JSON）、注非空
//! （去空白后），任一失手即记违约进降级（全字段严格校验会把重试
//! 率推高几个量级）；“软约束”就地容错——疏证对象缺失或非对象回落
//! 该类别空变体，变体内音标、例句、代码语言类型错当
//! 缺省、坏条目跳过。解析是严格的裸对象格式（JSON），无围栏容错：
//! 围栏输出即非法对象违约；注恒为首字段，流式渐进提取不受嵌套影响。
//!
//! 分类回复校验：仅含类别的单字段对象，类别取分类模板的
//! 四个类别标识之一（兜底类别是模型可选的兜底类别，映射到
//! 通用任务——通用任务模板就是它的执行落点）。类别集由
//! 分类模板固定，不再由调用方传清单。所有违约的措辞固定，
//! 不携带回复原文——违约信息进日志与修复指令都只有类别；
//! 格式违约的细分细节也只携带结构事实
//! （围栏、截断、键缺失），由修复指令通道单独取用。
//!
//! 内容红线：本模块的错误与日志不携带输入内容与模型回复原文。

use crate::task::{OutcomeStructured, TaskKind};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum Violation {
    EmptyReply,
    NotJson,
    MissingNote,
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

impl Violation {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::EmptyReply => "EmptyReply",
            Self::NotJson => "NotJson",
            Self::MissingNote => "MissingNote",
            Self::KindNotAllowed => "KindNotAllowed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatDetail {
    Fenced,
    NotAnObject,
    Truncated,
    NoteKeyMissing,
    NoteNotString,
}

impl FormatDetail {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Fenced => "fenced",
            Self::NotAnObject => "not_an_object",
            Self::Truncated => "truncated",
            Self::NoteKeyMissing => "note_key_missing",
            Self::NoteNotString => "note_not_string",
        }
    }
}

const FENCE_MARKER: &str = "```";

pub fn format_detail(violation: Violation, raw: &str) -> Option<FormatDetail> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    match violation {
        Violation::EmptyReply | Violation::KindNotAllowed => None,
        Violation::NotJson => {
            if trimmed.starts_with(FENCE_MARKER) {
                return Some(FormatDetail::Fenced);
            }
            match serde_json::from_str::<serde_json::Value>(trimmed) {
                Ok(value) => (!value.is_object()).then_some(FormatDetail::NotAnObject),
                Err(error) => error.is_eof().then_some(FormatDetail::Truncated),
            }
        }
        Violation::MissingNote => {
            let value = serde_json::from_str::<serde_json::Value>(trimmed).ok()?;
            match value.get("note") {
                None => Some(FormatDetail::NoteKeyMissing),
                Some(note) => note
                    .as_str()
                    .is_none_or(|text| text.trim().is_empty())
                    .then_some(FormatDetail::NoteNotString),
            }
        }
    }
}

pub const CLASSIFY_CATEGORY_NAMES: [&str; 4] = [
    "TranslateWord",
    "TranslateSentence",
    "ExplainCode",
    "Fallback",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifyChoice {
    pub model_kind: String,
    pub kind: TaskKind,
}

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

fn text_field(value: Option<&serde_json::Value>, key: &str) -> Option<String> {
    value
        .and_then(|value| value.get(key))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

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
    fn format_detail_maps_fence_truncation_and_non_object_shapes() {
        use super::{FormatDetail, format_detail};

        for raw in ["```json\n{\"note\":\"义\"}\n```", "  ```\n散行\n```  "] {
            assert_eq!(
                format_detail(Violation::NotJson, raw),
                Some(FormatDetail::Fenced),
                "{raw}: a trimmed fence prefix is the fenced detail"
            );
        }
        for raw in ["\"TranslateWord\"", "[\"数组也不是对象\"]", "null", "42"] {
            assert_eq!(
                format_detail(Violation::NotJson, raw),
                Some(FormatDetail::NotAnObject),
                "{raw}: valid JSON that is not an object"
            );
        }
        for raw in ["{\"note\":\"义", "{\"kind\":\"TranslateWord\""] {
            assert_eq!(
                format_detail(Violation::NotJson, raw),
                Some(FormatDetail::Truncated),
                "{raw}: an end-of-input parse error is truncation"
            );
        }
        assert_eq!(
            format_detail(Violation::NotJson, "整段散文，不是 JSON"),
            None,
            "a syntax error at the first byte is no truncation"
        );
        assert_eq!(
            format_detail(Violation::NotJson, "```\nMARKER-回复原文\n```"),
            Some(FormatDetail::Fenced),
            "the detail carries the structural fact only, never the fenced content"
        );
    }

    #[test]
    fn format_detail_maps_missing_note_shapes_to_key_or_string_details() {
        use super::{FormatDetail, format_detail};

        for raw in ["{}", "{\"structured\":{\"examples\":[]}}"] {
            assert_eq!(
                format_detail(Violation::MissingNote, raw),
                Some(FormatDetail::NoteKeyMissing),
                "{raw}: an object without the note key"
            );
        }
        for raw in ["{\"note\":null}", "{\"note\":42}", "{\"note\":\"   \"}"] {
            assert_eq!(
                format_detail(Violation::MissingNote, raw),
                Some(FormatDetail::NoteNotString),
                "{raw}: a non-string or blank note"
            );
        }
        assert_eq!(
            format_detail(Violation::MissingNote, "整段散文"),
            None,
            "an unparsable raw carries no note detail"
        );
    }

    #[test]
    fn format_detail_returns_none_for_undetailed_violations_and_legal_input() {
        use super::format_detail;

        for raw in ["", "   ", "{\"note\":\"义\",\"structured\":{}}"] {
            for violation in [Violation::EmptyReply, Violation::KindNotAllowed] {
                assert_eq!(
                    format_detail(violation, raw),
                    None,
                    "{violation:?} x {raw:?}: violations without details never detail"
                );
            }
        }
        assert_eq!(
            format_detail(Violation::NotJson, "{\"note\":\"义\"}"),
            None,
            "a legal object is no NotJson detail"
        );
        assert_eq!(
            format_detail(Violation::MissingNote, "{\"note\":\"义\"}"),
            None,
            "a legal note is no MissingNote detail"
        );
    }

    #[test]
    fn classify_category_names_match_the_validator_exactly() {
        use super::CLASSIFY_CATEGORY_NAMES;

        assert_eq!(CLASSIFY_CATEGORY_NAMES.len(), 4);
        for name in CLASSIFY_CATEGORY_NAMES {
            let choice = validate_classify(&format!("{{\"kind\":\"{name}\"}}"))
                .unwrap_or_else(|violation| panic!("{name} must validate: {violation}"));
            assert_eq!(choice.model_kind, name);
        }
        assert!(
            CLASSIFY_CATEGORY_NAMES.contains(&"Fallback"),
            "the model-facing fallback category is part of the fixed category set"
        );
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
