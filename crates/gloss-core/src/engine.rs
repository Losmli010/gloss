//! 大语言模型（LLM）任务服务（编排层）：意图定型 → 提示词渲染 → 流式转发 →
//! 契约修复轮。
//!
//! 文本输入先走一次大语言模型（LLM）分类（异常在分类模块内消化，恒有类别），
//! 图像输入固定映射图像解读并跳过分类。引擎错误（网络、
//! 鉴权、限流、格式模式、模态）一律不重试、原样上抛；契约违约走修复
//! 轮常量修复，耗尽后把原始文本交给调用方去降级。
//! “零缓存零配置”：模型取自任务选项，查与写缓存及产物组装归调用方。

pub mod llm;
pub mod sse;

use std::sync::Arc;

use crate::classify::{CLASSIFY_FALLBACK, classify};
use crate::log::{thread, warn};
use crate::model::GlossError;
use crate::ports::{AiEngine, EngineRequest, TaskChunk};
use crate::prompt::{ChatMessage, PromptRegistry};
use crate::repair::{RepairLoop, RepairStage};
use crate::schema::{self, Violation};
use crate::task::{Task, TaskInput, TaskKind, TaskOptions};

pub const TASK_REPAIRS: u32 = 3;

const MAX_REPLY_BYTES: usize = 1024 * 1024;

const FINISH_CONTENT_FILTER: &str = "content_filter";

const CONTENT_FILTER_DIAGNOSTIC: &str = "reply stopped by the server content filter";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutput {
    pub kind: TaskKind,
    pub raw: String,
    pub finish_reason: Option<String>,
    pub repairs: u32,
}

pub struct AiTaskService {
    engine: Arc<dyn AiEngine>,
    prompts: PromptRegistry,
}

impl AiTaskService {
    pub fn new(engine: Arc<dyn AiEngine>) -> Self {
        Self {
            engine,
            prompts: PromptRegistry::new(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn run(
        &self,
        input: TaskInput,
        options: TaskOptions,
        mut on_classified: impl FnMut(TaskKind),
        on_chunk: impl FnMut(String),
        on_repair: impl FnMut(),
    ) -> Result<RunOutput, GlossError> {
        if options.model.trim().is_empty() {
            return Err(GlossError::Config("empty model id".into()));
        }

        let kind = match &input {
            TaskInput::Text { .. } => match classify(
                self.engine.as_ref(),
                &options.model,
                options.prompt_locale.unwrap_or_default(),
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

        let locale = options.prompt_locale.unwrap_or_default();
        let task = Task::new(kind, input, options)?;
        let base = self.prompts.render(&task)?;
        run_task_rounds(
            TaskStage {
                engine: self.engine.as_ref(),
                prompts: &self.prompts,
                kind,
                base,
                model: &task.options().model,
                locale,
            },
            on_chunk,
            on_repair,
        )
        .await
    }
}

struct RoundReply {
    body: String,
    finish_reason: Option<String>,
    empty_completion: bool,
}

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

pub struct TaskStage<'a> {
    pub engine: &'a dyn AiEngine,
    pub prompts: &'a PromptRegistry,
    pub kind: TaskKind,
    pub base: Vec<ChatMessage>,
    pub model: &'a str,
    pub locale: crate::model::Locale,
}

pub async fn run_task_rounds(
    TaskStage {
        engine,
        prompts,
        kind,
        base,
        model,
        locale,
    }: TaskStage<'_>,
    mut on_chunk: impl FnMut(String),
    mut on_repair: impl FnMut(),
) -> Result<RunOutput, GlossError> {
    let mut messages = base.clone();
    let mut repairs = RepairLoop::new(RepairStage::Task, TASK_REPAIRS);
    loop {
        let request = EngineRequest {
            messages,
            model: model.to_owned(),
            max_tokens: None,
        };
        let reply = collect_round(engine, request, &mut on_chunk).await?;

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
                repairs: repairs.spent(),
            });
        };
        let Some(next) = repairs.next_round(&base, &reply.body, violation, |violation| {
            prompts.render_repair(locale, violation)
        }) else {
            return Ok(RunOutput {
                kind,
                raw: reply.body,
                finish_reason: reply.finish_reason,
                repairs: repairs.spent(),
            });
        };
        messages = next;
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

    fn count_with_prefix(events: &[String], prefix: &str) -> usize {
        events
            .iter()
            .filter(|event| event.starts_with(prefix))
            .count()
    }

    fn assert_repair_instruction_wording(instruction: &str) {
        assert!(
            instruction.contains("reply is not a bare JSON object"),
            "the instruction carries the fixed violation wording as the reason: {instruction}"
        );
        assert!(
            !instruction.contains("{\"note\""),
            "the repair template no longer quotes a contract schema line: {instruction}"
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
            super::TaskStage {
                engine: engine.as_ref(),
                prompts: &registry,
                kind: TaskKind::TranslateSentence,
                base,
                model: "m",
                locale: crate::model::Locale::Zh,
            },
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
            count_with_prefix(&events, "classified:"),
            1,
            "the fixed kind is reported exactly once: {events:?}"
        );
        assert_eq!(
            events.first().map(String::as_str),
            Some("classified:ImageExplain"),
            "the fixed kind precedes every chunk: {events:?}"
        );
        assert_eq!(count_with_prefix(&events, "chunk:"), 2);
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
        assert_repair_instruction_wording(&instruction);
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
    async fn a_length_finish_repairs_without_a_truncation_hint() {
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
            .expect("the truncated reply should repair like any other violation");
        assert_eq!(output.repairs, 1);
        assert_eq!(output.finish_reason.as_deref(), Some("stop"));
        let requests = engine.recorded_requests();
        assert_eq!(requests.len(), 2);
        let instruction = message_texts(&requests[1])[3].1.clone();
        assert!(
            !instruction.contains("输出被截断"),
            "the repair template has no truncation hint anymore: {instruction}"
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
