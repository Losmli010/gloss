//! 完成态解析：模型原始回复 → [`TaskOutcome`]（缓存站点与产物卡的唯一入口）。
//!
//! 两层解析，逐层退让：
//! 1. **JSON 主路径**（现行契约，见 `gloss_core::prompt`）：整段回复是一个
//!    JSON 对象——`body` 字段（markdown 正文）+ 按 kind 的结构化字段。字段
//!    缺失按契约就地回退（word 空串、phonetic/title 缺省、坏 sense 条目
//!    跳过），`body` 缺失才判整路失败。
//! 2. **围栏 fallback**（`finalize_outcome`，自 core 原样迁入）：回复不是
//!    JSON 时剥末尾 ` ```gloss … ``` ` 围栏解析——这是**旧契约**（markdown
//!    正文 + 末尾结构化块）的兼容位，模型跑偏输出旧契约时保住产物；围栏
//!    缺失或解析失败再按 kind 回退（OCR 回退为全文提取，其余回退为无标题
//!    Plain），无损保留全文。
//!
//! 两层衔接共用 [`gloss_core::prompt::STRUCTURED_FENCE`] 单点：流式期间
//! UI 按纯 JSON 渐进提取 `body`（见 `crate::ui::popup`），完成态以本模块
//! 的产物为准。

use gloss_core::prompt::STRUCTURED_FENCE;
use gloss_core::task::{OutcomeStructured, Sense, TaskKind, TaskOutcome};

/// 完成态产物组装（契约纯函数）：先走 JSON 主路径，失败退围栏 fallback。
/// 引擎失败的任务到不了这里，因此产物恒可回填缓存（写缓存归调用方）。
pub fn complete(kind: TaskKind, raw: &str) -> TaskOutcome {
    if let Some((body, structured)) = parse_json_outcome(kind, raw) {
        return TaskOutcome {
            kind,
            body,
            structured,
        };
    }
    finalize_outcome(kind, raw)
}

/// JSON 主路径：整段回复按一个 JSON 对象解析，`body` 给正文，结构化字段
/// 按 kind 就地容错。`body` 缺失或非字符串返回 `None`（整路失败，交围栏
/// fallback）。
fn parse_json_outcome(kind: TaskKind, raw: &str) -> Option<(String, OutcomeStructured)> {
    let value = serde_json::from_str::<serde_json::Value>(raw.trim()).ok()?;
    let body = value.get("body")?.as_str()?.to_owned();
    let structured = match kind {
        TaskKind::TranslateWord => OutcomeStructured::WordCard {
            word: text_field(&value, "word").unwrap_or_default(),
            phonetic: text_field(&value, "phonetic"),
            senses: parse_senses(value.get("senses")).unwrap_or_default(),
        },
        TaskKind::ImageOcr => OutcomeStructured::Extracted {
            text: text_field(&value, "text").unwrap_or_else(|| body.clone()),
        },
        _ => OutcomeStructured::Plain {
            title: text_field(&value, "title"),
        },
    };
    Some((body, structured))
}

/// 围栏 fallback（旧契约，自 gloss-core 原样迁入）：从原始回复里剥出末尾
/// ` ```gloss … ``` ` 围栏解析为 [`OutcomeStructured`]，其余作为 markdown
/// 正文。围栏语义与流式态 UI 的渐进提取不同源（后者按 JSON body 提取），
/// 本函数只在完成态兜底旧契约输出。
///
/// 剥离边界：定位**最后一个**围栏标记，其后（含闭合围栏与契约外尾随
/// 文字）一律不进正文——正常输出契约下模型不会有尾随内容；围栏缺失或
/// 解析失败按 kind 回退（OCR 回退为全文提取，其余回退为无标题 Plain），
/// 回退路径无损保留全文（不做有损剥离）。
pub fn finalize_outcome(kind: TaskKind, body: &str) -> TaskOutcome {
    let (body, structured) = parse_structured(kind, body);
    TaskOutcome {
        kind,
        body,
        structured,
    }
}

/// 从拼接正文中剥出旧契约的结构化 JSON 块：末尾的 ` ```gloss … ``` ` 围栏
/// 解析为 [`OutcomeStructured`]，其余作为 markdown 正文。围栏缺失或解析
/// 失败按 kind 回退（OCR 回退为全文提取，其余回退为无标题 Plain）。
pub fn parse_structured(kind: TaskKind, body: &str) -> (String, OutcomeStructured) {
    let fallback = || -> (String, OutcomeStructured) {
        let owned = body.to_owned();
        let structured = match kind {
            TaskKind::ImageOcr => OutcomeStructured::Extracted {
                text: owned.clone(),
            },
            _ => OutcomeStructured::Plain { title: None },
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
        TaskKind::TranslateWord => {
            let Some(senses) = parse_senses(value.get("senses")) else {
                return fallback();
            };
            OutcomeStructured::WordCard {
                word: text_field(&value, "word").unwrap_or_default(),
                phonetic: text_field(&value, "phonetic"),
                senses,
            }
        }
        TaskKind::ImageOcr => match text_field(&value, "text") {
            Some(text) => OutcomeStructured::Extracted { text },
            None => return fallback(),
        },
        _ => OutcomeStructured::Plain {
            title: text_field(&value, "title"),
        },
    };
    (body[..start].trim_end().to_owned(), structured)
}

/// 取字符串字段；JSON null 与缺失同样返回 None（phonetic/title 允许缺省）。
fn text_field(value: &serde_json::Value, key: &str) -> Option<String> {
    value.get(key).and_then(|v| v.as_str()).map(str::to_owned)
}

/// 解析释义数组（best-effort）：meaning 缺失/非字符串的坏条目跳过、
/// 好条目保留；senses 字段整体缺失（模型没按契约给）返回 None 交调用方
/// 分层回退。
fn parse_senses(value: Option<&serde_json::Value>) -> Option<Vec<Sense>> {
    let entries = value?.as_array()?;
    let senses = entries
        .iter()
        .filter_map(|entry| {
            Some(Sense {
                pos: text_field(entry, "pos"),
                meaning: text_field(entry, "meaning")?,
                examples: entry
                    .get("examples")
                    .and_then(|v| v.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|v| v.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect();
    Some(senses)
}

#[cfg(test)]
mod tests {
    use gloss_core::task::TaskKind;

    use super::{OutcomeStructured, complete, finalize_outcome, parse_structured};

    #[test]
    fn json_main_path_builds_the_word_card() {
        let raw = r#"{"body":"**gloss** 的释义","word":"gloss","phonetic":"/ɡlɒs/","senses":[{"pos":"n.","meaning":"光泽","examples":["a gloss of silk"]}]}"#;
        let outcome = complete(TaskKind::TranslateWord, raw);
        assert_eq!(outcome.kind, TaskKind::TranslateWord);
        assert_eq!(outcome.body, "**gloss** 的释义");
        match outcome.structured {
            OutcomeStructured::WordCard {
                word,
                phonetic,
                senses,
            } => {
                assert_eq!(word, "gloss");
                assert_eq!(phonetic.as_deref(), Some("/ɡlɒs/"));
                assert_eq!(senses.len(), 1);
                assert_eq!(senses[0].meaning, "光泽");
                assert_eq!(senses[0].examples, vec!["a gloss of silk".to_owned()]);
            }
            other => panic!("expected word card, got {other:?}"),
        }
    }

    #[test]
    fn json_main_path_tolerates_null_and_missing_fields() {
        let outcome = complete(
            TaskKind::TranslateWord,
            r#"{"body":"正文","word":"gloss","phonetic":null}"#,
        );
        match outcome.structured {
            OutcomeStructured::WordCard {
                phonetic, senses, ..
            } => {
                assert_eq!(phonetic, None, "JSON null must read as None");
                assert!(senses.is_empty(), "missing senses falls back to empty");
            }
            other => panic!("expected word card, got {other:?}"),
        }

        let plain = complete(
            TaskKind::TranslateSentence,
            r#"{"body":"译文","title":null}"#,
        );
        assert_eq!(plain.structured, OutcomeStructured::Plain { title: None });
    }

    #[test]
    fn json_main_path_skips_bad_sense_entries() {
        let raw = r#"{"body":"正文","word":"gloss","senses":[
            {"pos":"n.","meaning":"光泽","examples":[]},
            {"pos":"v.","meaning":null,"examples":[]},
            {"meaning":"注释"}
        ]}"#;
        let outcome = complete(TaskKind::TranslateWord, raw);
        match outcome.structured {
            OutcomeStructured::WordCard { senses, .. } => {
                assert_eq!(senses.len(), 2, "two good entries survive");
                assert_eq!(senses[1].meaning, "注释");
                assert_eq!(senses[1].pos, None);
            }
            other => panic!("expected word card, got {other:?}"),
        }
    }

    #[test]
    fn json_main_path_covers_plain_and_extracted_kinds() {
        let plain = complete(
            TaskKind::ExplainCode,
            r#"{"body":"What it does","title":"摘要"}"#,
        );
        assert_eq!(plain.body, "What it does");
        assert_eq!(
            plain.structured,
            OutcomeStructured::Plain {
                title: Some("摘要".into())
            }
        );

        let ocr = complete(TaskKind::ImageOcr, r#"{"body":"**提取**","text":"纯文本"}"#);
        assert!(matches!(
            ocr.structured,
            OutcomeStructured::Extracted { ref text } if text == "纯文本"
        ));
        assert_eq!(ocr.body, "**提取**", "the body stays markdown as-is");
    }

    #[test]
    fn missing_body_field_hands_over_to_the_fence_fallback() {
        // body 缺失：JSON 主路径整路失败，围栏 fallback 接住旧契约输出。
        let raw = "旧契约正文\n```gloss\n{\"word\":\"gloss\",\"senses\":[]}\n```";
        let outcome = complete(TaskKind::TranslateWord, raw);
        assert_eq!(outcome.body, "旧契约正文");
        assert!(matches!(
            outcome.structured,
            OutcomeStructured::WordCard { ref word, .. } if word == "gloss"
        ));
    }

    #[test]
    fn non_json_reply_falls_through_to_the_fence_fallback() {
        let outcome = complete(TaskKind::ExplainCode, "整段正文");
        assert_eq!(outcome.kind, TaskKind::ExplainCode);
        assert_eq!(outcome.body, "整段正文");
        assert_eq!(outcome.structured, OutcomeStructured::Plain { title: None });
    }

    #[test]
    fn fence_fallback_pairs_with_the_raw_stream() {
        let outcome = finalize_outcome(
            TaskKind::TranslateWord,
            "**gloss**\n\n/ɡlɒs/ n. 光泽\n\n```gloss\n{\"word\":\"gloss\",\"phonetic\":\"/ɡlɒs/\",\"senses\":[{\"pos\":\"n.\",\"meaning\":\"光泽\",\"examples\":[\"a gloss of silk\"]}]}\n```",
        );
        assert_eq!(outcome.body, "**gloss**\n\n/ɡlɒs/ n. 光泽");
        match outcome.structured {
            OutcomeStructured::WordCard { senses, .. } => {
                assert_eq!(senses.len(), 1);
                assert_eq!(senses[0].examples, vec!["a gloss of silk".to_owned()]);
            }
            other => panic!("expected word card, got {other:?}"),
        }
    }

    #[test]
    fn fence_fallback_keeps_the_rfind_semantics() {
        // 尾随文字不进正文。
        let (body, structured) = parse_structured(
            TaskKind::ExplainCode,
            "正文\n```gloss\n{\"title\":\"摘要\"}\n```\n以上内容仅供参考",
        );
        assert_eq!(body, "正文", "trailing prose must not leak into the body");
        assert_eq!(
            structured,
            OutcomeStructured::Plain {
                title: Some("摘要".into())
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
                title: Some("晚的".into())
            }
        );

        // OCR 围栏损坏时无损保留全文。
        let (body, structured) =
            parse_structured(TaskKind::ImageOcr, "文本\n```gloss\n{broken\n```");
        assert_eq!(body, "文本\n```gloss\n{broken\n```");
        assert!(matches!(
            structured,
            OutcomeStructured::Extracted { ref text } if text.contains("{broken")
        ));

        // 坏 sense 条目整体不可解析时也走 kind 回退。
        let (body, structured) = parse_structured(
            TaskKind::TranslateWord,
            "正文\n```gloss\n{\"word\":\"gloss\"}\n```",
        );
        assert_eq!(body, "正文");
        assert_eq!(structured, OutcomeStructured::Plain { title: None });
    }

    #[test]
    fn both_layers_agree_on_the_two_layer_handoff() {
        // 坏 JSON → 围栏 → kind 兜底：三层各就各位。
        let broken_json_then_bad_fence =
            complete(TaskKind::ImageOcr, "{not json\n```gloss\n{also broken\n```");
        assert!(matches!(
            broken_json_then_bad_fence.structured,
            OutcomeStructured::Extracted { .. }
        ));
        assert_eq!(
            broken_json_then_bad_fence.body, "{not json\n```gloss\n{also broken\n```",
            "the kind fallback keeps the whole raw text"
        );
    }
}
