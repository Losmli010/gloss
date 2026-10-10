//! LLM 任务服务（编排层）：意图定型 → prompt 渲染 → 引擎流式转发。
//!
//! 一次 `run` 的完整前半程都在这里：输入校验 → 意图定型（文本走 LLM
//! 分类，失败落 [`CLASSIFY_FALLBACK`]；图像输入自带显式意图，恒定映射
//! ImageExplain、跳过分类）→ `on_classified` 回调 → 任务形成（prompt 渲
//! 染 + 模态校验）→ 流式执行（原始增量经 `on_chunk` 逐条转交，同时累积）
//! → 返回原始完整文本与判定的 kind。**零缓存零配置**：模型取自
//! `options.model`，本层与缓存彻底无关——查/写缓存、完成态解析与产物组
//! 装都归调用方（app 桥）。
//!
//! 输出契约见 prompt 模块：模型按契约直接返回纯 JSON 对象（`note` 义 +
//! 按 kind 疏证）；本层只搬运原始文本，不解析——解析归调用方的完成态
//! （`gloss_app::finalize`，JSON 主路径 + 旧围栏契约 fallback）。
//!
//! 子模块 [`llm`] / [`sse`] 是 [`AiEngine`] 端口的内建传输适配：OpenAI
//! 兼容端点的流式客户端与 SSE 增量解码。

pub mod llm;
pub mod sse;

use std::sync::Arc;

use crate::classify::{CLASSIFY_FALLBACK, CLASSIFY_KINDS, classify};
use crate::log::{thread, warn};
use crate::model::GlossError;
use crate::ports::{AiEngine, EngineRequest};
use crate::prompt::PromptRegistry;
use crate::task::{Task, TaskInput, TaskKind, TaskOptions};

/// 一次 `run` 的产出：判定的任务类型 + 引擎返回的**原始**完整文本。
/// 解析成产物归调用方（完成态解析在 `gloss_app::finalize`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutput {
    /// 本次任务最终执行的类型（含分类兜底）。
    pub kind: TaskKind,
    /// 模型回复的原始完整文本（未解析、未剥离）。
    pub raw: String,
}

/// 渲染、分类与转发编排：只依赖端口与模板，单测用 `tests/stubs` 的
/// `MockEngine` 驱动。
pub struct AiTaskService {
    engine: Arc<dyn AiEngine>,
    prompts: PromptRegistry,
}

impl AiTaskService {
    /// 组装：引擎由入口注入。
    pub fn new(engine: Arc<dyn AiEngine>) -> Self {
        Self {
            engine,
            prompts: PromptRegistry::new(),
        }
    }

    /// 执行一条任务：入口校验（模型非空、模态放行）→ 意图定型（文本走
    /// LLM 分类，失败落兜底；图像输入恒定映射 ImageExplain、跳过分类，
    /// `on_classified` 恒发）→ prompt 渲染 → 引擎流式，每个增量经
    /// `on_chunk` 原样转交 → 流走完返回原始文本。
    ///
    /// `input` 与 `options` 按值接收：任务在执行期间独占两者，调用方
    /// （app 桥）从通道解出后即交出所有权。
    ///
    /// 调用方契约（app 桥据此接线，见 `gloss_app::pipeline`）：
    /// - `on_classified` 在**任何** chunk 之前恰好调用一次（含失败兜底
    ///   kind 与图像的固定 kind）——界面的任务标签以它为准；
    /// - `on_chunk` 收到的是**原始流**（纯 JSON 契约下就是模型的原始
    ///   输出），流式显示的渐进提取与完成态解析都在调用方；返回值
    ///   `raw` 是同一份文本的整体累积；
    /// - 返回 `Err` 时**已转发的增量不撤回**——以返回值为准丢弃半截正文
    ///   （状态机据此让 TaskChunk 之后接 TaskFailed 的渲染路径可收敛）；
    /// - 取消不进本服务：调用方以 `CancellationToken` 竞速，丢弃本 future
    ///   即停止转发。
    pub async fn run(
        &self,
        input: TaskInput,
        options: TaskOptions,
        mut on_classified: impl FnMut(TaskKind),
        mut on_chunk: impl FnMut(String),
    ) -> Result<RunOutput, GlossError> {
        // 模型随 options 冻结而来：空白 = 未配置（引擎侧同规则兜一层），
        // 在花任何一次往返之前拒绝。
        if options.model.trim().is_empty() {
            return Err(GlossError::Config("empty model id".into()));
        }

        // 意图定型：文本输入没有显式意图，交给 LLM 分类（失败落常量兜
        // 底；兜底有痕迹但无内容——warn 只记错误类别，不含选区原文与模型
        // 回复）；图像输入的取材动作即显式意图（剪贴板图片任务就是图像解
        // 读），恒定映射 ImageExplain、跳过分类——显式意图恒定映射与分类
        // 常量兜底同哲学，`CLASSIFY_KINDS` 不动，引擎调用少一次往返。
        let kind = match &input {
            TaskInput::Text { .. } => match classify(
                self.engine.as_ref(),
                &options.model,
                options.prompt_locale.unwrap_or_default(),
                &CLASSIFY_KINDS,
                &input,
            )
            .await
            {
                Ok(kind) => kind,
                Err(error) => {
                    warn!(
                        thread = thread::TOKIO,
                        error = %error,
                        "classification failed, falling back to the default kind"
                    );
                    CLASSIFY_FALLBACK
                }
            },
            TaskInput::Image { .. } => TaskKind::ImageExplain,
            // 语音是预留模态：取材端口未落地，任何 kind 都不放行。
            TaskInput::Audio { .. } => return Err(GlossError::UnsupportedModality),
        };
        on_classified(kind);

        // 任务形成与执行：渲染含模态校验（非法组合在进引擎前拒绝），模型
        // 恒取 options 冻结值。
        let model = options.model.clone();
        let task = Task {
            kind,
            input,
            options,
        };
        let messages = self.prompts.render(&task)?;
        let request = EngineRequest {
            messages,
            model,
            max_tokens: None,
        };
        let mut stream = self.engine.execute(request).await?;
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
        Ok(RunOutput { kind, raw: body })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::{AiEngine, AiTaskService, RunOutput};
    use crate::model::{GlossError, Lang};
    use crate::stubs::engine::MockEngine;
    use crate::task::{TaskInput, TaskKind, TaskOptions};

    fn make_service(engine: &MockEngine) -> (Arc<MockEngine>, AiTaskService) {
        let engine = Arc::new(engine.clone());
        let service = AiTaskService::new(Arc::clone(&engine) as Arc<dyn AiEngine>);
        (engine, service)
    }

    fn text_input(text: &str) -> TaskInput {
        TaskInput::Text { text: text.into() }
    }

    fn options() -> TaskOptions {
        TaskOptions {
            target_lang: Some(Lang::Zh),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn classified_kind_arrives_before_any_chunk() {
        let (_, service) = make_service(
            &MockEngine::new().with_chunks(vec![Ok("{\"note\":\"你".into()), Ok("好\"}".into())]),
        );
        let events = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let classified = Arc::clone(&events);
        let chunks = Arc::clone(&events);
        service
            .run(
                text_input("hello"),
                options(),
                move |kind| {
                    classified.lock().expect("events").push(match kind {
                        TaskKind::TranslateWord => "classified:TranslateWord",
                        other => panic!("unexpected kind {other:?}"),
                    });
                },
                move |delta| {
                    chunks.lock().expect("events").push("chunk");
                    let _ = delta;
                },
            )
            .await
            .expect("run should succeed");
        let events = events.lock().expect("events");
        assert_eq!(events.first(), Some(&"classified:TranslateWord"));
        assert_eq!(events.iter().filter(|event| **event == "chunk").count(), 2);
    }

    #[test]
    fn classify_failure_falls_back_and_the_task_still_runs() {
        let _serial = crate::log::test_support::lock_dispatchers();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("runtime should build");
        let line = (0..3)
            .find_map(|_| {
                let (engine, service) = make_service(
                    &MockEngine::new()
                        .with_execute_failure_once(GlossError::EngineRateLimited)
                        .with_chunks(vec![Ok("兜底产物".into())]),
                );
                let logs = crate::log::capture(|| {
                    let output = rt
                        .block_on(service.run(text_input("第一次划词"), options(), |_| {}, |_| {}))
                        .expect("the fallback must let the task run");
                    assert_eq!(output.kind, TaskKind::TranslateWord);
                });
                assert_eq!(engine.call_count(), 2, "one classify call + one task call");
                logs.lines()
                    .find(|line| line.contains("classification failed"))
                    .inspect(|line| {
                        assert!(
                            !line.contains("第一次划词"),
                            "the fallback warn must not carry the selection content: {line}"
                        );
                    })
                    .map(str::to_owned)
            })
            .is_some();
        assert!(line, "the fallback must leave a trace");
    }

    #[tokio::test]
    async fn raw_text_is_returned_verbatim() {
        let (_, service) = make_service(
            &MockEngine::new()
                .with_chunks(vec![Ok("{\"note\":\"光泽".into()), Ok("：注释\"}".into())]),
        );
        let output = service
            .run(text_input("gloss"), options(), |_| {}, |_| {})
            .await
            .expect("run should succeed");
        assert_eq!(output.raw, "{\"note\":\"光泽：注释\"}");
    }

    #[tokio::test]
    async fn audio_input_is_still_rejected_before_anything() {
        let (engine, service) = make_service(&MockEngine::new());
        assert_eq!(
            service
                .run(
                    TaskInput::Audio {
                        bytes: std::sync::Arc::from(&b"au"[..]),
                        duration_hint: None,
                    },
                    options(),
                    |_| {},
                    |_| {}
                )
                .await,
            Err(GlossError::UnsupportedModality)
        );
        assert_eq!(engine.call_count(), 0);
    }

    #[tokio::test]
    async fn image_input_skips_classification_and_lands_on_image_explain() {
        let (engine, service) = make_service(
            &MockEngine::new().with_chunks(vec![Ok("{\"note\":\"图\"".into()), Ok("}".into())]),
        );
        let events = Arc::new(Mutex::new(Vec::<String>::new()));
        let classified = Arc::clone(&events);
        let chunks = Arc::clone(&events);
        let output = service
            .run(
                TaskInput::Image {
                    png: std::sync::Arc::from(&b"png"[..]),
                    region: None,
                },
                options(),
                move |kind| {
                    classified
                        .lock()
                        .expect("events")
                        .push(format!("classified:{kind:?}"));
                },
                move |delta| {
                    chunks
                        .lock()
                        .expect("events")
                        .push(format!("chunk:{delta}"));
                },
            )
            .await
            .expect("image task should run");
        assert_eq!(output.kind, TaskKind::ImageExplain);
        assert_eq!(output.raw, "{\"note\":\"图\"}");
        let events = events.lock().expect("events");
        assert_eq!(
            events
                .iter()
                .filter(|event| event.starts_with("classified:"))
                .count(),
            1,
            "the fixed kind is reported exactly once: {events:?}"
        );
        assert_eq!(
            events.first().map(String::as_str),
            Some("classified:ImageExplain"),
            "the fixed kind precedes every chunk: {events:?}"
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.starts_with("chunk:"))
                .count(),
            2
        );
        assert_eq!(
            engine.call_count(),
            1,
            "image input skips classification: one engine call in total"
        );
    }

    #[tokio::test]
    async fn blank_model_is_a_config_failure_before_the_engine() {
        let (engine, service) = make_service(&MockEngine::new());
        let options = TaskOptions {
            model: "   ".into(),
            ..Default::default()
        };
        assert!(matches!(
            service
                .run(text_input("hello"), options, |_| {}, |_| {})
                .await,
            Err(GlossError::Config(_))
        ));
        assert_eq!(engine.call_count(), 0);
    }

    #[tokio::test]
    async fn engine_failures_propagate_and_earlier_chunks_are_kept() {
        let (_, service) =
            make_service(&MockEngine::new().with_execute_failure(GlossError::EngineRateLimited));
        assert_eq!(
            service
                .run(text_input("gloss"), options(), |_| {}, |_| {})
                .await,
            Err(GlossError::EngineRateLimited),
            "a classify-stage failure propagates as-is"
        );

        let (_, service) = make_service(
            &MockEngine::new().with_chunks(vec![Ok("半截".into()), Err(GlossError::EngineNetwork)]),
        );
        let mut seen = Vec::new();
        let result: Result<RunOutput, GlossError> = service
            .run(text_input("gloss"), options(), |_| {}, |d| seen.push(d))
            .await;
        assert_eq!(result, Err(GlossError::EngineNetwork));
        assert_eq!(
            seen,
            vec!["半截"],
            "chunks before the failure are delivered"
        );
    }
}
