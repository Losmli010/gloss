//! 任务编排（阶段二）：prompt 渲染 → 引擎流式转发。
//!
//! core 的职责面只有「渲染 → 转发 → 契约校验」：渲染出的 messages 连同
//! 已解析的模型交给引擎，流式增量经 `on_chunk` 逐条**原样**交出。不累积
//! 正文、不组装产物、不编排缓存——body 累积与缓存查/写归调用方（app 桥），
//! 完成态产物走 [`finalize_outcome`]（剥围栏 + 结构化解析的契约纯函数）。
//!
//! 模型输出契约见 prompt 模块：markdown 正文 + 末尾 ` ```gloss ` 围栏 JSON。
//! 剥离只发生在完成态（`finalize_outcome`）；流式期间转发的是含围栏的
//! 原始流，UI 按同一围栏常量截断显示。

use std::sync::Arc;

use crate::model::GlossError;
use crate::ports::{AiEngine, EngineRequest};
use crate::prompt::{PromptRegistry, STRUCTURED_FENCE};
use crate::task::{OutcomeStructured, Task, TaskKind, TaskOutcome};

/// 渲染与转发编排：只依赖端口与模板，单测用 `tests/stubs` 的 `MockEngine`
/// 驱动。
pub struct AiTaskService {
    engine: Arc<dyn AiEngine>,
    prompts: PromptRegistry,
}

impl AiTaskService {
    /// 组装：引擎由入口注入（缓存不在服务里——编排归调用方）。
    pub fn new(engine: Arc<dyn AiEngine>) -> Self {
        Self {
            engine,
            prompts: PromptRegistry::new(),
        }
    }

    /// 引擎端口：分类编排（`classify`）与任务转发共用同一引擎实例——
    /// 连接池只有一份。渲染与转发的路径仍走 [`Self::execute`]。
    pub fn engine(&self) -> &dyn AiEngine {
        self.engine.as_ref()
    }

    /// 渲染并转发一条任务：模板渲染（含模态校验，非法组合在进引擎前
    /// 拒绝）→ 引擎流式，每个增量经 `on_chunk` 原样转发 → 流走完或首错
    /// 即返回。返回 `Ok` 只表示流正常收尾——产物由调用方组装。
    ///
    /// 调用方契约（app 桥据此接线，见 `gloss_app::pipeline`）：
    /// - 缓存查/写归调用方，key 用 [`crate::cache::cache_key`]；本服务
    ///   不查缓存，命中路径由调用方在调用本方法之前直接返回；
    /// - 正文累积归调用方：在 `on_chunk` 里拼接，完成后以
    ///   [`finalize_outcome`]（`task.kind`，累积正文）组装产物；
    /// - 返回 `Err` 时**已转发的增量不撤回**——以返回值为准丢弃半截正文
    ///   （状态机据此让 TaskChunk 之后接 TaskFailed 的渲染路径可收敛）；
    /// - `on_chunk` 收到的是**含结构化 JSON 块的原始流**（剥离只发生在
    ///   `finalize_outcome`），流式渲染要隐藏围栏块由 UI 截断（首个围栏起）；
    /// - 取消不进本服务：调用方以 `CancellationToken` 竞速，丢弃本 future
    ///   即停止转发。
    pub async fn execute(
        &self,
        task: &Task,
        model: &str,
        mut on_chunk: impl FnMut(String),
    ) -> Result<(), GlossError> {
        // prompt 渲染先行：模态不合法在进引擎之前拒绝；渲染好的 messages
        // 与已解析的模型一起交给引擎（引擎不做渲染、不读配置）。日志（缓存
        // 命中/未命中、流收尾）在调用方桥上——core 只渲染与转发。
        let messages = self.prompts.render(task)?;

        let request = EngineRequest {
            kind: task.kind,
            messages,
            model: model.to_owned(),
            max_tokens: None,
        };
        let mut stream = self.engine.execute(&request).await?;
        while let Some(item) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
            match item {
                Ok(delta) => on_chunk(delta),
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

/// 完成态产物组装（契约纯函数）：从调用方累积的原始正文里剥出末尾
/// ` ```gloss … ``` ` 围栏解析为 [`OutcomeStructured`]，其余作为 markdown
/// 正文，构造 [`TaskOutcome`]。引擎失败的任务到不了这里，因此产物恒可
/// 回填缓存（写缓存归调用方）。
///
/// 围栏语义与流式态 UI 截断共用 [`STRUCTURED_FENCE`] 单点：完成态在
/// core 剥离（本函数经 [`parse_structured`]），流式态由 UI 从首个围栏起
/// 截断显示，各一处实现。
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

/// 从拼接正文中剥出结构化 JSON 块（输出契约见 prompt 模块）：末尾的
/// ` ```gloss … ``` ` 围栏解析为 [`OutcomeStructured`]，其余作为 markdown
/// 正文。围栏缺失或解析失败按 kind 回退（OCR 回退为全文提取，其余回退
/// 为无标题 Plain）。
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
/// 好条目保留；senses 字段整体缺失（模型没按契约给）返回 None 走回退。
fn parse_senses(value: Option<&serde_json::Value>) -> Option<Vec<crate::task::Sense>> {
    let entries = value?.as_array()?;
    let senses = entries
        .iter()
        .filter_map(|entry| {
            Some(crate::task::Sense {
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
    use std::sync::Arc;

    use super::{AiEngine, AiTaskService, OutcomeStructured, finalize_outcome, parse_structured};
    use crate::model::{GlossError, Lang, ScreenRect};
    use crate::stubs::engine::MockEngine;
    use crate::task::{InputHint, Task, TaskInput, TaskKind, TaskOptions};

    fn make_service(engine: &MockEngine) -> (Arc<MockEngine>, AiTaskService) {
        let engine = Arc::new(engine.clone());
        let service = AiTaskService::new(Arc::clone(&engine) as Arc<dyn AiEngine>);
        (engine, service)
    }

    fn text_task(kind: TaskKind, text: &str) -> Task {
        Task {
            kind,
            input: TaskInput::Text {
                text: text.into(),
                hint: Some(InputHint::CodeLanguage("rust".into())),
            },
            options: TaskOptions {
                target_lang: Some(Lang::Zh),
                ..Default::default()
            },
        }
    }

    #[tokio::test]
    async fn streams_chunks_in_order_verbatim() {
        let (engine, service) = make_service(&MockEngine::new().with_chunks(vec![
            Ok("# 光泽".into()),
            Ok("\n\n光泽：".into()),
            Ok("注释或反射".into()),
        ]));
        let mut forwarded = Vec::new();
        service
            .execute(
                &text_task(TaskKind::TranslateWord, "gloss"),
                "mock-model",
                |delta| forwarded.push(delta),
            )
            .await
            .expect("task should succeed");
        assert_eq!(
            forwarded,
            vec!["# 光泽", "\n\n光泽：", "注释或反射"],
            "chunks must be forwarded verbatim, assembly is the caller's job"
        );
        assert_eq!(engine.call_count(), 1);
    }

    #[tokio::test]
    async fn engine_failures_propagate() {
        let (_, service) =
            make_service(&MockEngine::new().with_execute_failure(GlossError::EngineRateLimited));
        assert_eq!(
            service
                .execute(&text_task(TaskKind::TranslateWord, "gloss"), "m", |_| {})
                .await,
            Err(GlossError::EngineRateLimited)
        );

        let (_, service) = make_service(
            &MockEngine::new().with_chunks(vec![Ok("半截".into()), Err(GlossError::EngineNetwork)]),
        );
        let mut seen = Vec::new();
        assert_eq!(
            service
                .execute(&text_task(TaskKind::TranslateWord, "gloss"), "m", |d| seen
                    .push(d))
                .await,
            Err(GlossError::EngineNetwork)
        );
        assert_eq!(
            seen,
            vec!["半截"],
            "chunks before the failure are delivered"
        );
    }

    #[tokio::test]
    async fn modality_mismatch_is_rejected_before_engine() {
        let (engine, service) = make_service(&MockEngine::new().with_chunks(vec![Ok("x".into())]));
        let mismatched = Task {
            kind: TaskKind::ImageOcr,
            input: TaskInput::Text {
                text: "not an image".into(),
                hint: None,
            },
            options: TaskOptions::default(),
        };
        assert_eq!(
            service.execute(&mismatched, "m", |_| {}).await,
            Err(GlossError::UnsupportedModality)
        );
        assert_eq!(engine.call_count(), 0, "engine must not be reached");
    }

    #[tokio::test]
    async fn image_tasks_stay_unsupported() {
        let (engine, service) = make_service(&MockEngine::new());
        let image = Task {
            kind: TaskKind::ImageOcr,
            input: TaskInput::Image {
                png: Arc::from(&b"png"[..]),
                region: ScreenRect {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
            },
            options: TaskOptions::default(),
        };
        assert_eq!(
            service.execute(&image, "m", |_| {}).await,
            Err(GlossError::UnsupportedModality)
        );
        assert_eq!(engine.call_count(), 0);
    }

    #[tokio::test]
    async fn finalize_outcome_pairs_with_execute_forwarding() {
        let (_, service) = make_service(&MockEngine::new().with_chunks(vec![
            Ok("**gloss**\n\n/ɡlɒs/ n. 光泽\n".into()),
            Ok("\n```gloss\n{\"word\":\"gloss\",\"phonetic\":\"/ɡlɒs/\",\"senses\":[{\"pos\":\"n.\",\"meaning\":\"光泽\",\"examples\":[\"a gloss of silk\"]}]}\n```".into()),
        ]));
        let mut body = String::new();
        service
            .execute(&text_task(TaskKind::TranslateWord, "gloss"), "m", |delta| {
                body.push_str(&delta)
            })
            .await
            .expect("run");
        let outcome = finalize_outcome(TaskKind::TranslateWord, &body);
        assert_eq!(outcome.kind, TaskKind::TranslateWord);
        assert_eq!(
            outcome.body, "**gloss**\n\n/ɡlɒs/ n. 光泽",
            "fence stripped in the finalized body"
        );
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
    fn phonetic_null_maps_to_none() {
        let outcome = finalize_outcome(
            TaskKind::TranslateWord,
            "正文\n```gloss\n{\"word\":\"gloss\",\"phonetic\":null,\"senses\":[]}\n```",
        );
        match outcome.structured {
            OutcomeStructured::WordCard {
                phonetic, senses, ..
            } => {
                assert_eq!(phonetic, None, "JSON null must read as None");
                assert!(senses.is_empty());
            }
            other => panic!("expected word card, got {other:?}"),
        }
    }

    #[test]
    fn bad_sense_entries_are_skipped_not_fatal() {
        let json = r#"{"word":"gloss","senses":[
            {"pos":"n.","meaning":"光泽","examples":[]},
            {"pos":"v.","meaning":null,"examples":[]},
            {"meaning":"注释"}
        ]}"#;
        let (body, structured) = parse_structured(
            TaskKind::TranslateWord,
            &format!("正文\n```gloss\n{json}\n```"),
        );
        assert_eq!(body, "正文");
        match structured {
            OutcomeStructured::WordCard { senses, .. } => {
                assert_eq!(senses.len(), 2, "two good entries survive");
                assert_eq!(senses[1].meaning, "注释");
                assert_eq!(senses[1].pos, None);
            }
            other => panic!("expected word card, got {other:?}"),
        }
    }

    #[test]
    fn trailing_text_after_fence_is_dropped() {
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
    }

    #[test]
    fn missing_structured_block_falls_back_to_plain() {
        let outcome = finalize_outcome(TaskKind::ExplainCode, "整段正文");
        assert_eq!(outcome.kind, TaskKind::ExplainCode);
        assert_eq!(outcome.body, "整段正文");
        assert_eq!(outcome.structured, OutcomeStructured::Plain { title: None });
    }

    #[test]
    fn ocr_fallback_extracts_whole_body() {
        let (body, structured) = parse_structured(TaskKind::ImageOcr, "提取到的全文");
        assert_eq!(body, "提取到的全文");
        assert!(matches!(
            structured,
            OutcomeStructured::Extracted { ref text } if text == "提取到的全文"
        ));

        let (body, structured) =
            parse_structured(TaskKind::ImageOcr, "文本\n```gloss\n{broken\n```");
        assert_eq!(body, "文本\n```gloss\n{broken\n```");
        assert!(matches!(
            structured,
            OutcomeStructured::Extracted { ref text } if text.contains("{broken")
        ));

        let (body, structured) = parse_structured(
            TaskKind::ImageOcr,
            "markdown 段落\n```gloss\n{\"text\":\"纯文本\"}\n```",
        );
        assert_eq!(body, "markdown 段落");
        assert!(matches!(
            structured,
            OutcomeStructured::Extracted { ref text } if text == "纯文本"
        ));
    }
}
