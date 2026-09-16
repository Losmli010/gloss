//! SSE 增量解码（OpenAI 兼容 chat completions 的 `stream: true` 响应）。
//!
//! 纯逻辑、不碰网络：喂字节、吐增量。因此单测可以把同一段响应按任意字节边界
//! 切开，钉住「跨块不丢不重、多字节字符不被切坏」这类只有分块才会暴露的问题
//! ——UTF-8 之所以不会被切坏，是因为只解码以 `\n` 结尾的完整行，而 0x0A 不会
//! 出现在 UTF-8 多字节序列内部，行内字节必然是完整的。
//!
//! 只实现用得到的 SSE 子集，不做通用实现：
//! - `data: {json}` 一行一个事件；`data: [DONE]` 表示流结束；
//! - `:` 开头的注释/心跳行与 `event:` / `id:` 等字段一律忽略；
//! - `choices[0].delta.content` 是文本增量；只带 role 的首块没有 content，
//!   不产出增量；
//! - 服务端在流中报错时给 `{"error": {...}}`，按错误码映射到 [`GlossError`]。

use gloss_core::model::GlossError;

/// 诊断文本上限：错误消息会进 UI 与日志，服务端给的长文本没有价值。
const MAX_DIAGNOSTIC_CHARS: usize = 200;

/// 一轮解码的产出。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseItem {
    /// 文本增量。
    Delta(String),
    /// 服务端声明的流结束（`data: [DONE]`）。
    Done,
    /// 流中错误：服务端错误对象，或解析不出的协议内容。
    Failed(GlossError),
}

/// 行缓冲解码器：`push` 喂入任意切分的字节，返回本次可确定产出的条目。
#[derive(Debug, Default)]
pub struct SseDecoder {
    /// 尚未凑成完整行的尾巴（跨网络分块）。
    pending: Vec<u8>,
    /// 已见到 `[DONE]`：之后的字节一律忽略。
    done: bool,
}

impl SseDecoder {
    /// 新建解码器。
    pub fn new() -> Self {
        Self::default()
    }

    /// 喂入一段字节（分块边界任意），返回可产出的条目（顺序即流序）。
    pub fn push(&mut self, bytes: &[u8]) -> Vec<SseItem> {
        if self.done {
            return Vec::new();
        }
        self.pending.extend_from_slice(bytes);
        let mut items = Vec::new();
        // 只处理完整行，剩下的尾巴留给下一块——这也是跨块 UTF-8 不碎的保证。
        while let Some(pos) = self.pending.iter().position(|byte| *byte == b'\n') {
            let mut line: Vec<u8> = self.pending.drain(..=pos).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if let Some(item) = self.decode_line(&line) {
                let done = item == SseItem::Done;
                items.push(item);
                if done {
                    break;
                }
            }
        }
        items
    }

    /// 连接关闭时收尾。
    ///
    /// 服务端没给 `[DONE]` 就关流是兼容端点上的常见收尾方式（不算错误），
    /// 残缺的半行只可能是截断，留着也没法解析，直接丢弃。
    pub fn finish(&mut self) {
        self.pending.clear();
        self.done = true;
    }

    /// 解一行：返回 None 表示这行不产出任何东西（分隔空行、心跳、其它字段、
    /// 以及只带 role 的块）。
    fn decode_line(&mut self, line: &[u8]) -> Option<SseItem> {
        let Ok(text) = std::str::from_utf8(line) else {
            return Some(SseItem::Failed(GlossError::EngineResponse(
                "non-utf8 line in stream".into(),
            )));
        };
        let trimmed = text.trim();
        if trimmed.is_empty() || trimmed.starts_with(':') {
            return None;
        }
        let payload = trimmed.strip_prefix("data:")?.trim();
        if payload.is_empty() {
            // 无载荷的 `data:` 行（个别兼容实现拿它当心跳）不产出东西。
            return None;
        }
        if payload == "[DONE]" {
            self.done = true;
            return Some(SseItem::Done);
        }
        decode_chunk(payload)
    }
}

/// 单个 `data:` 载荷：取增量内容、识别服务端错误；`None` = 这行不产出东西
/// （例如只带 role 的首块）。
fn decode_chunk(payload: &str) -> Option<SseItem> {
    let Ok(chunk) = serde_json::from_str::<StreamChunk>(payload) else {
        return Some(SseItem::Failed(GlossError::EngineResponse(format!(
            "unparsable stream chunk: {}",
            truncate(payload)
        ))));
    };
    if let Some(error) = chunk.error {
        return Some(SseItem::Failed(map_api_error(&error)));
    }
    // 本客户端每次只发一条 user 消息，choices 恒为一条；增量只在 content 上。
    chunk
        .choices
        .into_iter()
        .find_map(|choice| choice.delta.and_then(|delta| delta.content))
        .filter(|content| !content.is_empty())
        .map(SseItem::Delta)
}

/// 服务端错误对象 → [`GlossError`]：能识别的按语义分类，其余按协议错误透出
/// 服务端给的诊断文本（只取 `error.message`，不转述整个响应体）。
fn map_api_error(error: &ApiError) -> GlossError {
    let code = error
        .code
        .as_deref()
        .or(error.kind.as_deref())
        .unwrap_or_default();
    match code {
        "invalid_api_key" | "authentication_error" | "permission_denied" => GlossError::EngineAuth,
        "rate_limit_exceeded" | "rate_limit_error" => GlossError::EngineRateLimited,
        _ => {
            let message = error
                .message
                .as_deref()
                .map_or_else(|| "unspecified".to_owned(), truncate);
            GlossError::EngineResponse(format!("{code}: {message}"))
        }
    }
}

/// 诊断文本上限（按字符截断，避免切坏多字节字符）。
fn truncate(text: &str) -> String {
    if text.chars().count() <= MAX_DIAGNOSTIC_CHARS {
        return text.to_owned();
    }
    let head: String = text.chars().take(MAX_DIAGNOSTIC_CHARS).collect();
    format!("{head}…")
}

/// 流式响应的一块：OpenAI 兼容 `chat.completion.chunk`。
#[derive(serde::Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
    #[serde(default)]
    error: Option<ApiError>,
}

/// 一条候选回复（本客户端只用第一条）。
#[derive(serde::Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: Option<StreamDelta>,
}

/// 增量内容。
#[derive(serde::Deserialize)]
struct StreamDelta {
    #[serde(default)]
    content: Option<String>,
}

/// 服务端错误对象。
#[derive(serde::Deserialize)]
struct ApiError {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    code: Option<String>,
    /// OpenAI 用 `type` 字段，DeepSeek 等兼容实现两者都给。
    #[serde(default, rename = "type")]
    kind: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一段真实的 OpenAI 兼容流：首块只有 role，中间两块有增量，最后 `[DONE]`。
    fn sample_stream() -> &'static str {
        concat!(
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"},\"index\":0}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"光\"},\"index\":0}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"泽\"},\"index\":0}]}\n\n",
            "data: [DONE]\n\n",
        )
    }

    /// 一次喂完整段：只产出两段增量加一个结束标记（role 块与空行不产出）。
    #[test]
    fn decodes_a_complete_stream() {
        let mut decoder = SseDecoder::new();
        let items = decoder.push(sample_stream().as_bytes());
        assert_eq!(
            items,
            vec![
                SseItem::Delta("光".into()),
                SseItem::Delta("泽".into()),
                SseItem::Done,
            ]
        );
    }

    /// 按 1..n 字节的每种切法喂同一段响应，产出的增量必须完全一致——
    /// 这是「网络分块边界任意」的回归护栏（含把多字节字符切成两半的情形）。
    #[test]
    fn decodes_identically_for_every_chunk_split() {
        let payload = sample_stream().as_bytes();
        let expected = vec![
            SseItem::Delta("光".into()),
            SseItem::Delta("泽".into()),
            SseItem::Done,
        ];
        for chunk_size in 1..=payload.len() {
            let mut decoder = SseDecoder::new();
            let mut items = Vec::new();
            for chunk in payload.chunks(chunk_size) {
                items.extend(decoder.push(chunk));
            }
            assert_eq!(items, expected, "chunk_size = {chunk_size}");
        }
    }

    /// 逐字节喂入：多字节字符被切在最中间也不能产出坏字符。
    #[test]
    fn byte_by_byte_input_keeps_multibyte_characters_intact() {
        let mut decoder = SseDecoder::new();
        let mut items = Vec::new();
        for byte in sample_stream().as_bytes() {
            items.extend(decoder.push(std::slice::from_ref(byte)));
        }
        let text: String = items
            .iter()
            .filter_map(|item| match item {
                SseItem::Delta(delta) => Some(delta.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "光泽");
    }

    /// CRLF 行尾、心跳注释行与其它 SSE 字段都不产出增量。
    #[test]
    fn ignores_crlf_heartbeats_and_other_fields() {
        let stream = concat!(
            ": keep-alive\r\n",
            "event: message\r\n",
            "id: 1\r\n",
            "data:{\"choices\":[{\"delta\":{\"content\":\"好\"}}]}\r\n",
            "\r\n",
            "data: [DONE]\r\n",
        );
        let mut decoder = SseDecoder::new();
        assert_eq!(
            decoder.push(stream.as_bytes()),
            vec![SseItem::Delta("好".into()), SseItem::Done]
        );
    }

    /// 流中错误按错误码分类：限流 → 可退避重试；鉴权类 → 引导设置页；
    /// 识别不出的按协议错误带上服务端诊断文本。
    #[test]
    fn maps_server_errors_by_code() {
        let rate_limited = decode_chunk(
            r#"{"error":{"message":"Rate limit reached","type":"rate_limit_exceeded"}}"#,
        );
        assert_eq!(
            rate_limited,
            Some(SseItem::Failed(GlossError::EngineRateLimited)),
            "rate limit must be retryable"
        );

        let auth = decode_chunk(r#"{"error":{"message":"Invalid key","code":"invalid_api_key"}}"#);
        assert_eq!(auth, Some(SseItem::Failed(GlossError::EngineAuth)));

        let other =
            decode_chunk(r#"{"error":{"message":"model not found","code":"model_not_found"}}"#);
        assert_eq!(
            other,
            Some(SseItem::Failed(GlossError::EngineResponse(
                "model_not_found: model not found".into()
            )))
        );
    }

    /// 解析不出的载荷是协议错误，且诊断文本有长度上限。
    #[test]
    fn unparsable_payload_reports_bounded_diagnostic() {
        let Some(SseItem::Failed(error)) = decode_chunk("not json") else {
            panic!("expected failure");
        };
        assert!(
            matches!(error, GlossError::EngineResponse(_)),
            "got {error:?}"
        );

        let long_message = "x".repeat(MAX_DIAGNOSTIC_CHARS * 2);
        let Some(SseItem::Failed(error)) = decode_chunk(&format!(
            "{{\"error\":{{\"message\":\"{long_message}\",\"code\":\"other\"}}}}"
        )) else {
            panic!("expected failure");
        };
        let GlossError::EngineResponse(text) = error else {
            panic!("expected response error");
        };
        assert!(
            text.chars().count() <= MAX_DIAGNOSTIC_CHARS + 8,
            "diagnostic must be bounded, got {} chars",
            text.chars().count()
        );
    }

    /// 空载荷的 `data:` 行按心跳忽略（不报协议错误、不产出增量）。
    #[test]
    fn ignores_a_data_line_without_payload() {
        let mut decoder = SseDecoder::new();
        assert_eq!(
            decoder.push(b"data:\n\ndata: [DONE]\n"),
            vec![SseItem::Done]
        );
    }

    /// 见到 `[DONE]` 之后的字节一律忽略（连接上可能还有填充）。
    #[test]
    fn ignores_bytes_after_done() {
        let mut decoder = SseDecoder::new();
        assert_eq!(decoder.push(b"data: [DONE]\n"), vec![SseItem::Done]);
        assert_eq!(
            decoder.push(b"data: {\"choices\":[{\"delta\":{\"content\":\"late\"}}]}\n"),
            Vec::new()
        );
    }

    /// 收尾清掉残缺半行，不产出任何东西（截断不是错误，只是没法解析）。
    #[test]
    fn finish_drops_a_truncated_tail() {
        let mut decoder = SseDecoder::new();
        assert_eq!(
            decoder.push(b"data: {\"choices\":[{\"delta\":{\"cont"),
            Vec::new()
        );
        decoder.finish();
        assert_eq!(decoder.push(b"ent\":\"x\"}}]}\n"), Vec::new());
    }
}
