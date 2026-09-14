//! 任务编排（M3-T6，06 §4.2 阶段二）：prompt 渲染 → 缓存 → 引擎流式 →
//! 结构化解析 → 回填缓存。
//!
//! 运行在 tokio 后台（通道③的消费者）。流式增量经 `on_chunk` 回调逐条
//! 交出（调用方映射为通道④的 `Event::TaskChunk`）；产物按 prompt 模块
//! 定下的输出契约解析（` ```gloss ` 围栏 JSON），解析失败按 best-effort
//! 回退为整段正文——装饰性结构化信息不值得让任务失败。

use std::sync::Arc;

use crate::cache::cache_key;
use crate::model::GlossError;
use crate::ports::{AiEngine, Cache};
use crate::prompt::{PromptRegistry, STRUCTURED_FENCE};
use crate::task::{OutcomeStructured, Task, TaskKind, TaskOutcome};

#[cfg(any(test, feature = "test-util"))]
pub mod mock;

/// 全链路编排：只依赖端口与模板，单测用 [`mock::MockEngine`] 驱动。
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
    /// `model` 参与缓存 key：同任务换模型不命中旧产物。
    pub async fn execute(
        &self,
        task: &Task,
        model: &str,
        mut on_chunk: impl FnMut(String),
    ) -> Result<TaskOutcome, GlossError> {
        // prompt 渲染先行：模态不合法在进缓存/引擎之前拒绝；messages
        // 本身由 M4 的 LlmClient 组装请求时使用，这里先钉住校验与接线点。
        let _messages = self.prompts.render(task)?;

        let key = cache_key(task, model);
        if let Some(hit) = self.cache.get(key) {
            return Ok(hit);
        }

        let mut stream = self.engine.execute(task).await?;
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
fn parse_structured(kind: TaskKind, body: &str) -> (String, OutcomeStructured) {
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

/// 解析释义数组；字段缺失（模型没按契约给）返回 None 走回退。
fn parse_senses(value: Option<&serde_json::Value>) -> Option<Vec<crate::task::Sense>> {
    let entries = value?.as_array()?;
    let mut senses = Vec::with_capacity(entries.len());
    for entry in entries {
        senses.push(crate::task::Sense {
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
        });
    }
    Some(senses)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{AiEngine, AiTaskService, OutcomeStructured};
    use crate::cache::MokaCache;
    use crate::engine::mock::MockEngine;
    use crate::model::{GlossError, Lang, ScreenRect};
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

    /// 验收标准：流式 chunk 正确拼接——增量按序转发，正文完整落进产物。
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

    /// 验收标准：缓存命中不调引擎——第二次执行直接取缓存，引擎计数不变。
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

    /// 换模型不命中缓存（key 含模型 id）。
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

    /// 结构化解析：词卡按输出契约解析，围栏块从正文剥离。
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

    /// 模型没按契约给围栏：正文原样保留、回退为无标题 Plain——装饰信息
    /// 缺失不让任务失败（文本 kind 经 execute 全链路验证）。
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

    /// OCR 的围栏缺失/解析失败回退为全文提取（parse_structured 直测——
    /// 图像任务在 M5-T4 前被模态渲染拦下，走不到 execute）。
    #[test]
    fn ocr_fallback_extracts_whole_body() {
        let (body, structured) = super::parse_structured(TaskKind::ImageOcr, "提取到的全文");
        assert_eq!(body, "提取到的全文");
        assert!(matches!(
            structured,
            OutcomeStructured::Extracted { ref text } if text == "提取到的全文"
        ));

        // 有围栏但 JSON 非法：同样回退为全文，且围栏残片留在正文里
        //（不做有损剥离）。
        let (body, structured) =
            super::parse_structured(TaskKind::ImageOcr, "文本\n```gloss\n{broken\n```");
        assert_eq!(body, "文本\n```gloss\n{broken\n```");
        assert!(matches!(
            structured,
            OutcomeStructured::Extracted { ref text } if text.contains("{broken")
        ));

        // 正常路径：围栏内 text 字段被提取，正文剥离。
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

    /// 验收标准：错误路径——execute 整体失败原样上抛；流中失败终止任务。
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

    /// 模态不合法在引擎之前被拒（渲染阶段），引擎零调用。
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

    /// 图像任务在 M5-T4 前渲染为 UnsupportedModality（占位语义贯穿编排）。
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
