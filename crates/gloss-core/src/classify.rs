//! 自动任务分类（编排前半程）：文本输入 → 具体任务类型。
//!
//! 判定完全交给 LLM（极小 prompt + [`CLASSIFY_MAX_TOKENS`] 截断）→ JSON
//! 校验（kind 必须在允许清单内）——源语言与代码语言同样由模型从原文
//! 自行判断，取材不再携带模态提示。校验用 [`crate::schema::validate_classify`]
//! （严格裸 JSON，与生产/评测共用同一校验器）；流式循环内每次累积后即
//! 尝试解析，首个能通过校验的完整 JSON 立即定型返回（不等流自然结束，
//! 提前还流即还回 TTFT）。
//!
//! **异常内部消化**：契约违约（空回复 / 非 JSON / kind 越清单）走修复轮
//! （对话形态 `[system][user 原文][assistant 上一次坏回复][user 纠错指令]`，
//! EmptyReply 原样重发不带 assistant 轮），预算 [`CLASSIFY_REPAIRS`]；
//! 分类阶段的请求异常不重试。两者耗尽/失败都落常量兜底
//! [`CLASSIFY_FALLBACK`]——本函数对文本输入恒返回 `Ok(kind)`，用户无感，
//! warn 留痕（类别 + 序号，不含内容）。
//!
//! 内容红线：分类输出契约只有一个 kind 标识，没有理由字段；本模块的
//! 错误与日志都不携带输入内容与模型回复原文。

use crate::log::{debug, thread, warn};
use crate::model::{GlossError, Locale};
use crate::ports::{AiEngine, EngineRequest, TaskChunk};
use crate::prompt::{CLASSIFY_SCHEMA, PromptRegistry, repair_conversation};
use crate::schema::{self, Violation};
use crate::task::{TaskInput, TaskKind};

/// 分类的允许清单：分类只服务划词路径，清单 3 个就是**文本取材可执行**
/// 的 kind（图像/语音 kind 不在其中——划词到不了它们）。文本路径的完整
/// 落点是「清单 3 个 + 兜底 [`CLASSIFY_FALLBACK`]」：模型只在本清单内
/// 选择，清单外一律违约进修复轮，失效时落兜底。
pub const CLASSIFY_KINDS: [TaskKind; 3] = [
    TaskKind::TranslateWord,
    TaskKind::TranslateSentence,
    TaskKind::ExplainCode,
];

/// 分类失败（请求异常、修复轮后仍违约）时的兜底 kind：常量而非配置——
/// 兜底必须是可渲染、可执行的具体文本 kind。「清单 3 个 + 兜底 General」
/// 构成文本路径的完整落点：分类器只在 [`CLASSIFY_KINDS`] 内选择，清单外
/// 的 General 只是失效时的执行落点。
pub const CLASSIFY_FALLBACK: TaskKind = TaskKind::General;

/// 分类契约违约的修复轮预算：分类在 TTFT 之前，预算敏感——共
/// `1 + CLASSIFY_REPAIRS` 次尝试。
pub const CLASSIFY_REPAIRS: u32 = 1;

/// 分类回复的 token 上限（OpenAI 兼容 `max_tokens`）：正确回复是一个
/// 只含 kind 标识的小 JSON，截断只落在模型跑偏的长篇上——跑偏的回复
/// 解析不过，进修复轮。
pub const CLASSIFY_MAX_TOKENS: u32 = 128;

/// 回复累积上限（字节）：`max_tokens` 之外的第二道界，防止不守约的
/// 端点把无界回复灌进内存。超限部分丢弃（按字符边界截断）——截断的
/// JSON 解析不过，同样进修复轮。pub 供 gloss-eval 的 live 轨复用同一
/// 道界（评测与生产同源）。
pub const CLASSIFY_REPLY_CAP_BYTES: usize = 4096;

/// 按预算累积回复：单条增量超预算时按字符边界截断。分类编排与评测
/// live 轨共用。
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

/// 判定一条文本输入的任务类型。`allowed` 是允许模型选择的清单（编排
/// 传 [`CLASSIFY_KINDS`]），识别结果超出清单按未识别处理。契约违约走
/// 修复轮（预算 [`CLASSIFY_REPAIRS`]），请求异常不重试；两者最终都落
/// [`CLASSIFY_FALLBACK`]——文本输入恒 `Ok`。
pub async fn classify(
    engine: &dyn AiEngine,
    model: &str,
    locale: Locale,
    allowed: &[TaskKind],
    input: &TaskInput,
) -> Result<TaskKind, GlossError> {
    let TaskInput::Text { text } = input else {
        // 分类只服务划词路径；图像/音频任务带着具体 kind 进编排，
        // 到不了这里。
        return Err(GlossError::UnsupportedModality);
    };
    match classify_traced(engine, model, locale, allowed, text).await {
        Ok(decision) => Ok(decision.kind),
        // 分类阶段的请求异常不重试（重发同请求无益），内部消化落兜底
        // ——warn 只记错误类别，不含选区原文与模型回复。
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

/// 一次分类的完整轨迹：判定的任务类型、契约修复轮次数与每一轮的完整
/// 回复（末轮是完成态判定的输入）。生产编排只消费 `kind`；评测
/// （gloss-eval live 轨）据 `rounds`/`repairs` 把违约类别与修复轮次计入
/// 统计——两侧消费同一份轨迹，评测数字度量的才是生产行为。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifyDecision {
    /// 判定的任务类型（含兜底 [`CLASSIFY_FALLBACK`]）。
    pub kind: TaskKind,
    /// 契约违约触发的修复轮次数（0 = 一次通过）。
    pub repairs: u32,
    /// 每一轮的完整回复，按轮序（修复轮由违约触发，逐轮跑
    /// [`crate::schema::validate_classify`] 即还原违约轨迹）。
    pub rounds: Vec<String>,
}

/// [`classify`] 的全轨迹形态：逐轮执行分类请求并记录回复，契约违约走
/// 修复轮（对话形态 `[system][user 原文][assistant 上一次坏回复]
/// [user 纠错指令]`，EmptyReply 原样重发不带 assistant 轮），预算
/// [`CLASSIFY_REPAIRS`] 耗尽落 [`CLASSIFY_FALLBACK`]（恒 `Ok`）。与
/// [`classify`] 的差别只有一处：**请求异常不消化**、原样上抛——消化
/// （落兜底 + warn）归 [`classify`]；评测需要区分「请求失败」与「回复
/// 违约」，故在 traced 层保持上抛。
///
/// `text` 是待分类的选区文本；`allowed` 是允许模型选择的清单（编排传
/// [`CLASSIFY_KINDS`]）。pub 供 gloss-eval 的 live 轨复用同一循环（评测
/// 与生产同源）。
pub async fn classify_traced(
    engine: &dyn AiEngine,
    model: &str,
    locale: Locale,
    allowed: &[TaskKind],
    text: &str,
) -> Result<ClassifyDecision, GlossError> {
    let registry = PromptRegistry::new();
    let base = registry.render_classify(locale, allowed, text);
    let mut messages = base.clone();
    let mut repairs = 0u32;
    let mut rounds = Vec::new();
    loop {
        let request = EngineRequest {
            messages: messages.clone(),
            model: model.to_owned(),
            max_tokens: Some(CLASSIFY_MAX_TOKENS),
        };
        let reply = classify_round(engine, &request, allowed).await?;
        rounds.push(reply.clone());
        match schema::validate_classify(&reply, allowed) {
            Ok(kind) => {
                debug!(kind = ?kind, "classified by the model");
                return Ok(ClassifyDecision {
                    kind,
                    repairs,
                    rounds,
                });
            }
            Err(violation) => {
                if repairs >= CLASSIFY_REPAIRS {
                    warn!(
                        thread = thread::TOKIO,
                        reason = %violation,
                        "classify repairs exhausted, falling back to the default kind"
                    );
                    return Ok(ClassifyDecision {
                        kind: CLASSIFY_FALLBACK,
                        repairs,
                        rounds,
                    });
                }
                repairs += 1;
                // 违约 warn 只记类别 + 轮次（第几轮违约），无内容。
                warn!(
                    thread = thread::TOKIO,
                    attempt = repairs,
                    reason = %violation,
                    "classify reply violated the contract, preparing a repair round"
                );
                let instruction = registry.render_repair(locale, violation, CLASSIFY_SCHEMA, false);
                // EmptyReply 无内容可纠：原样重发，不带 assistant 轮。
                let bad = match violation {
                    Violation::EmptyReply => None,
                    _ => Some(reply.as_str()),
                };
                messages = repair_conversation(&base, bad, instruction);
            }
        }
    }
}

/// 执行一轮分类请求并按预算累积回复：流式增量逐条累积，首个能通过校验
/// 的完整 JSON 立即提前收卷（不等流自然结束，提前还流即还回 TTFT）；
/// 只在 Ok 时提前退出——部分 JSON 继续等，跑偏与截断的回复照旧走完流、
/// 落到整段校验。每次累积后的全量校验上界是回复上限（4KB）× 个位数
/// chunk，相对 chunk 的毫秒级到达间隔可忽略。结束帧对分类没有增量价值。
async fn classify_round(
    engine: &dyn AiEngine,
    request: &EngineRequest,
    allowed: &[TaskKind],
) -> Result<String, GlossError> {
    let mut stream = engine.execute(request.clone()).await?;
    let mut reply = String::new();
    while let Some(item) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
        match item {
            Ok(TaskChunk::Delta(delta)) => {
                push_capped(&mut reply, &delta);
                if let Ok(kind) = schema::validate_classify(&reply, allowed) {
                    debug!(kind = ?kind, "classified by the model");
                    return Ok(reply);
                }
            }
            Ok(TaskChunk::Done { .. }) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(reply)
}

/// 解析模型回复：严格裸 JSON，无围栏容错——围栏输出即违约（与生产
/// 编排的 [`crate::schema::validate_classify`] 同一条判线，本函数只把
/// [`Violation`] 折算成 [`GlossError::EngineResponse`]，错误文本是固定
/// 措辞，不携带回复原文）。
///
/// pub 供 gloss-eval 评测重放复用：评测与生产走同一个校验器。
pub fn parse_classify_reply(reply: &str, allowed: &[TaskKind]) -> Result<TaskKind, GlossError> {
    schema::validate_classify(reply, allowed)
        .map_err(|_| GlossError::EngineResponse("unrecognized classify reply".into()))
}

#[cfg(test)]
mod tests {
    use super::{
        CLASSIFY_FALLBACK, CLASSIFY_KINDS, CLASSIFY_REPAIRS, classify, classify_traced,
        parse_classify_reply,
    };
    use crate::model::{GlossError, Locale};
    use crate::ports::TaskChunk;
    use crate::stubs::engine::MockEngine;
    use crate::task::{TaskInput, TaskKind};

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
                &CLASSIFY_KINDS,
                &TaskInput::Audio {
                    bytes: std::sync::Arc::from(&b"au"[..]),
                    duration_hint: None,
                },
            )
            .await,
            Err(GlossError::UnsupportedModality)
        ));
    }

    #[tokio::test]
    async fn bare_json_reply_selects_the_kind() {
        let engine = MockEngine::new().with_chunks(vec![Ok("{\"kind\":\"TranslateWord\"}".into())]);
        let kind = classify(
            &engine,
            "m",
            Locale::Zh,
            &CLASSIFY_KINDS,
            &text_input("gloss"),
        )
        .await
        .expect("bare JSON must parse");
        assert_eq!(kind, TaskKind::TranslateWord);
    }

    #[tokio::test]
    async fn a_fenced_reply_repairs_then_falls_back() {
        let engine = MockEngine::new()
            .with_chunks(vec![Ok("```json\n{\"kind\":\"ExplainCode\"}\n```".into())]);
        let kind = classify(
            &engine,
            "m",
            Locale::En,
            &CLASSIFY_KINDS,
            &text_input("select * from t"),
        )
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
        let kind = classify(
            &engine,
            "m",
            Locale::Zh,
            &CLASSIFY_KINDS,
            &text_input("gloss"),
        )
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
        let kind = classify(
            &engine,
            "m",
            Locale::Zh,
            &CLASSIFY_KINDS,
            &text_input("gloss"),
        )
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
            classify(
                &engine,
                "m",
                Locale::Zh,
                &CLASSIFY_KINDS,
                &text_input("gloss"),
            )
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
            classify(
                &engine,
                "m",
                Locale::Zh,
                &CLASSIFY_KINDS,
                &text_input("gloss"),
            )
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
        let kind = classify(
            &engine,
            "m",
            Locale::Zh,
            &CLASSIFY_KINDS,
            &text_input("kubectl get pods"),
        )
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
            instruction.contains("reply is not a bare JSON object"),
            "the instruction carries the violation wording: {instruction}"
        );
        assert!(
            instruction.contains(crate::prompt::CLASSIFY_SCHEMA),
            "the instruction carries the classify contract line: {instruction}"
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
        let decision = classify_traced(
            &engine,
            "m",
            Locale::Zh,
            &CLASSIFY_KINDS,
            "kubectl get pods",
        )
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
        let decision = classify_traced(&engine, "m", Locale::Zh, &CLASSIFY_KINDS, "一段文本")
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
        let kind = classify(
            &engine,
            "m",
            Locale::Zh,
            &CLASSIFY_KINDS,
            &text_input("bonjour"),
        )
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
                            &CLASSIFY_KINDS,
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
    fn replies_outside_the_allowed_list_are_rejected() {
        assert!(
            parse_classify_reply(
                "{\"kind\":\"TranslateSentence\"}",
                &[TaskKind::TranslateWord]
            )
            .is_err()
        );
        assert!(parse_classify_reply("{\"kind\":\"ImageExplain\"}", &CLASSIFY_KINDS).is_err());
        assert!(parse_classify_reply("{\"kind\":\"Nonsense\"}", &CLASSIFY_KINDS).is_err());
        assert!(parse_classify_reply("{\"kind\":null}", &CLASSIFY_KINDS).is_err());
        assert!(parse_classify_reply("我觉得这是一段翻译", &CLASSIFY_KINDS).is_err());
        assert!(parse_classify_reply("", &CLASSIFY_KINDS).is_err());
        assert!(
            parse_classify_reply(
                "前置说明\n```gloss\n{\"kind\":\"TranslateWord\"}\n```",
                &CLASSIFY_KINDS
            )
            .is_err(),
            "the fence tolerance is gone: a gloss-fenced reply violates the \
             bare-JSON contract even with a valid kind inside"
        );
    }

    #[test]
    fn classify_constants_cover_the_text_kinds_with_an_off_list_fallback() {
        assert_eq!(
            CLASSIFY_KINDS,
            [
                TaskKind::TranslateWord,
                TaskKind::TranslateSentence,
                TaskKind::ExplainCode
            ]
        );
        assert_eq!(
            CLASSIFY_FALLBACK,
            TaskKind::General,
            "the fallback lands on the general kind"
        );
        assert!(
            !CLASSIFY_KINDS.contains(&CLASSIFY_FALLBACK),
            "the general kind is the fallback landing, never a classifier choice"
        );
        assert!(
            !CLASSIFY_KINDS.contains(&TaskKind::General),
            "the classifier still picks among the three text kinds only"
        );
    }
}
