//! 自动任务分类（编排前半程）：文本输入 → 具体任务类型。
//!
//! 判定完全交给 LLM（极小 prompt + [`CLASSIFY_MAX_TOKENS`] 截断）→ JSON
//! 校验（kind 必须在允许清单内）——源语言与代码语言同样由模型从原文
//! 自行判断，取材不再携带模态提示。解析先认 `{"kind": "…"}` 裸 JSON，
//! 失败退围栏提取；识别不出即 `Err`——回退到兜底 kind
//! （[`CLASSIFY_FALLBACK`]）是编排层（engine）的决策，本模块不做静默
//! 回退。流式循环内每次累积后即尝试解析，首个能通过校验的完整 JSON
//! 立即定型返回（不等流自然结束，见 [`classify`]）。
//!
//! 内容红线：分类输出契约只有一个 kind 标识，没有理由字段；本模块的
//! 错误与日志（由调用方记）都不携带输入内容与模型回复原文。

use crate::log::debug;
use crate::model::{GlossError, Locale};
use crate::ports::{AiEngine, EngineRequest};
use crate::prompt::{PromptRegistry, STRUCTURED_FENCE};
use crate::task::{TaskInput, TaskKind};

/// 分类的允许清单：分类只服务划词路径，答案集合就是**文本取材可执行**
/// 的三个 kind（图像/语音 kind 不在其中——划词到不了它们）。
pub const CLASSIFY_KINDS: [TaskKind; 3] = [
    TaskKind::TranslateWord,
    TaskKind::TranslateSentence,
    TaskKind::ExplainCode,
];

/// 分类失败（引擎错误、回复解析不过）时的兜底 kind：常量而非配置——
/// 兜底必须是可渲染、可执行的具体文本 kind，词卡是划词最高频的意图。
pub const CLASSIFY_FALLBACK: TaskKind = TaskKind::TranslateWord;

/// 分类回复的 token 上限（OpenAI 兼容 `max_tokens`）：正确回复是一个
/// 只含 kind 标识的小 JSON，截断只落在模型跑偏的长篇上——跑偏的回复
/// 解析不过，走调用方的兜底。
pub const CLASSIFY_MAX_TOKENS: u32 = 128;

/// 回复累积上限（字节）：`max_tokens` 之外的第二道界，防止不守约的
/// 端点把无界回复灌进内存。超限部分丢弃（按字符边界截断）——截断的
/// JSON 解析不过，同样落到兜底。pub 供 gloss-eval 的 live 轨复用同一
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
/// 传 [`CLASSIFY_KINDS`]），识别结果超出清单按未识别处理。
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

    let messages = PromptRegistry::new().render_classify(locale, allowed, text);
    let request = EngineRequest {
        messages,
        model: model.to_owned(),
        max_tokens: Some(CLASSIFY_MAX_TOKENS),
    };
    let mut stream = engine.execute(request).await?;
    let mut reply = String::new();
    while let Some(item) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
        match item {
            Ok(delta) => {
                push_capped(&mut reply, &delta);
                // 增量解析提前退出：裸 JSON 到闭合括号即定型，不等流自然
                // 结束——首个能通过校验的完整 JSON 就是判定，其后的增量
                // 与结束帧作废（提前还流即还回 TTFT）。只在 Ok 时退出：
                // 部分 JSON 继续等，跑偏与截断的回复照旧走完流、落到循环
                // 外的同一条解析路径。每次累积后的全量 parse 上界是回复
                // 上限（4KB）× 个位数 chunk，相对 chunk 的毫秒级到达间隔
                // 可忽略。
                if let Ok(kind) = parse_classify_reply(&reply, allowed) {
                    debug!(kind = ?kind, "classified by the model");
                    return Ok(kind);
                }
            }
            Err(error) => return Err(error),
        }
    }
    let kind = parse_classify_reply(&reply, allowed)?;
    debug!(kind = ?kind, "classified by the model");
    Ok(kind)
}

/// 解析模型回复：先按裸 JSON 解析，失败退到围栏提取（```gloss 或
/// ```json 围栏都收）。kind 标识必须是 [`TaskKind`] 的 serde 名且在
/// `allowed` 清单内，否则一律 [`GlossError::EngineResponse`]——错误文本
/// 是固定措辞，不携带回复原文。
///
/// pub 供 gloss-eval 评测重放复用：评测与生产走同一个校验器。
pub fn parse_classify_reply(reply: &str, allowed: &[TaskKind]) -> Result<TaskKind, GlossError> {
    let rejected = || GlossError::EngineResponse("unrecognized classify reply".into());
    let trimmed = reply.trim();
    let value = serde_json::from_str::<serde_json::Value>(trimmed)
        .ok()
        .or_else(|| fenced_json(trimmed).and_then(|json| serde_json::from_str(json).ok()));
    let Some(value) = value else {
        return Err(rejected());
    };
    let Some(kind) = value
        .get("kind")
        .and_then(|v| v.as_str())
        .map(|name| serde_json::Value::String(name.to_owned()))
        .and_then(|name| serde_json::from_value::<TaskKind>(name).ok())
    else {
        return Err(rejected());
    };
    if allowed.contains(&kind) {
        Ok(kind)
    } else {
        Err(rejected())
    }
}

/// 从回复中提取第一个代码围栏的正文（跳过 ` ``` ` 后的语言标识行，
/// 到闭合 ` ``` ` 为止）。分类契约请模型裸输出，围栏是它不守约时的
/// 容错位。
fn fenced_json(reply: &str) -> Option<&str> {
    let after_marker = match reply.strip_prefix(STRUCTURED_FENCE) {
        Some(rest) => rest,
        None => {
            let start = reply.find("```")?;
            &reply[start + 3..]
        }
    };
    let content = match after_marker.find('\n') {
        // 语言标识（如 json）独占首行，跳到行首之后。
        Some(line_end) => &after_marker[line_end + 1..],
        None => after_marker,
    };
    let end = content.find("```")?;
    Some(content[..end].trim())
}

#[cfg(test)]
mod tests {
    use super::{CLASSIFY_FALLBACK, CLASSIFY_KINDS, classify, parse_classify_reply};
    use crate::model::{GlossError, Locale};
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
    async fn fenced_reply_is_extracted_despite_the_contract() {
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
        .expect("a fenced reply must still parse");
        assert_eq!(kind, TaskKind::ExplainCode);
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
        assert!(
            classify(
                &engine,
                "m",
                Locale::Zh,
                &CLASSIFY_KINDS,
                &text_input("gloss"),
            )
            .await
            .is_err(),
            "a truncated reply must not settle"
        );
    }

    #[tokio::test]
    async fn engine_failure_propagates() {
        let engine = MockEngine::new().with_execute_failure(GlossError::EngineRateLimited);
        assert_eq!(
            classify(
                &engine,
                "m",
                Locale::Zh,
                &CLASSIFY_KINDS,
                &text_input("gloss"),
            )
            .await,
            Err(GlossError::EngineRateLimited)
        );
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
        assert!(parse_classify_reply("{\"kind\":\"ImageOcr\"}", &CLASSIFY_KINDS).is_err());
        assert!(parse_classify_reply("{\"kind\":\"Nonsense\"}", &CLASSIFY_KINDS).is_err());
        assert!(parse_classify_reply("{\"kind\":null}", &CLASSIFY_KINDS).is_err());
        assert!(parse_classify_reply("我觉得这是一段翻译", &CLASSIFY_KINDS).is_err());
        assert!(parse_classify_reply("", &CLASSIFY_KINDS).is_err());
        assert_eq!(
            parse_classify_reply(
                "前置说明\n```gloss\n{\"kind\":\"TranslateWord\"}\n```",
                &CLASSIFY_KINDS
            )
            .expect("gloss-fenced reply parses"),
            TaskKind::TranslateWord
        );
    }

    #[test]
    fn classify_constants_cover_the_text_kinds_with_a_concrete_fallback() {
        assert_eq!(
            CLASSIFY_KINDS,
            [
                TaskKind::TranslateWord,
                TaskKind::TranslateSentence,
                TaskKind::ExplainCode
            ]
        );
        assert!(CLASSIFY_KINDS.contains(&CLASSIFY_FALLBACK));
    }
}
