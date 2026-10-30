//! 自动任务分类（编排前半程）：文本输入 → 具体任务类型。
//!
//! 判定完全交给大语言模型（LLM），只给极小的提示词加分类截断预算，
//! 产出再走格式校验（JSON）：类别取分类模板四个类别之一，
//! 兜底类别映射到通用任务——源语言与代码语言同样由模型从原文自行判断，
//! 取材不再携带模态提示。校验用核心层的分类校验器，要求严格裸格式（JSON），
//! 与生产及评测共用同一校验器；流式循环内每次累积后即尝试解析，
//! 首个能通过校验的完整对象立即定型返回——不等流自然结束，提前还流即
//! 还回首字延迟（TTFT）。
//!
//! 异常内部消化：契约违约（空回复、非法对象、类别越模板清单）走修复轮
//! （对话形态为系统设定、用户原文、上一轮坏回复、纠错指令四段，
//! 空回复原样重发不带坏回复轮），预算见分类修复轮数常量；
//! 端点以零增量成功收尾与空回复同口径，
//! 转空回复进修复轮（与任务段逐轮收集对称：坏回复而非坏
//! 请求，值得一次重发）；传输停摆（建连后首条目时限内无任何
//! 流条目）与其余流错误按请求异常收卷且不重试。两者耗尽或失败都落常量兜底
//! 类别——本函数对文本输入恒返回一个确定类别，用户无感，
//! 留警告痕（类别与序号，不含内容）。模型判定的原始类别与最终执行的
//! 类别在定型时同行落信息级日志。
//!
//! 内容红线：分类输出契约只有一个类别标识，没有理由字段；本模块的
//! 错误与日志都不携带输入内容与模型回复原文。

use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use futures_core::Stream;
use tokio::time::{Instant as TokioInstant, Sleep, sleep_until};

use crate::log::{info, thread, warn};
use crate::model::{GlossError, Locale};
use crate::ports::{AiEngine, EngineRequest, TaskChunk, TaskStream};
use crate::prompt::PromptRegistry;
use crate::repair::{RepairLoop, RepairStage};
use crate::schema;
use crate::task::{TaskInput, TaskKind};

pub const CLASSIFY_FALLBACK: TaskKind = TaskKind::General;

pub const CLASSIFY_REPAIRS: u32 = 1;

pub const CLASSIFY_MAX_TOKENS: u32 = 128;

pub const CLASSIFY_REPLY_CAP_BYTES: usize = 4096;

pub fn push_capped(reply: &mut String, delta: &str) {
    let budget = CLASSIFY_REPLY_CAP_BYTES.saturating_sub(reply.len());
    if budget == 0 {
        return;
    }
    let mut take = budget.min(delta.len());
    while take > 0 && !delta.is_char_boundary(take) {
        take -= 1;
    }
    reply.push_str(&delta[..take]);
}

const FIRST_ITEM_DEADLINE: Duration = Duration::from_secs(3);

struct FirstItemDeadline {
    inner: TaskStream,
    deadline: Option<Pin<Box<Sleep>>>,
    armed: bool,
}

impl FirstItemDeadline {
    fn new(inner: TaskStream) -> Self {
        Self {
            inner,
            deadline: None,
            armed: true,
        }
    }
}

impl Stream for FirstItemDeadline {
    type Item = Result<TaskChunk, GlossError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match this.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(item)) => {
                this.armed = false;
                return Poll::Ready(Some(item));
            }
            Poll::Ready(None) => return Poll::Ready(None),
            Poll::Pending => {}
        }
        if !this.armed {
            return Poll::Pending;
        }
        let deadline = this.deadline.get_or_insert_with(|| {
            Box::pin(sleep_until(TokioInstant::now() + FIRST_ITEM_DEADLINE))
        });
        if deadline.as_mut().poll(cx).is_ready() {
            warn!(
                thread = thread::TOKIO,
                deadline_secs = FIRST_ITEM_DEADLINE.as_secs(),
                "classify stream silent past the first-item deadline"
            );
            return Poll::Ready(Some(Err(GlossError::EngineNetwork)));
        }
        Poll::Pending
    }
}

pub async fn classify(
    engine: &dyn AiEngine,
    model: &str,
    locale: Locale,
    input: &TaskInput,
) -> Result<TaskKind, GlossError> {
    let TaskInput::Text { text } = input else {
        return Err(GlossError::UnsupportedModality);
    };
    match classify_traced(engine, model, locale, text).await {
        Ok(decision) => Ok(decision.kind),
        Err(error) => {
            warn!(
                thread = thread::TOKIO,
                error = %error,
                "classify request failed, falling back to the default kind"
            );
            Ok(CLASSIFY_FALLBACK)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifyDecision {
    pub kind: TaskKind,
    pub repairs: u32,
    pub rounds: Vec<String>,
}

pub async fn classify_traced(
    engine: &dyn AiEngine,
    model: &str,
    locale: Locale,
    text: &str,
) -> Result<ClassifyDecision, GlossError> {
    let registry = PromptRegistry::new();
    let base = registry.render_classify(locale, text);
    let mut messages = base.clone();
    let mut repairs = RepairLoop::new(RepairStage::Classify, CLASSIFY_REPAIRS);
    let mut rounds = Vec::new();
    loop {
        let request = EngineRequest {
            messages,
            model: model.to_owned(),
            max_tokens: Some(CLASSIFY_MAX_TOKENS),
        };
        let round = classify_round(engine, request).await?;
        let verdict = match round.settled {
            Some(choice) => Ok(choice),
            None => schema::validate_classify(&round.reply),
        };
        match verdict {
            Ok(choice) => {
                rounds.push(round.reply);
                return Ok(settled_decision(choice, repairs.spent(), rounds));
            }
            Err(violation) => {
                let resend = (!matches!(violation, crate::schema::Violation::EmptyReply))
                    .then_some(round.reply.as_str());
                let Some(next) = repairs.next_round(&base, resend, violation, || {
                    registry.render_repair(locale, violation, None)
                }) else {
                    rounds.push(round.reply);
                    return Ok(ClassifyDecision {
                        kind: CLASSIFY_FALLBACK,
                        repairs: repairs.spent(),
                        rounds,
                    });
                };
                rounds.push(round.reply);
                messages = next;
            }
        }
    }
}

fn settled_decision(
    choice: schema::ClassifyChoice,
    repairs: u32,
    rounds: Vec<String>,
) -> ClassifyDecision {
    info!(
        thread = thread::TOKIO,
        model_kind = %choice.model_kind,
        kind = ?choice.kind,
        repairs = repairs,
        "model classification settled"
    );
    ClassifyDecision {
        kind: choice.kind,
        repairs,
        rounds,
    }
}

struct ClassifyRound {
    reply: String,
    settled: Option<schema::ClassifyChoice>,
}

async fn classify_round(
    engine: &dyn AiEngine,
    request: EngineRequest,
) -> Result<ClassifyRound, GlossError> {
    let stream = engine.execute(request).await?;
    let mut stream = std::pin::pin!(FirstItemDeadline::new(stream));
    let mut reply = String::new();
    let mut settled = None;
    while let Some(item) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
        match item {
            Ok(TaskChunk::Delta(delta)) => {
                push_capped(&mut reply, &delta);
                if let Ok(choice) = schema::validate_classify(&reply) {
                    settled = Some(choice);
                    break;
                }
            }
            Ok(TaskChunk::Done { .. }) => {}
            Err(GlossError::EmptyCompletion) if reply.is_empty() => break,
            Err(error) => return Err(error),
        }
    }
    Ok(ClassifyRound { reply, settled })
}

pub fn parse_classify_reply(reply: &str) -> Result<TaskKind, GlossError> {
    schema::validate_classify(reply)
        .map(|choice| choice.kind)
        .map_err(|_| GlossError::EngineResponse("unrecognized classify reply".into()))
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::pin::Pin;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Poll};
    use std::time::Duration;

    use futures_core::Stream;
    use tokio::time::{Instant as TokioInstant, Sleep, sleep_until};

    use super::{
        CLASSIFY_FALLBACK, CLASSIFY_REPAIRS, classify, classify_traced, parse_classify_reply,
    };
    use crate::model::{GlossError, Locale};
    use crate::ports::{AiEngine, BoxFuture, EngineRequest, TaskChunk, TaskStream};
    use crate::stubs::engine::MockEngine;
    use crate::task::{TaskInput, TaskKind};

    struct SilentStream;

    impl Stream for SilentStream {
        type Item = Result<TaskChunk, GlossError>;

        fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Poll::Pending
        }
    }

    struct TimedStream {
        steps: std::vec::IntoIter<(Duration, Result<TaskChunk, GlossError>)>,
        next: Pin<Box<Sleep>>,
    }

    impl TimedStream {
        fn new(steps: Vec<(Duration, Result<TaskChunk, GlossError>)>) -> Self {
            let first = steps.first().map_or(Duration::ZERO, |(delay, _)| *delay);
            Self {
                steps: steps.into_iter(),
                next: Box::pin(sleep_until(TokioInstant::now() + first)),
            }
        }
    }

    impl Stream for TimedStream {
        type Item = Result<TaskChunk, GlossError>;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            if self.next.as_mut().poll(cx).is_ready() {
                return match self.steps.next() {
                    Some((delay, item)) => {
                        self.next.as_mut().reset(TokioInstant::now() + delay);
                        Poll::Ready(Some(item))
                    }
                    None => Poll::Ready(None),
                };
            }
            Poll::Pending
        }
    }

    struct StreamEngine {
        streams: Mutex<VecDeque<TaskStream>>,
        calls: AtomicUsize,
    }

    impl StreamEngine {
        fn new(streams: Vec<TaskStream>) -> Self {
            Self {
                streams: Mutex::new(VecDeque::from(streams)),
                calls: AtomicUsize::new(0),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl AiEngine for StreamEngine {
        fn execute(
            &self,
            _request: EngineRequest,
        ) -> BoxFuture<'static, Result<TaskStream, GlossError>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let stream = self
                .streams
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .pop_front()
                .expect("stream script queued");
            Box::pin(async move { Ok(stream) })
        }
    }

    fn text_input(text: &str) -> TaskInput {
        TaskInput::Text { text: text.into() }
    }

    #[tokio::test]
    async fn non_text_input_is_rejected() {
        let engine = MockEngine::new();
        assert!(matches!(
            classify(
                &engine,
                "m",
                Locale::Zh,
                &TaskInput::Image {
                    png: std::sync::Arc::from(&b"png"[..]),
                },
            )
            .await,
            Err(GlossError::UnsupportedModality)
        ));
    }

    #[tokio::test]
    async fn bare_json_reply_selects_the_kind() {
        let engine = MockEngine::new().with_chunks(vec![Ok("{\"kind\":\"TranslateWord\"}".into())]);
        let kind = classify(&engine, "m", Locale::Zh, &text_input("gloss"))
            .await
            .expect("bare JSON must parse");
        assert_eq!(kind, TaskKind::TranslateWord);
    }

    #[tokio::test]
    async fn a_fenced_reply_repairs_then_falls_back() {
        let engine = MockEngine::new()
            .with_chunks(vec![Ok("```json\n{\"kind\":\"ExplainCode\"}\n```".into())]);
        let kind = classify(&engine, "m", Locale::En, &text_input("select * from t"))
            .await
            .expect("a violated reply never fails the classification");
        assert_eq!(
            kind,
            TaskKind::General,
            "a fenced reply violates the bare-JSON contract: repair, then the fallback"
        );
        assert_eq!(
            engine.call_count(),
            1 + CLASSIFY_REPAIRS as usize,
            "one first attempt plus one repair round"
        );
    }

    #[tokio::test]
    async fn a_complete_json_settles_before_the_stream_ends() {
        let engine = MockEngine::new().with_chunks(vec![
            Ok("{\"kind\":\"Transl".into()),
            Ok("ateWord\"}".into()),
            Err(GlossError::EngineRateLimited),
        ]);
        let kind = classify(&engine, "m", Locale::Zh, &text_input("gloss"))
            .await
            .expect("the completed JSON must settle before the trailing failure");
        assert_eq!(kind, TaskKind::TranslateWord);
    }

    #[tokio::test]
    async fn the_first_complete_json_wins_over_later_deltas() {
        let engine = MockEngine::new().with_chunks(vec![
            Ok("{\"kind\":\"TranslateWord\"}".into()),
            Ok("{\"kind\":\"ExplainCode\"}".into()),
        ]);
        let kind = classify(&engine, "m", Locale::Zh, &text_input("gloss"))
            .await
            .expect("the first complete JSON must win");
        assert_eq!(kind, TaskKind::TranslateWord);
    }

    #[tokio::test]
    async fn partial_json_keeps_waiting_for_the_stream() {
        let engine = MockEngine::new().with_chunks(vec![
            Ok("{\"kind\":\"Transl".into()),
            Ok("ateWord\"".into()),
        ]);
        assert_eq!(
            classify(&engine, "m", Locale::Zh, &text_input("gloss"),)
                .await
                .expect("a truncated reply must not settle mid-stream"),
            TaskKind::General,
            "after the stream ends the truncated reply repairs into the fallback"
        );
    }

    #[tokio::test]
    async fn a_request_failure_is_digested_into_the_fallback_without_retry() {
        let engine = MockEngine::new().with_execute_failure(GlossError::EngineRateLimited);
        assert_eq!(
            classify(&engine, "m", Locale::Zh, &text_input("gloss"),)
                .await
                .expect("a request failure is digested, never raised"),
            TaskKind::General,
        );
        assert_eq!(
            engine.call_count(),
            1,
            "engine errors are never retried in the classify stage"
        );
    }

    #[tokio::test]
    async fn a_violated_reply_repairs_into_a_kind() {
        let engine = MockEngine::new().with_script_queue(vec![
            vec![Ok(TaskChunk::Delta("MARKER-prose-reply 不是 JSON".into()))],
            vec![Ok(TaskChunk::Delta("{\"kind\":\"ExplainCode\"}".into()))],
        ]);
        let kind = classify(&engine, "m", Locale::Zh, &text_input("kubectl get pods"))
            .await
            .expect("the repaired reply should settle");
        assert_eq!(kind, TaskKind::ExplainCode);
        assert_eq!(engine.call_count(), 2);
        let requests = engine.recorded_requests();
        let roles: Vec<_> = requests[1].messages.iter().map(|m| m.role).collect();
        assert_eq!(
            roles,
            [
                crate::prompt::Role::System,
                crate::prompt::Role::User,
                crate::prompt::Role::Assistant,
                crate::prompt::Role::User
            ],
            "classify repair: [system][user 原文][assistant 坏回复][user 纠错指令]"
        );
        let instruction = match &requests[1].messages[3].content {
            crate::prompt::MessageContent::Text(text) => text.clone(),
            other => panic!("expected text content, got {other:?}"),
        };
        assert!(
            instruction.contains("回复不是裸（JSON）对象：NotJson——"),
            "the instruction rides the violation-level registry row (the classify stage \
             speaks in violation-level rows): {instruction}"
        );
        assert!(
            !instruction.contains("{\"kind\""),
            "the repair template no longer carries a contract schema line: {instruction}"
        );
        assert!(
            !instruction.contains("MARKER-prose-reply"),
            "the instruction must not quote the bad reply: {instruction}"
        );
        assert!(
            !instruction.contains("kubectl get pods"),
            "the instruction must not carry the selection text: {instruction}"
        );
    }

    #[tokio::test]
    async fn classify_traced_carries_rounds_and_repairs_for_the_trajectory() {
        let engine = MockEngine::new().with_script_queue(vec![
            vec![Ok(TaskChunk::Delta("MARKER-prose-reply 不是 JSON".into()))],
            vec![Ok(TaskChunk::Delta("{\"kind\":\"ExplainCode\"}".into()))],
        ]);
        let decision = classify_traced(&engine, "m", Locale::Zh, "kubectl get pods")
            .await
            .expect("the repaired reply should settle");
        assert_eq!(decision.kind, TaskKind::ExplainCode);
        assert_eq!(decision.repairs, 1);
        assert_eq!(
            decision.rounds,
            vec![
                "MARKER-prose-reply 不是 JSON".to_owned(),
                "{\"kind\":\"ExplainCode\"}".to_owned(),
            ],
            "every round's full reply rides the decision in order"
        );

        let engine = MockEngine::new().with_script_queue(vec![
            vec![Ok(TaskChunk::Delta("MARKER-还是散文".into()))],
            vec![Ok(TaskChunk::Delta("MARKER-依旧散文".into()))],
        ]);
        let decision = classify_traced(&engine, "m", Locale::Zh, "一段文本")
            .await
            .expect("exhausted repairs never fail the classification");
        assert_eq!(decision.kind, TaskKind::General);
        assert_eq!(decision.repairs, CLASSIFY_REPAIRS);
        assert_eq!(decision.rounds.len(), 1 + CLASSIFY_REPAIRS as usize);
    }

    #[tokio::test]
    async fn an_empty_reply_repairs_by_resending_as_is() {
        let engine = MockEngine::new().with_script_queue(vec![
            Vec::new(),
            vec![Ok(TaskChunk::Delta(
                "{\"kind\":\"TranslateSentence\"}".into(),
            ))],
        ]);
        let kind = classify(&engine, "m", Locale::Zh, &text_input("bonjour"))
            .await
            .expect("the resent request should settle");
        assert_eq!(kind, TaskKind::TranslateSentence);
        let requests = engine.recorded_requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[1].messages, requests[0].messages,
            "EmptyReply has nothing to repair: the original request is resent verbatim"
        );
    }

    #[tokio::test]
    async fn an_empty_completion_enters_the_repair_round_and_resends() {
        let engine = MockEngine::new().with_script_queue(vec![
            vec![Err(GlossError::EmptyCompletion)],
            vec![Ok(TaskChunk::Delta("{\"kind\":\"TranslateWord\"}".into()))],
        ]);
        let kind = classify(&engine, "m", Locale::Zh, &text_input("gloss"))
            .await
            .expect("the resent request should settle");
        assert_eq!(
            kind,
            TaskKind::TranslateWord,
            "a zero-increment empty completion is a bad reply, not a request failure"
        );
        let requests = engine.recorded_requests();
        assert_eq!(requests.len(), 2, "one empty round plus one repair round");
        assert_eq!(
            requests[1].messages, requests[0].messages,
            "the empty completion resends the original request verbatim"
        );
    }

    #[tokio::test]
    async fn a_repeated_empty_completion_exhausts_into_the_fallback() {
        let engine = MockEngine::new().with_script_queue(vec![
            vec![Err(GlossError::EmptyCompletion)],
            vec![Err(GlossError::EmptyCompletion)],
        ]);
        assert_eq!(
            classify(&engine, "m", Locale::Zh, &text_input("gloss"))
                .await
                .expect("exhausted repairs never fail the classification"),
            TaskKind::General,
            "the empty completion burns the repair budget like any EmptyReply"
        );
        assert_eq!(engine.call_count(), 1 + CLASSIFY_REPAIRS as usize);
    }

    #[test]
    fn exhausted_repairs_fall_back_to_general_with_a_trace() {
        let _serial = crate::log::test_support::lock_dispatchers();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("runtime should build");
        let line = (0..3)
            .find_map(|_| {
                let engine = MockEngine::new().with_chunks(vec![Ok("MARKER-还是散文".into())]);
                let logs = crate::log::capture(|| {
                    let kind = rt
                        .block_on(classify(
                            &engine,
                            "m",
                            Locale::Zh,
                            &text_input("一段普通选区"),
                        ))
                        .expect("exhausted repairs never fail the classification");
                    assert_eq!(kind, TaskKind::General);
                });
                assert_eq!(
                    engine.call_count(),
                    1 + CLASSIFY_REPAIRS as usize,
                    "one first attempt plus one repair round"
                );
                logs.lines()
                    .find(|line| line.contains("classify repairs exhausted"))
                    .inspect(|line| {
                        assert!(
                            !line.contains("MARKER") && !line.contains("一段普通选区"),
                            "the fallback warn must not carry reply or selection content: {line}"
                        );
                        assert!(
                            line.contains("reply is not a bare JSON object"),
                            "the warn carries the fixed category: {line}"
                        );
                    })
                    .map(str::to_owned)
            })
            .is_some();
        assert!(line, "the exhausted fallback must leave a trace");
    }

    #[test]
    fn replies_outside_the_template_categories_are_rejected() {
        assert!(parse_classify_reply("{\"kind\":\"ImageExplain\"}").is_err());
        assert!(
            parse_classify_reply("{\"kind\":\"General\"}").is_err(),
            "the general kind is the execution landing, never a model category"
        );
        assert!(parse_classify_reply("{\"kind\":\"Nonsense\"}").is_err());
        assert!(parse_classify_reply("{\"kind\":null}").is_err());
        assert!(parse_classify_reply("我觉得这是一段翻译").is_err());
        assert!(parse_classify_reply("").is_err());
        assert!(
            parse_classify_reply("前置说明\n```gloss\n{\"kind\":\"TranslateWord\"}\n```").is_err(),
            "the fence tolerance is gone: a gloss-fenced reply violates the \
             bare-JSON contract even with a valid kind inside"
        );
    }

    #[test]
    fn a_fallback_choice_lands_on_the_general_kind() {
        assert_eq!(
            parse_classify_reply("{\"kind\":\"Fallback\"}"),
            Ok(TaskKind::General),
            "the template's fallback category executes the general task"
        );
    }

    #[test]
    fn the_fallback_kind_is_the_general_template_landing() {
        assert_eq!(
            CLASSIFY_FALLBACK,
            TaskKind::General,
            "the fallback lands on the general kind"
        );
    }

    #[test]
    fn a_fallback_reply_lands_on_the_general_task_with_both_kinds_logged() {
        let _serial = crate::log::test_support::lock_dispatchers();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("runtime should build");
        let line = (0..3)
            .find_map(|_| {
                let engine =
                    MockEngine::new().with_chunks(vec![Ok("{\"kind\":\"Fallback\"}".into())]);
                let logs = crate::log::capture(|| {
                    let kind = rt
                        .block_on(classify(
                            &engine,
                            "m",
                            Locale::Zh,
                            &text_input("MARKER-闲聊文本"),
                        ))
                        .expect("the fallback category must settle");
                    assert_eq!(kind, TaskKind::General);
                });
                logs.lines()
                    .find(|line| line.contains("model classification settled"))
                    .inspect(|line| {
                        assert!(
                            line.contains("\"model_kind\":\"Fallback\"")
                                && line.contains("\"kind\":\"General\""),
                            "the settled log carries the model category and the final kind: {line}"
                        );
                        assert!(
                            !line.contains("MARKER"),
                            "the settled log must not carry the selection content: {line}"
                        );
                    })
                    .map(str::to_owned)
            })
            .is_some();
        assert!(line, "the settled classification must leave a trace");
    }

    #[tokio::test]
    async fn a_silent_stream_is_cut_at_the_first_item_deadline() {
        tokio::time::pause();
        let engine = StreamEngine::new(vec![Box::pin(SilentStream) as TaskStream]);
        let kind = classify(&engine, "m", Locale::Zh, &text_input("gloss"))
            .await
            .expect("the deadline cut is digested into the fallback");
        assert_eq!(kind, CLASSIFY_FALLBACK);
        assert_eq!(
            engine.call_count(),
            1,
            "a deadline cut is a request failure: no retry, one attempt"
        );
    }

    #[tokio::test]
    async fn items_after_the_first_flow_without_the_deadline() {
        tokio::time::pause();
        let engine = StreamEngine::new(vec![Box::pin(TimedStream::new(vec![
            (
                Duration::from_secs(2),
                Ok(TaskChunk::Delta("{\"kind\":\"Translate".into())),
            ),
            (
                Duration::from_secs(1_000),
                Ok(TaskChunk::Delta("Word\"}".into())),
            ),
        ])) as TaskStream]);
        let kind = classify(&engine, "m", Locale::Zh, &text_input("gloss"))
            .await
            .expect("a late second item must still land: the deadline covers the first item only");
        assert_eq!(kind, TaskKind::TranslateWord);
        assert_eq!(engine.call_count(), 1);
    }
}
