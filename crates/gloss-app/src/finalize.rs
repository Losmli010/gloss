//! 完成态解析：模型原始回复 → [`TaskOutcome`]（缓存站点与产物卡的唯一入口）。
//!
//! 两层解析，逐层退让——**模型返回非 JSON 时必有产物**：
//! 1. **JSON 主路径**（现行契约，见 `gloss_core::prompt`）：整段回复是一个
//!    JSON 对象——`note`（注，markdown 注文）放首位，其后按 kind 的结构化
//!    字段（词卡的字/音/义/例，句译与讲解的 title，提取任务的 text）。字段
//!    缺失按契约就地回退（word 空串、phonetic/title 缺省、坏 sense 条目
//!    跳过），`note` 缺失（OCR 则 `text` 缺失）才判整路失败。
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
    if let Some((note, structured)) = parse_json_outcome(kind, raw) {
        return TaskOutcome {
            kind,
            note,
            structured,
        };
    }
    finalize_outcome(kind, raw)
}

/// JSON 主路径：整段回复按一个 JSON 对象解析，`note`（注）给注文，
/// 结构化字段按 kind 就地容错。词卡与句译/讲解要求 `note` 在场（流式
/// 渐进提取的靶字段）；提取任务只要求 `text`（经文提取无注）。必需字段
/// 缺失或非字符串返回 `None`（整路失败，交围栏 fallback）。
fn parse_json_outcome(kind: TaskKind, raw: &str) -> Option<(String, OutcomeStructured)> {
    let value = serde_json::from_str::<serde_json::Value>(raw.trim()).ok()?;
    match kind {
        TaskKind::ImageOcr => {
            let text = value.get("text")?.as_str()?.to_owned();
            let note = text_field(&value, "note").unwrap_or_default();
            Some((note, OutcomeStructured::Extracted { text }))
        }
        _ => {
            let note = value.get("note")?.as_str()?.to_owned();
            let structured = match kind {
                TaskKind::TranslateWord => OutcomeStructured::WordCard {
                    word: text_field(&value, "word").unwrap_or_default(),
                    phonetic: text_field(&value, "phonetic"),
                    senses: parse_senses(value.get("senses")).unwrap_or_default(),
                },
                _ => OutcomeStructured::Plain {
                    title: text_field(&value, "title"),
                },
            };
            Some((note, structured))
        }
    }
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
pub fn finalize_outcome(kind: TaskKind, raw: &str) -> TaskOutcome {
    let (note, structured) = parse_structured(kind, raw);
    TaskOutcome {
        kind,
        note,
        structured,
    }
}

/// 从原始回复里剥出旧契约的结构化 JSON 块：末尾的 ` ```gloss … ``` ` 围栏
/// 解析为 [`OutcomeStructured`]，其余作为 markdown 注文。围栏缺失或解析
/// 失败按 kind 回退（OCR 回退为全文提取，其余回退为无标题 Plain）。
pub fn parse_structured(kind: TaskKind, raw: &str) -> (String, OutcomeStructured) {
    let body = raw;
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
        let raw = r#"{"note":"**gloss** 的释义","word":"gloss","phonetic":"/ɡlɒs/","senses":[{"pos":"n.","meaning":"光泽","examples":["a gloss of silk"]}]}"#;
        let outcome = complete(TaskKind::TranslateWord, raw);
        assert_eq!(outcome.kind, TaskKind::TranslateWord);
        assert_eq!(outcome.note, "**gloss** 的释义");
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
            r#"{"note":"正文","word":"gloss","phonetic":null}"#,
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
            r#"{"note":"译文","title":null}"#,
        );
        assert_eq!(plain.structured, OutcomeStructured::Plain { title: None });
    }

    #[test]
    fn json_main_path_skips_bad_sense_entries() {
        let raw = r#"{"note":"正文","word":"gloss","senses":[
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
            r#"{"note":"What it does","title":"摘要"}"#,
        );
        assert_eq!(plain.note, "What it does");
        assert_eq!(
            plain.structured,
            OutcomeStructured::Plain {
                title: Some("摘要".into())
            }
        );

        let ocr = complete(TaskKind::ImageOcr, r#"{"text":"纯文本"}"#);
        assert!(matches!(
            ocr.structured,
            OutcomeStructured::Extracted { ref text } if text == "纯文本"
        ));
        assert_eq!(
            ocr.note, "",
            "extraction carries no 注; the text is the product"
        );
    }

    #[test]
    fn missing_note_field_hands_over_to_the_fence_fallback() {
        // note 缺失：JSON 主路径整路失败，围栏 fallback 接住旧契约输出
        //（该围栏是完整的词卡，正文剥到围栏前）。
        let raw = "旧契约正文\n```gloss\n{\"word\":\"gloss\",\"senses\":[]}\n```";
        let outcome = complete(TaskKind::TranslateWord, raw);
        assert_eq!(outcome.note, "旧契约正文");
        assert!(matches!(
            outcome.structured,
            OutcomeStructured::WordCard { ref word, .. } if word == "gloss"
        ));
    }

    #[test]
    fn non_json_reply_falls_through_to_the_fence_fallback() {
        let outcome = complete(TaskKind::ExplainCode, "整段正文");
        assert_eq!(outcome.kind, TaskKind::ExplainCode);
        assert_eq!(outcome.note, "整段正文");
        assert_eq!(outcome.structured, OutcomeStructured::Plain { title: None });
    }

    #[test]
    fn fence_fallback_pairs_with_the_raw_stream() {
        let outcome = finalize_outcome(
            TaskKind::TranslateWord,
            "**gloss**\n\n/ɡlɒs/ n. 光泽\n\n```gloss\n{\"word\":\"gloss\",\"phonetic\":\"/ɡlɒs/\",\"senses\":[{\"pos\":\"n.\",\"meaning\":\"光泽\",\"examples\":[\"a gloss of silk\"]}]}\n```",
        );
        assert_eq!(outcome.note, "**gloss**\n\n/ɡlɒs/ n. 光泽");
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

        // word kind 缺 senses（围栏 JSON 解析不出词卡）走 kind 回退：
        // 全文无损保留、结构化落无标题 Plain（core 原语义）。
        let (body, structured) = parse_structured(
            TaskKind::TranslateWord,
            "正文\n```gloss\n{\"word\":\"gloss\"}\n```",
        );
        assert_eq!(body, "正文\n```gloss\n{\"word\":\"gloss\"}\n```");
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
            broken_json_then_bad_fence.note, "{not json\n```gloss\n{also broken\n```",
            "the kind fallback keeps the whole raw text"
        );
    }
}
