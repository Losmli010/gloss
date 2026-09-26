//! 自动任务分类（前半程）：待分类的文本输入 → 具体任务类型。
//!
//! 判定顺序：启发式直通（代码语言提示 → [`TaskKind::ExplainCode`]，不调
//! 引擎）→ LLM 分类（极小 prompt + [`CLASSIFY_MAX_TOKENS`] 截断）→ JSON
//! 校验（kind 必须在允许清单内）。解析先认 `{"kind": "…"}` 裸 JSON，失败
//! 退围栏提取；识别不出即 `Err`——回退到兜底 kind 是调用方（app 桥）的
//! 编排决策，本模块不做静默回退。
//!
//! 内容红线：分类输出契约只有一个 kind 标识，没有理由字段；本模块的
//! 错误与日志（由调用方记）都不携带输入内容与模型回复原文。
//!
//! 分类缓存 key 由 [`classify_key`] 统一派生，与主产物的 `cache_key`
//! 各走各的键空间（分实例存放，见 `ports::Cache`）。

use std::hash::{Hash, Hasher};

use crate::log::debug;
use crate::model::{GlossError, Locale};
use crate::ports::{AiEngine, EngineRequest};
use crate::prompt::{PromptRegistry, STRUCTURED_FENCE};
use crate::task::{InputHint, TaskInput, TaskKind};

/// 分类回复的 token 上限（OpenAI 兼容 `max_tokens`）：正确回复是一个
/// 只含 kind 标识的小 JSON，截断只落在模型跑偏的长篇上——跑偏的回复
/// 解析不过，走调用方的兜底。
pub const CLASSIFY_MAX_TOKENS: u32 = 128;

/// 回复累积上限（字节）：`max_tokens` 之外的第二道界，防止不守约的
/// 端点把无界回复灌进内存。超限部分丢弃（按字符边界截断）——截断的
/// JSON 解析不过，同样落到兜底。
const REPLY_CAP_BYTES: usize = 4096;

/// 模态提示的启发式直通：能不经 LLM 直接定型的 kind（当前只有代码
/// 语言提示 → [`TaskKind::ExplainCode`]）。桥在查缓存前用它截住确定性
/// 答案，[`classify`] 内部用它短路引擎调用——规则单点在这。
pub fn hint_kind(hint: Option<&InputHint>) -> Option<TaskKind> {
    matches!(hint, Some(InputHint::CodeLanguage(_))).then_some(TaskKind::ExplainCode)
}

/// 判定一条文本输入的任务类型。`allowed` 是允许模型选择的清单（调用方
/// 按「text-capable ∩ enabled」算好传入），识别结果超出清单按未识别
/// 处理。`model` 参与分类缓存 key（见 [`classify_key`]）。
pub async fn classify(
    engine: &dyn AiEngine,
    model: &str,
    locale: Locale,
    allowed: &[TaskKind],
    input: &TaskInput,
) -> Result<TaskKind, GlossError> {
    let TaskInput::Text { text, hint } = input else {
        // 分类只服务划词路径；图像/音频任务带着具体 kind 进编排，
        // 到不了这里。
        return Err(GlossError::UnsupportedModality);
    };
    // 启发式直通：能由提示定型的输入不花一次往返。
    if let Some(kind) = hint_kind(hint.as_ref()) {
        debug!(kind = ?kind, "classified by the input hint");
        return Ok(kind);
    }

    let messages = PromptRegistry::new().render_classify(locale, allowed, text);
    let request = EngineRequest {
        kind: TaskKind::Auto,
        messages,
        model: model.to_owned(),
        max_tokens: Some(CLASSIFY_MAX_TOKENS),
    };
    let mut stream = engine.execute(&request).await?;
    let mut reply = String::new();
    while let Some(item) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
        match item {
            Ok(delta) => {
                let budget = REPLY_CAP_BYTES.saturating_sub(reply.len());
                if budget > 0 {
                    let mut take = budget.min(delta.len());
                    while take > 0 && !delta.is_char_boundary(take) {
                        take -= 1;
                    }
                    reply.push_str(&delta[..take]);
                }
            }
            Err(error) => return Err(error),
        }
    }
    let kind = parse_classify_reply(&reply, allowed)?;
    debug!(kind = ?kind, "classified by the model");
    Ok(kind)
}

/// 派生分类缓存 key：文本、模态提示、模板语言与分类模型共同参与——
/// 允许清单不参与（设置改动后由调用方对缓存命中再做校验）。
pub fn classify_key(text: &str, hint: Option<&InputHint>, locale: Locale, model: &str) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    text.hash(&mut hasher);
    hint.hash(&mut hasher);
    locale.hash(&mut hasher);
    model.hash(&mut hasher);
    hasher.finish()
}

/// 解析模型回复：先按裸 JSON 解析，失败退到围栏提取（```gloss 或
/// ```json 围栏都收）。kind 标识必须是 [`TaskKind`] 的 serde 名且在
/// `allowed` 清单内，否则一律 [`GlossError::EngineResponse`]——错误文本
/// 是固定措辞，不携带回复原文。
fn parse_classify_reply(reply: &str, allowed: &[TaskKind]) -> Result<TaskKind, GlossError> {
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
    use super::{classify, classify_key, parse_classify_reply};
    use crate::model::{GlossError, Locale};
    use crate::stubs::engine::MockEngine;
    use crate::task::{InputHint, TaskInput, TaskKind};

    fn text_input(text: &str, hint: Option<InputHint>) -> TaskInput {
        TaskInput::Text {
            text: text.into(),
            hint,
        }
    }

    fn all_text_kinds() -> Vec<TaskKind> {
        vec![
            TaskKind::TranslateWord,
            TaskKind::TranslateSentence,
            TaskKind::ExplainCode,
        ]
    }

    #[tokio::test]
    async fn code_language_hint_short_circuits_without_the_engine() {
        let engine = MockEngine::new().with_chunks(vec![Ok("{\"kind\":\"TranslateWord\"}".into())]);
        let kind = classify(
            &engine,
            "m",
            Locale::Zh,
            &all_text_kinds(),
            &text_input("fn main() {}", Some(InputHint::CodeLanguage("rust".into()))),
        )
        .await
        .expect("hint must classify directly");
        assert_eq!(kind, TaskKind::ExplainCode);
        assert_eq!(engine.call_count(), 0, "the hint needs no round trip");
    }

    #[tokio::test]
    async fn non_text_input_is_rejected() {
        let engine = MockEngine::new();
        assert!(matches!(
            classify(
                &engine,
                "m",
                Locale::Zh,
                &all_text_kinds(),
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
            &all_text_kinds(),
            &text_input("gloss", None),
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
            &all_text_kinds(),
            &text_input("select * from t", None),
        )
        .await
        .expect("a fenced reply must still parse");
        assert_eq!(kind, TaskKind::ExplainCode);
    }

    #[tokio::test]
    async fn engine_failure_propagates() {
        let engine = MockEngine::new().with_execute_failure(GlossError::EngineRateLimited);
        assert_eq!(
            classify(
                &engine,
                "m",
                Locale::Zh,
                &all_text_kinds(),
                &text_input("gloss", None),
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
        assert!(parse_classify_reply("{\"kind\":\"ImageOcr\"}", &all_text_kinds()).is_err());
        assert!(parse_classify_reply("{\"kind\":\"Nonsense\"}", &all_text_kinds()).is_err());
        assert!(parse_classify_reply("{\"kind\":null}", &all_text_kinds()).is_err());
        assert!(parse_classify_reply("我觉得这是一段翻译", &all_text_kinds()).is_err());
        assert!(parse_classify_reply("", &all_text_kinds()).is_err());
        assert_eq!(
            parse_classify_reply(
                "前置说明\n```gloss\n{\"kind\":\"TranslateWord\"}\n```",
                &all_text_kinds()
            )
            .expect("gloss-fenced reply parses"),
            TaskKind::TranslateWord
        );
    }

    #[test]
    fn classify_key_is_stable_and_sensitive() {
        let base = classify_key("gloss", None, Locale::Zh, "m");
        assert_eq!(base, classify_key("gloss", None, Locale::Zh, "m"));
        assert_ne!(base, classify_key("gloss2", None, Locale::Zh, "m"), "text");
        assert_ne!(
            base,
            classify_key(
                "gloss",
                Some(&InputHint::CodeLanguage("rs".into())),
                Locale::Zh,
                "m"
            ),
            "hint"
        );
        assert_ne!(base, classify_key("gloss", None, Locale::En, "m"), "locale");
        assert_ne!(base, classify_key("gloss", None, Locale::Zh, "m2"), "model");
    }
}
