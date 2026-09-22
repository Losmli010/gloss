//! 任务编排（阶段二）：prompt 渲染 → 缓存 → 引擎流式 →
//! 结构化解析 → 回填缓存。
//!
//! 运行在 tokio 后台（通道③的消费者）。流式增量经 `on_chunk` 回调逐条
//! 交出（调用方映射为通道④的 `Event::TaskChunk`）；产物按 prompt 模块
//! 定下的输出契约解析（` ```gloss ` 围栏 JSON），解析失败按 best-effort
//! 回退为整段正文。

use std::sync::Arc;

use crate::cache::cache_key;
use crate::log::{debug, info};
use crate::model::GlossError;
use crate::ports::{AiEngine, Cache, EngineRequest};
use crate::prompt::{PromptRegistry, STRUCTURED_FENCE};
use crate::task::{OutcomeStructured, Task, TaskKind, TaskOutcome};

/// 全链路编排：只依赖端口与模板，单测用 `tests/stubs` 的 `MockEngine` 驱动。
pub struct AiTaskService {
    engine: Arc<dyn AiEngine>,
    cache: Arc<dyn Cache>,
    prompts: PromptRegistry,
}

impl AiTaskService {
    /// 组装：引擎与缓存由入口注入。
    pub fn new(engine: Arc<dyn AiEngine>, cache: Arc<dyn Cache>) -> Self {
        Self {
            engine,
            cache,
            prompts: PromptRegistry::new(),
        }
    }

    /// 执行一条任务：模板渲染（含模态校验）→ 缓存命中即返回（不调引
    /// 擎）→ 未命中走引擎流式，每个增量经 `on_chunk` 转发 → 拼接正文并
    /// 解析结构化字段 → 回填缓存。
    ///
    /// 调用方契约：
    /// - 返回 `Err` 时**已转发的增量不撤回**——以返回值为准丢弃半截产物
    ///   （状态机据此让 TaskChunk 之后接 TaskFailed 的渲染路径可收敛）；
    /// - `on_chunk` 收到的是**含结构化 JSON 块的原始流**（剥离只发生在
    ///   完成时的 `body`），流式渲染若要隐藏围栏块由 UI 调用方过滤
    ///   （渲染层接手）；
    /// - 引擎失败的任务不写缓存，下次触发重新执行。
    ///
    /// `model` 参与缓存 key：同任务换模型不命中旧产物。
    pub async fn execute(
        &self,
        task: &Task,
        model: &str,
        mut on_chunk: impl FnMut(String),
    ) -> Result<TaskOutcome, GlossError> {
        // prompt 渲染先行：模态不合法在进缓存/引擎之前拒绝；渲染好的
        // messages 与已解析的模型一起交给引擎（引擎不做渲染、不读配置）。
        let messages = self.prompts.render(task)?;

        let key = cache_key(task, model);
        if let Some(hit) = self.cache.get(key) {
            info!(
                kind = ?task.kind,
                model = %model,
                "cache hit, engine call skipped"
            );
            return Ok(hit);
        }
        debug!(
            kind = ?task.kind,
            model = %model,
            "cache miss, calling the engine"
        );

        let request = EngineRequest {
            kind: task.kind,
            messages,
            model: model.to_owned(),
        };
        let mut stream = self.engine.execute(&request).await?;
        let mut body = String::new();
        while let Some(item) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
            match item {
                Ok(delta) => {
                    body.push_str(&delta);
                    on_chunk(delta);
                }
                Err(error) => return Err(error),
            }
        }

        let (body, structured) = parse_structured(task.kind, &body);
        let outcome = TaskOutcome {
            kind: task.kind,
            body,
            structured,
        };
        self.cache.set(key, outcome.clone());
        Ok(outcome)
    }
}

/// 从拼接正文中剥出结构化 JSON 块（输出契约见 prompt 模块）：末尾的
/// ` ```gloss … ``` ` 围栏解析为 [`OutcomeStructured`]，其余作为 markdown
/// 正文。围栏缺失或解析失败按 kind 回退（OCR 回退为全文提取，其余回退
/// 为无标题 Plain）。
///
/// 剥离边界：定位**最后一个**围栏标记，其后（含闭合围栏与契约外尾随
/// 文字）一律不进正文——正常输出契约下模型不会有尾随内容；回退路径
/// 则无损保留全文（不做有损剥离）。
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

    use super::{AiEngine, AiTaskService, OutcomeStructured};
    use crate::cache::MokaCache;
    use crate::model::{GlossError, Lang, ScreenRect};
    use crate::stubs::engine::MockEngine;
    use crate::task::{InputHint, Task, TaskInput, TaskKind, TaskOptions};

    fn make_service(engine: &MockEngine) -> (Arc<MockEngine>, AiTaskService) {
        let engine = Arc::new(engine.clone());
        let service = AiTaskService::new(
            Arc::clone(&engine) as Arc<dyn AiEngine>,
            Arc::new(MokaCache::new()),
        );
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
    async fn streams_chunks_in_order_and_assembles_body() {
        let (engine, service) = make_service(&MockEngine::new().with_chunks(vec![
            Ok("# 光泽".into()),
            Ok("\n\n光泽：".into()),
            Ok("注释或反射".into()),
        ]));
        let mut forwarded = Vec::new();
        let outcome = service
            .execute(
                &text_task(TaskKind::TranslateWord, "gloss"),
                "mock-model",
                |delta| forwarded.push(delta),
            )
            .await
            .expect("task should succeed");
        assert_eq!(forwarded, vec!["# 光泽", "\n\n光泽：", "注释或反射"]);
        assert_eq!(
            outcome.body, "# 光泽\n\n光泽：注释或反射",
            "no structured block in script, body kept verbatim"
        );
        assert_eq!(engine.call_count(), 1);
    }

    #[tokio::test]
    async fn cache_hit_skips_the_engine() {
        let (engine, service) = make_service(
            &MockEngine::new().with_chunks(vec![Ok("第一".into()), Ok("次产物".into())]),
        );
        let task = text_task(TaskKind::TranslateSentence, "hello world");
        let first = service
            .execute(&task, "mock-model", |_| {})
            .await
            .expect("first run");
        assert_eq!(engine.call_count(), 1);

        let second = service
            .execute(&task, "mock-model", |_| {})
            .await
            .expect("second run");
        assert_eq!(second, first, "cached outcome must be identical");
        assert_eq!(engine.call_count(), 1, "cache hit must not reach engine");
    }

    #[tokio::test]
    async fn different_model_misses_the_cache() {
        let (engine, service) =
            make_service(&MockEngine::new().with_chunks(vec![Ok("产物".into())]));
        let task = text_task(TaskKind::TranslateSentence, "hello");
        service
            .execute(&task, "model-a", |_| {})
            .await
            .expect("run a");
        service
            .execute(&task, "model-b", |_| {})
            .await
            .expect("run b");
        assert_eq!(engine.call_count(), 2, "model switch must re-execute");
    }

    #[tokio::test]
    async fn word_card_is_parsed_from_structured_block() {
        let script = vec![
            Ok("**gloss**\n\n/ɡlɒs/ n. 光泽\n".into()),
            Ok("\n```gloss\n{\"word\":\"gloss\",\"phonetic\":\"/ɡlɒs/\",\"senses\":[{\"pos\":\"n.\",\"meaning\":\"光泽\",\"examples\":[\"a gloss of silk\"]}]}\n```".into()),
        ];
        let (_, service) = make_service(&MockEngine::new().with_chunks(script));
        let outcome = service
            .execute(&text_task(TaskKind::TranslateWord, "gloss"), "m", |_| {})
            .await
            .expect("run");
        assert_eq!(
            outcome.body, "**gloss**\n\n/ɡlɒs/ n. 光泽",
            "fence stripped"
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

    #[tokio::test]
    async fn phonetic_null_maps_to_none() {
        let script = vec![
            Ok("正文\n".into()),
            Ok("\n```gloss\n{\"word\":\"gloss\",\"phonetic\":null,\"senses\":[]}\n```".into()),
        ];
        let (_, service) = make_service(&MockEngine::new().with_chunks(script));
        let outcome = service
            .execute(&text_task(TaskKind::TranslateWord, "gloss"), "m", |_| {})
            .await
            .expect("run");
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
        let (body, structured) = super::parse_structured(
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
        let (body, structured) = super::parse_structured(
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

    #[tokio::test]
    async fn missing_structured_block_falls_back_to_plain() {
        let (_, service) =
            make_service(&MockEngine::new().with_chunks(vec![Ok("整段正文".into())]));
        let plain = service
            .execute(&text_task(TaskKind::ExplainCode, "unused"), "m", |_| {})
            .await
            .expect("run");
        assert_eq!(plain.body, "整段正文");
        assert_eq!(plain.structured, OutcomeStructured::Plain { title: None });
    }

    #[test]
    fn ocr_fallback_extracts_whole_body() {
        let (body, structured) = super::parse_structured(TaskKind::ImageOcr, "提取到的全文");
        assert_eq!(body, "提取到的全文");
        assert!(matches!(
            structured,
            OutcomeStructured::Extracted { ref text } if text == "提取到的全文"
        ));

        let (body, structured) =
            super::parse_structured(TaskKind::ImageOcr, "文本\n```gloss\n{broken\n```");
        assert_eq!(body, "文本\n```gloss\n{broken\n```");
        assert!(matches!(
            structured,
            OutcomeStructured::Extracted { ref text } if text.contains("{broken")
        ));

        let (body, structured) = super::parse_structured(
            TaskKind::ImageOcr,
            "markdown 段落\n```gloss\n{\"text\":\"纯文本\"}\n```",
        );
        assert_eq!(body, "markdown 段落");
        assert!(matches!(
            structured,
            OutcomeStructured::Extracted { ref text } if text == "纯文本"
        ));
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
}
