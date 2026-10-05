//! 完成态解析：模型原始回复 → [`TaskOutcome`]（缓存站点与产物卡的唯一入口）。
//!
//! 两层解析，逐层退让——**模型返回非 JSON 时必有产物**：
//! 1. **JSON 主路径**（现行契约，见 `gloss_core::prompt`）：整段回复是一个
//!    JSON 对象——`note`（义，markdown 注文）+ 按 kind 的疏证字段（词卡的
//!    phonetic/examples，句译与讲解的 examples，代码另带 code_language）。
//!    字段缺失按契约就地回退（phonetic 缺省 null、坏条目跳过、examples
//!    空表），`note` 缺失才判整路失败。
//! 2. **围栏 fallback**（`finalize_outcome`，自 core 原样迁入）：回复不是
//!    JSON 时剥末尾 ` ```gloss … ``` ` 围栏解析——这是**旧契约**（markdown
//!    正文 + 末尾结构化块）的兼容位，模型跑偏输出旧契约时保住产物；围栏
//!    缺失或解析失败再按 kind 回退，无损保留全文。
//!
//! 两层衔接共用 [`gloss_core::prompt::STRUCTURED_FENCE`] 单点：流式期间
//! UI 按纯 JSON 渐进提取 `note`（见 `crate::ui::popup`），完成态以本模块
//! 的产物为准。

use gloss_core::prompt::STRUCTURED_FENCE;
use gloss_core::task::{OutcomeStructured, TaskKind, TaskOutcome};

/// 完成态产物组装（契约纯函数）：先走 JSON 主路径，失败退围栏 fallback。
/// 引擎失败的任务到不了这里，因此产物恒可回填缓存（写缓存归调用方）。
pub fn complete(kind: TaskKind, raw: &str) -> TaskOutcome {
    if let Some((note, code_language, structured)) = parse_json_outcome(kind, raw) {
        return TaskOutcome {
            kind,
            note,
            code_language,
            structured,
        };
    }
    finalize_outcome(kind, raw)
}

/// JSON 主路径：整段回复按一个 JSON 对象解析，`note`（义）给注文，
/// 疏证字段按 kind 就地容错（phonetic 缺省 null、examples 坏条目跳过、
/// code_language 缺省 null）。`note` 缺失或非字符串返回 `None`（整路
/// 失败，交围栏 fallback）。
fn parse_json_outcome(
    kind: TaskKind,
    raw: &str,
) -> Option<(String, Option<String>, OutcomeStructured)> {
    let value = serde_json::from_str::<serde_json::Value>(raw.trim()).ok()?;
    let note = value.get("note")?.as_str()?.to_owned();
    let structured = match kind {
        TaskKind::TranslateWord => OutcomeStructured::WordCard {
            phonetic: text_field(&value, "phonetic"),
            examples: parse_examples(value.get("examples")),
        },
        TaskKind::ImageOcr | TaskKind::ImageExplain => OutcomeStructured::Extracted,
        _ => OutcomeStructured::Plain {
            examples: parse_examples(value.get("examples")),
        },
    };
    let code_language = (kind == TaskKind::ExplainCode)
        .then(|| text_field(&value, "code_language"))
        .flatten();
    Some((note, code_language, structured))
}

/// 围栏 fallback（旧契约，自 gloss-core 原样迁入）：从原始回复里剥出末尾
/// ` ```gloss … ``` ` 围栏解析为 [`OutcomeStructured`]，其余作为 markdown
/// 注文。本函数只在完成态兜底旧契约输出；围栏语义与流式态 UI 的渐进提取
/// 不同源（后者按 JSON note 提取）。
///
/// 剥离边界：定位**最后一个**围栏标记，其后（含闭合围栏与契约外尾随
/// 文字）一律不进注文——正常输出契约下模型不会有尾随内容；围栏缺失或
/// 解析失败按 kind 回退，回退路径无损保留全文（不做有损剥离）。
pub fn finalize_outcome(kind: TaskKind, raw: &str) -> TaskOutcome {
    let (note, structured) = parse_structured(kind, raw);
    TaskOutcome {
        kind,
        note,
        code_language: None,
        structured,
    }
}

/// 从原始回复里剥出旧契约的结构化 JSON 块：末尾的 ` ```gloss … ``` ` 围栏
/// 解析，其余作为 markdown 注文。围栏缺失或解析失败按 kind 回退（无损
/// 保留全文）。
pub fn parse_structured(kind: TaskKind, raw: &str) -> (String, OutcomeStructured) {
    let body = raw;
    let fallback = || -> (String, OutcomeStructured) {
        let owned = body.to_owned();
        let structured = match kind {
            TaskKind::TranslateWord => OutcomeStructured::WordCard {
                phonetic: None,
                examples: Vec::new(),
            },
            TaskKind::ImageOcr | TaskKind::ImageExplain => OutcomeStructured::Extracted,
            _ => OutcomeStructured::Plain {
                examples: Vec::new(),
            },
        };
        (owned, structured)
    };

    let Some(start) = body.rfind(STRUCTURED_FENCE) else {
        return fallback();
    };
    let after_marker = &body[start + STRUCTURED_FENCE.len()..];
    let Some(end_rel) = after_marker.find("```") else {
        return fallback();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(after_marker[..end_rel].trim())
    else {
        return fallback();
    };

    let structured = match kind {
        TaskKind::TranslateWord => OutcomeStructured::WordCard {
            phonetic: text_field(&value, "phonetic"),
            examples: parse_examples(value.get("examples")),
        },
        TaskKind::ImageOcr | TaskKind::ImageExplain => OutcomeStructured::Extracted,
        _ => OutcomeStructured::Plain {
            examples: parse_examples(value.get("examples")),
        },
    };
    (body[..start].trim_end().to_owned(), structured)
}

/// 取字符串字段；JSON null 与缺失同样返回 None（phonetic/code_language
/// 允许缺省）。
fn text_field(value: &serde_json::Value, key: &str) -> Option<String> {
    value.get(key).and_then(|v| v.as_str()).map(str::to_owned)
}

/// 解析字符串数组（best-effort）：非字符串的坏条目跳过、好条目保留；
/// 字段整体缺失（模型没按契约给）回退为空表。
fn parse_examples(value: Option<&serde_json::Value>) -> Vec<String> {
    value
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
    use gloss_core::task::TaskKind;

    use super::{OutcomeStructured, complete, finalize_outcome, parse_structured};

    #[test]
    fn json_main_path_builds_the_word_card() {
        let raw = r#"{"phonetic":"/ɡlɒs/","note":"**gloss** 的义释","examples":["a gloss of silk","光泽的例句"]}"#;
        let outcome = complete(TaskKind::TranslateWord, raw);
        assert_eq!(outcome.kind, TaskKind::TranslateWord);
        assert_eq!(outcome.note, "**gloss** 的义释");
        assert_eq!(outcome.code_language, None);
        match outcome.structured {
            OutcomeStructured::WordCard { phonetic, examples } => {
                assert_eq!(phonetic.as_deref(), Some("/ɡlɒs/"));
                assert_eq!(examples, vec!["a gloss of silk", "光泽的例句"]);
            }
            other => panic!("expected word card, got {other:?}"),
        }
    }

    #[test]
    fn json_main_path_tolerates_null_and_missing_fields() {
        let outcome = complete(
            TaskKind::TranslateWord,
            r#"{"phonetic":null,"note":"正文","examples":[null,"好例句",42]}"#,
        );
        match outcome.structured {
            OutcomeStructured::WordCard { phonetic, examples } => {
                assert_eq!(phonetic, None, "JSON null must read as None");
                assert_eq!(examples, vec!["好例句"], "bad entries are skipped");
            }
            other => panic!("expected word card, got {other:?}"),
        }

        let bare = complete(TaskKind::TranslateWord, r#"{"note":"只有义"}"#);
        assert_eq!(
            bare.structured,
            OutcomeStructured::WordCard {
                phonetic: None,
                examples: Vec::new()
            },
            "missing subprov fields fall back per the contract"
        );
    }

    #[test]
    fn json_main_path_covers_plain_and_extracted_kinds() {
        let plain = complete(
            TaskKind::ExplainCode,
            r#"{"note":"讲解","examples":["展开一","展开二"],"code_language":"rust"}"#,
        );
        assert_eq!(plain.note, "讲解");
        assert_eq!(plain.code_language.as_deref(), Some("rust"));
        assert_eq!(
            plain.structured,
            OutcomeStructured::Plain {
                examples: vec!["展开一".into(), "展开二".into()]
            }
        );

        let sentence = complete(
            TaskKind::TranslateSentence,
            r#"{"note":"译文","examples":[]}"#,
        );
        assert_eq!(sentence.code_language, None, "仅代码任务回传语言");

        let extracted = complete(TaskKind::ImageOcr, r#"{"note":"会议纪要\n参会：产品组"}"#);
        assert_eq!(extracted.note, "会议纪要\n参会：产品组");
        assert_eq!(extracted.structured, OutcomeStructured::Extracted);
    }

    #[test]
    fn missing_note_field_hands_over_to_the_fence_fallback() {
        // note 缺失：JSON 主路径整路失败，围栏 fallback 接住旧契约输出
        //（该围栏是完整的词卡，注文剥到围栏前）。
        let raw = "旧契约正文\n```gloss\n{\"word\":\"gloss\",\"senses\":[]}\n```";
        let outcome = complete(TaskKind::TranslateWord, raw);
        assert_eq!(outcome.note, "旧契约正文");
        assert!(matches!(
            outcome.structured,
            OutcomeStructured::WordCard { .. }
        ));
    }

    #[test]
    fn non_json_reply_falls_through_to_the_fence_fallback() {
        let outcome = complete(TaskKind::ExplainCode, "整段正文");
        assert_eq!(outcome.kind, TaskKind::ExplainCode);
        assert_eq!(outcome.note, "整段正文");
        assert_eq!(outcome.code_language, None);
        assert_eq!(
            outcome.structured,
            OutcomeStructured::Plain {
                examples: Vec::new()
            }
        );
    }

    #[test]
    fn fence_fallback_pairs_with_the_raw_stream() {
        let outcome = finalize_outcome(
            TaskKind::TranslateWord,
            "**gloss**\n\n/ɡlɒs/ n. 光泽\n\n```gloss\n{\"word\":\"gloss\",\"phonetic\":\"/ɡlɒs/\",\"senses\":[{\"pos\":\"n.\",\"meaning\":\"光泽\",\"examples\":[\"a gloss of silk\"]}]}\n```",
        );
        assert_eq!(outcome.note, "**gloss**\n\n/ɡlɒs/ n. 光泽");
        match outcome.structured {
            OutcomeStructured::WordCard { phonetic, .. } => {
                assert_eq!(phonetic.as_deref(), Some("/ɡlɒs/"));
            }
            other => panic!("expected word card, got {other:?}"),
        }
    }

    #[test]
    fn fence_fallback_keeps_the_rfind_semantics() {
        // 尾随文字不进注文。
        let (body, structured) = parse_structured(
            TaskKind::ExplainCode,
            "正文\n```gloss\n{\"title\":\"摘要\"}\n```\n以上内容仅供参考",
        );
        assert_eq!(body, "正文", "trailing prose must not leak into the note");
        assert_eq!(
            structured,
            OutcomeStructured::Plain {
                examples: Vec::new()
            }
        );

        // 取最后一个围栏：正文里出现过的围栏标记不影响剥离。
        let (body, structured) = parse_structured(
            TaskKind::TranslateSentence,
            "```gloss\n{\"title\":\"早的\"}\n```\n正文\n```gloss\n{\"title\":\"晚的\"}\n```",
        );
        assert_eq!(body, "```gloss\n{\"title\":\"早的\"}\n```\n正文");
        assert_eq!(
            structured,
            OutcomeStructured::Plain {
                examples: Vec::new()
            }
        );

        // OCR 围栏损坏时无损保留全文。
        let (body, structured) =
            parse_structured(TaskKind::ImageOcr, "文本\n```gloss\n{broken\n```");
        assert_eq!(body, "文本\n```gloss\n{broken\n```");
        assert_eq!(structured, OutcomeStructured::Extracted);

        // word kind 的围栏 JSON 现行字段照常解析（无 phonetic/examples
        // 就地回退），注文剥到围栏前。
        let (body, structured) = parse_structured(
            TaskKind::TranslateWord,
            "正文\n```gloss\n{\"word\":\"gloss\"}\n```",
        );
        assert_eq!(body, "正文");
        assert_eq!(
            structured,
            OutcomeStructured::WordCard {
                phonetic: None,
                examples: Vec::new()
            }
        );
    }

    #[test]
    fn both_layers_agree_on_the_two_layer_handoff() {
        // 坏 JSON → 围栏 → kind 兜底：三层各就各位。
        let broken_json_then_bad_fence =
            complete(TaskKind::ImageOcr, "{not json\n```gloss\n{also broken\n```");
        assert_eq!(
            broken_json_then_bad_fence.structured,
            OutcomeStructured::Extracted
        );
        assert_eq!(
            broken_json_then_bad_fence.note, "{not json\n```gloss\n{also broken\n```",
            "the kind fallback keeps the whole raw text"
        );
    }
}
