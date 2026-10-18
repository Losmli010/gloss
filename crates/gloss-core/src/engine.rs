//! LLM 任务服务（编排层）：意图定型 → prompt 渲染 → 流式转发 → 契约修复轮。
//!
//! 文本输入先走一次 LLM 分类（异常在 [`crate::classify`] 内消化，恒有 kind），
//! 图像输入固定映射 [`TaskKind::ImageExplain`] 并跳过分类。引擎错误（网络/
//! 鉴权/限流/JSON 模式/模态）一律不重试、原样上抛；契约违约走 [`TASK_REPAIRS`]
//! 轮修复，耗尽后把原始文本交给调用方去降级。
//! **零缓存零配置**：模型取自 `options.model`，查/写缓存与产物组装归调用方。

pub mod llm;
pub mod sse;

use std::sync::Arc;

use crate::classify::{CLASSIFY_FALLBACK, CLASSIFY_KINDS, classify};
use crate::log::{thread, warn};
use crate::model::GlossError;
use crate::ports::{AiEngine, EngineRequest, TaskChunk};
use crate::prompt::{ChatMessage, PromptRegistry};
use crate::repair::{RepairStage, next_round};
use crate::schema::{self, Violation};
use crate::task::{Task, TaskInput, TaskKind, TaskOptions};

/// 任务契约违约的修复轮预算：任务共 `1 + TASK_REPAIRS` 次尝试；耗尽后原始
/// 文本原样上交，由调用方落降级产物。
pub const TASK_REPAIRS: u32 = 3;

/// 单轮流式回复的累积字节上限：卡片正文远用不到这个量级，而无上限的累积
/// 会把「模型失控输出」放大成无界内存（流式期间逐 chunk 追加）。超限按请求
/// 异常失败——半截超长回复不可能过 JSON 契约，进修复轮只会重跑同样的输出。
const MAX_REPLY_BYTES: usize = 1024 * 1024;

/// 服务端以内容审查收尾的 `finish_reason`。
const FINISH_CONTENT_FILTER: &str = "content_filter";

/// 服务端以长度截断收尾的 `finish_reason`（修复指令据此附加精确提示）。
const FINISH_LENGTH: &str = "length";

/// 内容审查的失败诊断：内容被服务端拦截，重发同请求无益，不重试。
const CONTENT_FILTER_DIAGNOSTIC: &str = "reply stopped by the server content filter";

/// 一次 `run` 的产出：判定的任务类型 + 引擎返回的**原始**完整文本。
/// 解析成产物归调用方。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutput {
    /// 本次任务最终执行的类型（含分类兜底）。
    pub kind: TaskKind,
    /// 模型回复的原始完整文本（未解析、未剥离）。修复轮耗尽后是最后一轮的
    /// 违约原文——调用方据此落降级产物。
    pub raw: String,
    /// 服务端声明的结束原因（`choices[0].finish_reason`）；服务端没给
    /// `[DONE]`（连接关闭、脚本桩）为 `None`。以最后一轮为准。
    pub finish_reason: Option<String>,
    /// 契约违约触发的修复轮次数（0 = 一次通过）。
    pub repairs: u32,
}

/// 渲染、分类与转发编排：只依赖端口与模板。
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

    /// 执行一条任务：入口校验（模型非空、模态放行）→ 意图定型 → prompt 渲染
    /// → [`run_task_rounds`] 执行任务段。
    ///
    /// `input` 与 `options` 按值接收：任务在执行期间独占两者，调用方从通道
    /// 解出后即交出所有权。
    ///
    /// 调用方契约：
    /// - `on_classified` 在**任何** chunk 之前恰好调用一次（含兜底 kind 与
    ///   图像的固定 kind）——界面的任务标签以它为准；
    /// - `on_chunk` / `on_repair` 与返回值的契约见 [`run_task_rounds`]；
    /// - 取消不进本服务：调用方以 `CancellationToken` 竞速，丢弃本 future
    ///   即停止转发（修复轮也在 future 内，竞速天然覆盖）。
    pub async fn run(
        &self,
        input: TaskInput,
        options: TaskOptions,
        mut on_classified: impl FnMut(TaskKind),
        on_chunk: impl FnMut(String),
        on_repair: impl FnMut(),
    ) -> Result<RunOutput, GlossError> {
        // 模型随 options 冻结而来：空白 = 未配置（引擎侧同规则兜一层），
        // 在花任何一次往返之前拒绝。
        if options.model.trim().is_empty() {
            return Err(GlossError::Config("empty model id".into()));
        }

        // 意图定型：文本输入没有显式意图，交给 LLM 分类（分类恒 Ok，这里的
        // Err 臂只是防御）；图像输入的取材动作即显式意图——剪贴板图片就是
        // 图像解读，固定映射并跳过分类，省一次往返。
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
        };
        on_classified(kind);

        // 任务形成与执行：构造即过模态约束（非法组合在进引擎前拒绝），
        // 模型恒取 options 冻结值（借自本函数存活的 task，不再复制）；
        // 修复指令语言同源——prompt_locale 一次任务只认触发时定下的那一份。
        let locale = options.prompt_locale.unwrap_or_default();
        let task = Task::new(kind, input, options)?;
        let base = self.prompts.render(&task)?;
        run_task_rounds(
            self.engine.as_ref(),
            &self.prompts,
            kind,
            base,
            &task.options().model,
            locale,
            on_chunk,
            on_repair,
        )
        .await
    }
}

/// 一轮回复的传输结果：累积正文、服务端结束原因与「零增量完成」标记。
struct RoundReply {
    body: String,
    finish_reason: Option<String>,
    /// 传输层以「成功收尾但一个增量都没有」终结：坏回复而非坏请求。
    empty_completion: bool,
}

/// 消费一轮流：增量经 `on_chunk` 逐条转交并累积，终结帧只取原因。
/// 超预算与中途协议错误按请求异常上抛（已转发的增量不撤回）。
async fn collect_round(
    engine: &dyn AiEngine,
    request: EngineRequest,
    on_chunk: &mut impl FnMut(String),
) -> Result<RoundReply, GlossError> {
    let mut stream = engine.execute(request).await?;
    let mut body = String::new();
    let mut finish_reason = None;
    let mut empty_completion = false;
    while let Some(item) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
        match item {
            Ok(TaskChunk::Delta(delta)) => {
                if body.len().saturating_add(delta.len()) > MAX_REPLY_BYTES {
                    // 已转发的增量不撤回（调用方契约）；超限的失败以返回值
                    // 为准丢弃半截正文。
                    return Err(GlossError::EngineResponse(
                        "streamed reply exceeds the size budget".into(),
                    ));
                }
                body.push_str(&delta);
                on_chunk(delta);
            }
            Ok(TaskChunk::Done {
                finish_reason: reason,
            }) => finish_reason = reason,
            Err(error) => {
                // 零增量的「empty completion」是坏回复而非坏请求：转
                // EmptyReply 进修复轮；其余引擎错误（含已有增量后的中途
                // 协议错误）按请求异常上抛，不重试。
                if body.is_empty() && matches!(error, GlossError::EmptyCompletion) {
                    empty_completion = true;
                    break;
                }
                return Err(error);
            }
        }
    }
    Ok(RoundReply {
        body,
        finish_reason,
        empty_completion,
    })
}

/// 任务段执行循环（意图定型之后的完整执行段）：流式转发（原始增量经
/// `on_chunk` 逐条转交，同时累积）→ **任务契约校验**（`schema::validate_task`）
/// → 违约进修复轮（对话形态
/// `[system][user 原文/图][assistant 上一次坏回复][user 纠错指令]`，EmptyReply
/// 原样重发），预算 [`TASK_REPAIRS`] 轮，每轮开始前经 `on_repair` 通知调
/// 用方重置 UI → 通过即返回，耗尽则原始文本原样上交（完成态解析落降级
/// 产物）。引擎错误（网络/鉴权/限流/JSON 模式不支持/多模态不支持/中途
/// 协议错误）一律不重试、原样上抛。
///
/// 独立成 pub 供 gloss-eval 的 live 轨直达任务段：评测以数据集给定的
/// kind 渲染底座（`PromptRegistry::render` 的产物），与生产消费同一循环，
/// 评测数字度量的才是生产行为（`RunOutput.repairs` 即本函数的修复轮次）。
///
/// 调用方契约（[`AiTaskService::run`] 与评测共用）：
/// - `on_chunk` 收到的是**原始流**（纯 JSON 契约下就是模型的原始输出），
///   流式显示的渐进提取与完成态解析都在调用方；修复轮的增量接在违约轮
///   之后继续到达（`on_repair` 是唯一的轮边界信号）；返回值 `raw` 是最后
///   一轮文本的整体累积；
/// - `on_repair` 在每个修复轮开始前恰好调用一次（违约轮的 chunk 已全部
///   送达之后）——调用方重置流式显示并切换「重新注解」；
/// - 返回 `Err` 时**已转发的增量不撤回**——以返回值为准丢弃半截正文
///   （状态机据此让 TaskChunk 之后接 TaskFailed 的渲染路径可收敛）；
///   `Err` 只来自引擎、请求构造与流式失控（超 [`MAX_REPLY_BYTES`] 的回复
///   按请求异常失败，不重试），契约违约不产生 `Err`。
// 豁免 too_many_arguments：八个参数各自独立且缺一不可（引擎、模板、kind、
// 底座、模型、语言与两个回调），捆成参数结构只会把 run/评测两侧的调用点
// 改成无谓的装拆包——与 gloss-app 事件装配处的既有豁免同理由。
#[allow(clippy::too_many_arguments)]
pub async fn run_task_rounds(
    engine: &dyn AiEngine,
    prompts: &PromptRegistry,
    kind: TaskKind,
    base: Vec<ChatMessage>,
    model: &str,
    locale: crate::model::Locale,
    mut on_chunk: impl FnMut(String),
    mut on_repair: impl FnMut(),
) -> Result<RunOutput, GlossError> {
    // base 是首轮消息；修复轮以它为底座拼 [assistant 坏回复][user 纠错
    // 指令]（图像部件随 base 零拷贝重发）。
    let mut messages = base.clone();
    let mut repairs = 0u32;
    loop {
        let request = EngineRequest {
            messages,
            model: model.to_owned(),
            max_tokens: None,
        };
        let reply = collect_round(engine, request, &mut on_chunk).await?;

        // 内容审查：服务端拦了内容，重发无益——不重试直接失败卡。
        if reply.finish_reason.as_deref() == Some(FINISH_CONTENT_FILTER) {
            return Err(GlossError::EngineResponse(CONTENT_FILTER_DIAGNOSTIC.into()));
        }

        let violation = if reply.empty_completion {
            Some(Violation::EmptyReply)
        } else {
            schema::validate_task(kind, &reply.body).err()
        };
        let Some(violation) = violation else {
            return Ok(RunOutput {
                kind,
                raw: reply.body,
                finish_reason: reply.finish_reason,
                repairs,
            });
        };
        let Some(next) = next_round(
            RepairStage::Task,
            &base,
            &reply.body,
            violation,
            &mut repairs,
            TASK_REPAIRS,
            |violation| {
                prompts.render_repair(
                    locale,
                    violation,
                    &prompts.task_contract_line(kind, locale),
                    reply.finish_reason.as_deref() == Some(FINISH_LENGTH),
                )
            },
        ) else {
            // 预算耗尽：违约原文原样上交，完成态解析落降级产物。
            return Ok(RunOutput {
                kind,
                raw: reply.body,
                finish_reason: reply.finish_reason,
                repairs,
            });
        };
        messages = next;
        // 修复轮照常流式，开始前发事件重置 UI（清流式正文、切「重新注解」）。
        on_repair();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::{AiEngine, AiTaskService, RunOutput, TASK_REPAIRS};
    use crate::model::{GlossError, Lang};
    use crate::ports::{BoxFuture, EngineRequest, TaskChunk, TaskStream};
    use crate::prompt::{MessageContent, Role};
    use crate::stubs::engine::MockEngine;
    use crate::task::{Task, TaskInput, TaskKind, TaskOptions};

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
    async fn run_task_rounds_executes_a_fixed_kind_stream_without_classification() {
        let engine = Arc::new(MockEngine::new().with_script_queue(vec![
            vec![delta("整段散文，不是 JSON")],
            vec![delta(
                "{\"note\":\"句译完成\",\"structured\":{\"examples\":[]}}",
            )],
        ]));
        let registry = crate::prompt::PromptRegistry::new();
        let base = registry
            .render(
                &Task::new(TaskKind::TranslateSentence, text_input("hello"), options())
                    .expect("the test task is legal"),
            )
            .expect("render should pass the modality check");
        let output = super::run_task_rounds(
            engine.as_ref(),
            &registry,
            TaskKind::TranslateSentence,
            base,
            "m",
            crate::model::Locale::Zh,
            |_| {},
            || {},
        )
        .await
        .expect("the repaired round should converge");
        assert_eq!(
            output.kind,
            TaskKind::TranslateSentence,
            "the kind is the caller's decision, never classified"
        );
        assert_eq!(output.repairs, 1);
        assert_eq!(
            output.raw,
            "{\"note\":\"句译完成\",\"structured\":{\"examples\":[]}}"
        );
        assert_eq!(
            engine.call_count(),
            2,
            "two task rounds: no classify call in front"
        );
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
            vec![Err(GlossError::EmptyCompletion)],
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
    async fn an_oversized_stream_fails_instead_of_accumulating_without_bound() {
        let big = "x".repeat(4096);
        let deltas: Vec<Result<String, GlossError>> = (0..300).map(|_| Ok(big.clone())).collect();
        let (_, service) = make_service(&MockEngine::new().with_chunks(deltas));
        let mut forwarded = 0usize;
        let result = service
            .run(
                image_input(),
                options(),
                |_| {},
                |d| forwarded += d.len(),
                || {},
            )
            .await;
        assert!(
            matches!(result, Err(GlossError::EngineResponse(_))),
            "a runaway stream must fail, not accumulate without bound"
        );
        assert!(
            forwarded <= super::MAX_REPLY_BYTES,
            "forwarded bytes stay within the budget"
        );
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
