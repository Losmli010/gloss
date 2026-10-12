//! LLM 任务服务（编排层）：意图定型 → prompt 渲染 → 引擎流式转发（含
//! 契约修复轮）。
//!
//! 一次 `run` 的完整前半程都在这里：输入校验 → 意图定型（文本走 LLM
//! 分类——违约与请求异常在 `classify` 内部消化，修复 [`CLASSIFY_REPAIRS`]
//! 轮后落 [`CLASSIFY_FALLBACK`]，恒 `Ok`；图像输入自带显式意图，恒定映射
//! ImageExplain、跳过分类）→ `on_classified` 回调 → 任务形成（prompt 渲
//! 染 + 模态校验）→ 流式执行（原始增量经 `on_chunk` 逐条转交，同时累积）
//! → **任务契约校验**（`schema::validate_task`）→ 违约进修复轮（对话形态
//! `[system][user 原文/图][assistant 上一次坏回复][user 纠错指令]`，EmptyReply
//! 原样重发），预算 [`TASK_REPAIRS`] 轮，每轮开始前经 `on_repair` 通知调
//! 用方重置 UI → 通过即返回，耗尽则原始文本原样上交（完成态解析落降级
//! 产物）。引擎错误（网络/鉴权/限流/JSON 模式不支持/多模态不支持/中途
//! 协议错误）一律不重试、原样上抛。**零缓存零配置**：模型取自
//! `options.model`，本层与缓存彻底无关——查/写缓存、完成态解析与产物组
//! 装都归调用方（app 桥）。
//!
//! 输出契约见 prompt 模块与 `crate::schema`：模型按契约直接返回纯 JSON
//! 对象（`note` 义 + `structured` 疏证）；本层搬运原始文本并做**硬约束**
//! 校验（整段合法 JSON 对象 + note 非空）——产物组装归调用方的完成态
//! （`gloss_app::finalize`，经 `schema::validate_task` 两分支：校验过出
//! 正常产物，违约落降级产物）。
//!
//! 子模块 [`llm`] / [`sse`] 是 [`AiEngine`] 端口的内建传输适配：OpenAI
//! 兼容端点的流式客户端与 SSE 增量解码。

pub mod llm;
pub mod sse;

use std::sync::Arc;

use crate::classify::{CLASSIFY_FALLBACK, CLASSIFY_KINDS, classify};
use crate::log::{thread, warn};
use crate::model::GlossError;
use crate::ports::{AiEngine, EngineRequest, TaskChunk};
use crate::prompt::{PromptRegistry, repair_conversation};
use crate::schema::{self, Violation};
use crate::task::{Task, TaskInput, TaskKind, TaskOptions};

/// 任务契约违约的修复轮预算：任务共 `1 + TASK_REPAIRS` 次尝试；耗尽后
/// 原始文本原样上交，完成态解析落降级产物。
pub const TASK_REPAIRS: u32 = 3;

/// 服务端以内容审查收尾的 `finish_reason`。
const FINISH_CONTENT_FILTER: &str = "content_filter";

/// 服务端以长度截断收尾的 `finish_reason`（修复指令据此附加精确提示）。
const FINISH_LENGTH: &str = "length";

/// 内容审查的失败诊断（固定措辞，进失败卡与日志）：内容被服务端拦截，
/// 重发同请求无益，不重试直接失败卡。
const CONTENT_FILTER_DIAGNOSTIC: &str = "reply stopped by the server content filter";

/// 零增量完成的识别：传输适配器（`llm::SseStream`）对「成功收尾但一个
/// 增量都没有」的坏响应统一发这个固定诊断——它是坏**回复**而非坏请求，
/// 重试循环据此转 [`Violation::EmptyReply`] 进修复轮。
fn is_empty_completion(error: &GlossError) -> bool {
    matches!(error, GlossError::EngineResponse(text) if text == llm::EMPTY_COMPLETION)
}

/// 一次 `run` 的产出：判定的任务类型 + 引擎返回的**原始**完整文本。
/// 解析成产物归调用方（完成态解析在 `gloss_app::finalize`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutput {
    /// 本次任务最终执行的类型（含分类兜底）。
    pub kind: TaskKind,
    /// 模型回复的原始完整文本（未解析、未剥离）。修复轮耗尽后是最后
    /// 一轮的违约原文——完成态解析按违约落降级产物。
    pub raw: String,
    /// 服务端声明的结束原因（`choices[0].finish_reason`，如 `stop` /
    /// `length` / `content_filter`）；服务端未声明 `[DONE]`（连接关闭、
    /// 脚本桩）为 `None`。以最后一轮为准。
    pub finish_reason: Option<String>,
    /// 任务契约违约触发的修复轮次数（0 = 一次通过）。
    pub repairs: u32,
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
    /// LLM 分类，异常内部消化、恒 `Ok`；图像输入恒定映射 ImageExplain、
    /// 跳过分类，`on_classified` 恒发）→ prompt 渲染 → 引擎流式，每个
    /// 增量经 `on_chunk` 原样转交 → 契约校验，违约进修复轮（`on_repair`
    /// 先于每轮首个增量发出）→ 通过或预算耗尽后返回原始文本。
    ///
    /// `input` 与 `options` 按值接收：任务在执行期间独占两者，调用方
    /// （app 桥）从通道解出后即交出所有权。
    ///
    /// 调用方契约（app 桥据此接线，见 `gloss_app::pipeline`）：
    /// - `on_classified` 在**任何** chunk 之前恰好调用一次（含失败兜底
    ///   kind 与图像的固定 kind）——界面的任务标签以它为准；
    /// - `on_chunk` 收到的是**原始流**（纯 JSON 契约下就是模型的原始
    ///   输出），流式显示的渐进提取与完成态解析都在调用方；返回值
    ///   `raw` 是最后一轮文本的整体累积；
    /// - `on_repair` 在每个修复轮开始前恰好调用一次（违约轮的 chunk 已
    ///   全部送达之后）——调用方重置流式显示并切换「重新注解」；
    /// - 返回 `Err` 时**已转发的增量不撤回**——以返回值为准丢弃半截正文
    ///   （状态机据此让 TaskChunk 之后接 TaskFailed 的渲染路径可收敛）；
    ///   `Err` 只来自引擎与请求构造（不重试），契约违约不产生 `Err`；
    /// - 取消不进本服务：调用方以 `CancellationToken` 竞速，丢弃本 future
    ///   即停止转发（修复轮也是 future 内的 await 点，竞速天然覆盖）。
    pub async fn run(
        &self,
        input: TaskInput,
        options: TaskOptions,
        mut on_classified: impl FnMut(TaskKind),
        mut on_chunk: impl FnMut(String),
        mut on_repair: impl FnMut(),
    ) -> Result<RunOutput, GlossError> {
        // 模型随 options 冻结而来：空白 = 未配置（引擎侧同规则兜一层），
        // 在花任何一次往返之前拒绝。
        if options.model.trim().is_empty() {
            return Err(GlossError::Config("empty model id".into()));
        }

        // 意图定型：文本输入没有显式意图，交给 LLM 分类（分类内部消化
        // 违约与请求异常，恒 `Ok`；这里的 Err 臂是非文本输入的防御路径）；
        // 图像输入的取材动作即显式意图（剪贴板图片任务就是图像解读），
        // 恒定映射 ImageExplain、跳过分类——显式意图恒定映射与分类常量
        // 兜底同哲学，`CLASSIFY_KINDS` 不动，引擎调用少一次往返。
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
        // 恒取 options 冻结值。base 是首轮消息；修复轮以它为底座拼
        // [assistant 坏回复][user 纠错指令]（图像部件随 base 零拷贝重发）。
        let model = options.model.clone();
        let locale = options.prompt_locale.unwrap_or_default();
        let task = Task {
            kind,
            input,
            options,
        };
        let base = self.prompts.render(&task)?;
        let mut messages = base.clone();
        let mut repairs = 0u32;
        loop {
            let request = EngineRequest {
                messages,
                model: model.clone(),
                max_tokens: None,
            };
            let mut stream = self.engine.execute(request).await?;
            let mut body = String::new();
            let mut finish_reason = None;
            let mut empty_completion = false;
            while let Some(item) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
                match item {
                    Ok(TaskChunk::Delta(delta)) => {
                        body.push_str(&delta);
                        on_chunk(delta);
                    }
                    Ok(TaskChunk::Done {
                        finish_reason: reason,
                    }) => finish_reason = reason,
                    Err(error) => {
                        // 零增量的「empty completion」是坏回复而非坏请求：
                        // 转 EmptyReply 进修复轮；其余引擎错误（含已有增量
                        // 后的中途协议错误）按请求异常上抛，不重试。
                        if body.is_empty() && is_empty_completion(&error) {
                            empty_completion = true;
                            break;
                        }
                        return Err(error);
                    }
                }
            }

            // 内容审查：服务端拦了内容，重发无益——不重试直接失败卡。
            if finish_reason.as_deref() == Some(FINISH_CONTENT_FILTER) {
                return Err(GlossError::EngineResponse(CONTENT_FILTER_DIAGNOSTIC.into()));
            }

            let violation = if empty_completion {
                Some(Violation::EmptyReply)
            } else {
                schema::validate_task(kind, &body).err()
            };
            let Some(violation) = violation else {
                return Ok(RunOutput {
                    kind,
                    raw: body,
                    finish_reason,
                    repairs,
                });
            };
            if repairs >= TASK_REPAIRS {
                // 预算耗尽：违约原文原样上交，完成态解析落降级产物。
                warn!(
                    thread = thread::TOKIO,
                    reason = %violation,
                    "task repairs exhausted, handing the raw reply to the completion parser"
                );
                return Ok(RunOutput {
                    kind,
                    raw: body,
                    finish_reason,
                    repairs,
                });
            }
            repairs += 1;
            // 每轮违约 warn 只记类别 + 轮次，无选区与回复原文（安全红线）。
            warn!(
                thread = thread::TOKIO,
                attempt = repairs,
                reason = %violation,
                "task reply violated the contract, preparing a repair round"
            );
            let instruction = self.prompts.render_repair(
                locale,
                violation,
                &self.prompts.task_contract_line(kind, locale),
                finish_reason.as_deref() == Some(FINISH_LENGTH),
            );
            // EmptyReply 无内容可纠：原样重发，不带 assistant 轮。
            let bad = match violation {
                Violation::EmptyReply => None,
                _ => Some(body.as_str()),
            };
            messages = repair_conversation(&base, bad, instruction);
            // 修复轮照常流式，开始前发事件重置 UI（清流式正文、切「重新注解」）。
            on_repair();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::{AiEngine, AiTaskService, RunOutput, TASK_REPAIRS};
    use crate::engine::llm::EMPTY_COMPLETION;
    use crate::model::{GlossError, Lang};
    use crate::ports::{BoxFuture, EngineRequest, TaskChunk, TaskStream};
    use crate::prompt::{MessageContent, Role};
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

    fn image_input() -> TaskInput {
        TaskInput::Image {
            png: Arc::from(&b"png"[..]),
            region: None,
        }
    }

    fn options() -> TaskOptions {
        TaskOptions {
            target_lang: Some(Lang::Zh),
            ..Default::default()
        }
    }

    fn delta(chunk: &str) -> Result<TaskChunk, GlossError> {
        Ok(TaskChunk::Delta(chunk.into()))
    }

    fn done(reason: &str) -> Result<TaskChunk, GlossError> {
        Ok(TaskChunk::Done {
            finish_reason: Some(reason.into()),
        })
    }

    fn message_texts(request: &EngineRequest) -> Vec<(Role, String)> {
        request
            .messages
            .iter()
            .map(|message| {
                let text = match &message.content {
                    MessageContent::Text(text) => text.clone(),
                    MessageContent::Parts(_) => String::new(),
                };
                (message.role, text)
            })
            .collect()
    }

    struct DoneScript {
        deltas: Vec<String>,
        finish_reason: Option<String>,
    }

    impl AiEngine for DoneScript {
        fn execute(
            &self,
            _request: crate::ports::EngineRequest,
        ) -> BoxFuture<'static, Result<TaskStream, GlossError>> {
            let deltas = self.deltas.clone();
            let finish_reason = self.finish_reason.clone();
            Box::pin(async move {
                let items: Vec<Result<TaskChunk, GlossError>> = deltas
                    .into_iter()
                    .map(|delta| Ok(TaskChunk::Delta(delta)))
                    .chain(std::iter::once(Ok(TaskChunk::Done { finish_reason })))
                    .collect();
                Ok(Box::pin(futures::stream::iter(items)) as TaskStream)
            })
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
                        TaskKind::General => "classified:General",
                        other => panic!("unexpected kind {other:?}"),
                    });
                },
                move |delta| {
                    chunks.lock().expect("events").push("chunk");
                    let _ = delta;
                },
                || {},
            )
            .await
            .expect("run should succeed");
        let events = events.lock().expect("events");
        assert_eq!(events.first(), Some(&"classified:General"));
        assert_eq!(events.iter().filter(|event| **event == "chunk").count(), 2);
    }

    #[test]
    fn classify_request_failure_falls_back_without_retrying() {
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
                        .with_chunks(vec![Ok("{\"note\":\"兜底产物\"}".into())]),
                );
                let logs = crate::log::capture(|| {
                    let output = rt
                        .block_on(service.run(
                            text_input("第一次划词"),
                            options(),
                            |_| {},
                            |_| {},
                            || {},
                        ))
                        .expect("the digest inside classify must let the task run");
                    assert_eq!(output.kind, TaskKind::General);
                });
                assert_eq!(
                    engine.call_count(),
                    2,
                    "the failed classify call is not retried: one classify + one task call"
                );
                logs.lines()
                    .find(|line| line.contains("classify request failed"))
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
            .run(text_input("gloss"), options(), |_| {}, |_| {}, || {})
            .await
            .expect("run should succeed");
        assert_eq!(output.raw, "{\"note\":\"光泽：注释\"}");
    }

    #[tokio::test]
    async fn run_output_carries_the_server_finish_reason() {
        let service = AiTaskService::new(Arc::new(DoneScript {
            deltas: vec!["{\"note\":\"光".into(), "泽\"}".into()],
            finish_reason: Some("stop".into()),
        }));
        let output = service
            .run(text_input("gloss"), options(), |_| {}, |_| {}, || {})
            .await
            .expect("run should succeed");
        assert_eq!(output.raw, "{\"note\":\"光泽\"}");
        assert_eq!(output.finish_reason.as_deref(), Some("stop"));

        let service = AiTaskService::new(Arc::new(DoneScript {
            deltas: vec!["{\"note\":\"光".into(), "泽\"}".into()],
            finish_reason: Some("length".into()),
        }));
        let output = service
            .run(text_input("gloss"), options(), |_| {}, |_| {}, || {})
            .await
            .expect("run should succeed");
        assert_eq!(output.finish_reason.as_deref(), Some("length"));
        assert_eq!(
            output.repairs, 0,
            "a contract-valid reply never repairs, finish reason notwithstanding"
        );
    }

    #[tokio::test]
    async fn run_output_finish_reason_stays_none_without_a_done_marker() {
        let (_, service) =
            make_service(&MockEngine::new().with_chunks(vec![Ok("{\"note\":\"光泽\"}".into())]));
        let output = service
            .run(text_input("gloss"), options(), |_| {}, |_| {}, || {})
            .await
            .expect("run should succeed");
        assert_eq!(output.finish_reason, None);
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
                    |_| {},
                    || {}
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
                image_input(),
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
                || {},
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
                .run(text_input("hello"), options, |_| {}, |_| {}, || {})
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
                .run(text_input("gloss"), options(), |_| {}, |_| {}, || {})
                .await,
            Err(GlossError::EngineRateLimited),
            "the task-stage engine failure propagates as-is (classify digests its own)"
        );

        let (_, service) = make_service(
            &MockEngine::new().with_chunks(vec![Ok("半截".into()), Err(GlossError::EngineNetwork)]),
        );
        let mut seen = Vec::new();
        let result: Result<RunOutput, GlossError> = service
            .run(
                text_input("gloss"),
                options(),
                |_| {},
                |d| seen.push(d),
                || {},
            )
            .await;
        assert_eq!(result, Err(GlossError::EngineNetwork));
        assert_eq!(
            seen,
            vec!["半截"],
            "chunks before the failure are delivered"
        );
    }

    #[tokio::test]
    async fn a_contract_violation_repairs_with_the_bad_reply_in_the_history() {
        let bad_reply = "MARKER-bad-reply-7f3a 整段散文，不是 JSON";
        let good_reply = "{\"note\":\"图面有折线\",\"structured\":{\"examples\":[]}}";
        let (engine, service) = make_service(
            &MockEngine::new()
                .with_script_queue(vec![vec![delta(bad_reply)], vec![delta(good_reply)]]),
        );
        let repairs = Arc::new(Mutex::new(0u32));
        let repair_sink = Arc::clone(&repairs);
        let output = service
            .run(
                image_input(),
                options(),
                |_| {},
                |_| {},
                move || {
                    *repair_sink.lock().expect("repairs") += 1;
                },
            )
            .await
            .expect("the repaired round should converge");
        assert_eq!(output.repairs, 1);
        assert_eq!(output.raw, good_reply);
        assert_eq!(*repairs.lock().expect("repairs"), 1);
        assert_eq!(
            engine.call_count(),
            2,
            "one violated round + one repair round"
        );

        let requests = engine.recorded_requests();
        assert_eq!(requests[0].messages.len(), 2, "the first round is bare");
        let repair_roles: Vec<Role> = requests[1].messages.iter().map(|m| m.role).collect();
        assert_eq!(
            repair_roles,
            [Role::System, Role::User, Role::Assistant, Role::User],
            "repair conversation: [system][user][assistant][user]"
        );
        assert!(
            matches!(&requests[1].messages[1].content, MessageContent::Parts(_)),
            "the image rides the repair conversation again"
        );
        assert_eq!(
            message_texts(&requests[1])[2].1,
            bad_reply,
            "the bad reply rides as the assistant history"
        );
        let instruction = message_texts(&requests[1])[3].1.clone();
        assert!(
            instruction.contains("reply is not a bare JSON object"),
            "the instruction carries the fixed violation wording: {instruction}"
        );
        assert!(
            instruction.contains("{\"note\":\"…\""),
            "the instruction carries the contract example line: {instruction}"
        );
        assert!(
            !instruction.contains("MARKER-bad-reply"),
            "the instruction must not quote the bad reply: {instruction}"
        );
        assert!(
            !instruction.contains("empty reply"),
            "the wording matches the actual violation: {instruction}"
        );
    }

    #[test]
    fn the_repair_round_warn_carries_the_category_and_round_only() {
        let _serial = crate::log::test_support::lock_dispatchers();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("runtime should build");
        let bad_reply = "MARKER-选区与回复都不得出现在日志";
        let line = (0..3)
            .find_map(|_| {
                let (_, service) = make_service(&MockEngine::new().with_script_queue(vec![
                    vec![delta(bad_reply)],
                    vec![delta("{\"note\":\"图\"}")],
                ]));
                let logs = crate::log::capture(|| {
                    let output = rt
                        .block_on(service.run(image_input(), options(), |_| {}, |_| {}, || {}))
                        .expect("the repair should converge");
                    assert_eq!(output.repairs, 1);
                });
                logs.lines()
                    .find(|line| line.contains("task reply violated the contract"))
                    .inspect(|line| {
                        assert!(
                            !line.contains("MARKER"),
                            "the violation warn must not carry reply content: {line}"
                        );
                        assert!(
                            line.contains("reply is not a bare JSON object"),
                            "the warn carries the fixed category: {line}"
                        );
                        assert!(
                            line.contains("\"attempt\":1"),
                            "the warn carries the round number: {line}"
                        );
                    })
                    .map(str::to_owned)
            })
            .is_some();
        assert!(line, "the violated round must leave a trace");
    }

    #[tokio::test]
    async fn an_empty_reply_repairs_by_resending_without_an_assistant_turn() {
        let (engine, service) = make_service(
            &MockEngine::new()
                .with_script_queue(vec![Vec::new(), vec![delta("{\"note\":\"图\"}")]]),
        );
        let output = service
            .run(image_input(), options(), |_| {}, |_| {}, || {})
            .await
            .expect("the resent round should converge");
        assert_eq!(output.repairs, 1);
        assert_eq!(output.raw, "{\"note\":\"图\"}");
        let requests = engine.recorded_requests();
        assert_eq!(requests.len(), 2);
        let repair_roles: Vec<Role> = requests[1].messages.iter().map(|m| m.role).collect();
        assert_eq!(
            repair_roles,
            [Role::System, Role::User],
            "EmptyReply has nothing to repair: resend as-is, no assistant turn"
        );
    }

    #[tokio::test]
    async fn a_zero_increment_empty_completion_enters_the_repair_path() {
        let (engine, service) = make_service(&MockEngine::new().with_script_queue(vec![
            vec![Err(GlossError::EngineResponse(EMPTY_COMPLETION.into()))],
            vec![delta("{\"note\":\"图\"}")],
        ]));
        let output = service
            .run(image_input(), options(), |_| {}, |_| {}, || {})
            .await
            .expect("the empty completion must repair, not fail");
        assert_eq!(output.repairs, 1);
        assert_eq!(engine.call_count(), 2);
    }

    #[tokio::test]
    async fn task_repairs_are_budgeted_then_the_raw_reply_is_handed_over() {
        let bad_reply = "MARKER-still-violating";
        let (engine, service) =
            make_service(&MockEngine::new().with_script_queue(vec![vec![delta(bad_reply)]]));
        let repairs = Arc::new(Mutex::new(0u32));
        let repair_sink = Arc::clone(&repairs);
        let output = service
            .run(
                image_input(),
                options(),
                |_| {},
                |_| {},
                move || {
                    *repair_sink.lock().expect("repairs") += 1;
                },
            )
            .await
            .expect("exhausted repairs hand the raw reply over, not an error");
        assert_eq!(output.repairs, TASK_REPAIRS);
        assert_eq!(output.raw, bad_reply, "the last violating reply rides out");
        assert_eq!(*repairs.lock().expect("repairs"), TASK_REPAIRS);
        assert_eq!(
            engine.call_count(),
            1 + TASK_REPAIRS as usize,
            "one first round plus one round per repair"
        );
    }

    #[tokio::test]
    async fn task_stage_engine_errors_are_never_retried() {
        let (engine, service) =
            make_service(&MockEngine::new().with_execute_failure(GlossError::EngineNetwork));
        assert_eq!(
            service
                .run(image_input(), options(), |_| {}, |_| {}, || {})
                .await,
            Err(GlossError::EngineNetwork),
        );
        assert_eq!(engine.call_count(), 1, "engine errors are never repaired");

        let (_, service) = make_service(&MockEngine::new().with_chunks(vec![Err(
            GlossError::EngineResponse("unparsable stream chunk: x".into()),
        )]));
        assert!(
            matches!(
                service
                    .run(image_input(), options(), |_| {}, |_| {}, || {})
                    .await,
                Err(GlossError::EngineResponse(_))
            ),
            "a zero-increment protocol error is a request exception, not an EmptyReply"
        );
    }

    #[tokio::test]
    async fn a_content_filter_finish_fails_without_a_retry() {
        let (engine, service) = make_service(&MockEngine::new().with_script_queue(vec![vec![
            delta("{\"note\":\"被拦"),
            done("content_filter"),
        ]]));
        let result = service
            .run(image_input(), options(), |_| {}, |_| {}, || {})
            .await;
        match result {
            Err(GlossError::EngineResponse(detail)) => {
                assert_eq!(detail, "reply stopped by the server content filter");
            }
            other => panic!("expected a content filter failure, got {other:?}"),
        }
        assert_eq!(engine.call_count(), 1, "content filter is terminal");
    }

    #[tokio::test]
    async fn a_length_finish_augments_the_repair_instruction() {
        let (engine, service) = make_service(&MockEngine::new().with_script_queue(vec![
            vec![delta("{\"note\":\"被截断"), done("length")],
            vec![
                delta("{\"note\":\"完整\",\"structured\":{\"examples\":[]}}"),
                done("stop"),
            ],
        ]));
        let output = service
            .run(image_input(), options(), |_| {}, |_| {}, || {})
            .await
            .expect("the truncation repair should converge");
        assert_eq!(output.repairs, 1);
        assert_eq!(output.finish_reason.as_deref(), Some("stop"));
        let requests = engine.recorded_requests();
        assert_eq!(requests.len(), 2);
        let instruction = message_texts(&requests[1])[3].1.clone();
        assert!(
            instruction.contains("输出被截断，请重新完整输出"),
            "a length finish adds the truncation hint: {instruction}"
        );
    }

    #[tokio::test]
    async fn a_fenced_reply_repairs_like_any_other_violation() {
        let (engine, service) = make_service(&MockEngine::new().with_script_queue(vec![
            vec![delta("```json\n{\"note\":\"围栏也违约\"}\n```")],
            vec![delta("{\"note\":\"图\"}")],
        ]));
        let output = service
            .run(image_input(), options(), |_| {}, |_| {}, || {})
            .await
            .expect("fenced output repairs like any other violation");
        assert_eq!(output.repairs, 1);
        assert_eq!(engine.call_count(), 2);
    }

    #[tokio::test]
    async fn cancellation_cuts_through_the_repair_round() {
        let engine = MockEngine::new()
            .with_chunk_delay(Duration::from_millis(150))
            .with_script_queue(vec![
                vec![delta("{\"note\":\"被截断")],
                vec![delta("{\"note\":\"完整\"}")],
            ]);
        let (_, service) = make_service(&engine);
        let repair_started = Arc::new(tokio::sync::Notify::new());
        let sink = Arc::clone(&repair_started);
        let mut run = std::pin::pin!(service.run(
            image_input(),
            options(),
            |_| {},
            |_| {},
            move || sink.notify_one(),
        ));
        let outcome = tokio::select! {
            _ = async {
                repair_started.notified().await;
            } => true,
            result = &mut run => {
                result.expect("the repair should converge without cancellation");
                false
            }
        };
        assert!(
            outcome,
            "the racing cancellation must win while the repair round streams"
        );
    }
}
