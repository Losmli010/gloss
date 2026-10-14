//! SSE 增量解码（OpenAI 兼容 chat completions 的 `stream: true` 响应）。
//!
//! 纯逻辑、不碰网络：喂字节、吐增量。因此单测可以把同一段响应按任意字节边界
//! 切开，钉住「跨块不丢不重、多字节字符不被切坏」这类只有分块才会暴露的问题。
//!
//! 只实现用得到的 SSE 子集，不做通用实现：
//! - `data: {json}` 一行一个事件；`data: [DONE]` 表示流结束；
//! - `:` 开头的注释/心跳行与 `event:` / `id:` 等字段一律忽略；
//! - `choices[0].delta.content` 是文本增量；只带 role 的首块没有 content，
//!   不产出增量；
//! - `choices[0].finish_reason` 随块捕获，在 `[DONE]` 上随 [`SseItem::Done`]
//!   交出（服务端没给 `[DONE]` 就关流的，结束原因未知，不产出 Done）；
//! - 服务端在流中报错时给 `{"error": {...}}`，按错误码映射到 [`GlossError`]。

use crate::model::GlossError;

/// 诊断文本上限：错误消息会进 UI 与日志，服务端给的长文本没有价值。
const MAX_DIAGNOSTIC_CHARS: usize = 200;

/// 单行上限：OpenAI 兼容载荷恒为单行 JSON，超限即协议异常。没有这条，一个
/// 只发字节不发 `\n` 的服务端（或坏网关）就能让 `pending` 无界增长。
const MAX_LINE_BYTES: usize = 1 << 20;

/// 一轮解码的产出。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseItem {
    /// 文本增量。
    Delta(String),
    /// 服务端声明的流结束（`data: [DONE]`），携带捕获到的
    /// `choices[0].finish_reason`（整条流从未声明则为 `None`）。
    Done {
        /// 结束原因原文（`stop` / `length` / `content_filter`…）。
        finish_reason: Option<String>,
    },
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
    /// 最近一次捕获的 `choices[0].finish_reason`（结束原因先于 `[DONE]`
    /// 随最后一个内容块到达），交出后清空。
    finish_reason: Option<String>,
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
                let done = matches!(item, SseItem::Done { .. });
                items.push(item);
                if done {
                    break;
                }
            }
        }
        // 攒下的尾巴超限就是协议异常：终结流，别继续为它占内存。
        if !self.done && self.pending.len() > MAX_LINE_BYTES {
            self.pending.clear();
            self.done = true;
            items.push(SseItem::Failed(GlossError::EngineResponse(
                "sse line exceeds limit".into(),
            )));
        }
        items
    }

    /// 连接关闭时收尾，返回尾巴里还能交出去的增量。
    ///
    /// 服务端没给 `[DONE]` 就关流是兼容端点上的常见收尾方式（不算错误）。尾巴
    /// 里可能是「完整但没被换行终止的最后一行」——端点 flush 完最后一块数据行
    /// 直接关连接时就是这种形态，丢了就是一个真实增量；解析不出来的一律忽略
    /// （截断不该升级成任务失败）。
    pub fn finish(&mut self) -> Vec<SseItem> {
        let mut items = Vec::new();
        if !self.done && !self.pending.is_empty() {
            let tail = std::mem::take(&mut self.pending);
            if let Some(item @ SseItem::Delta(_)) = self.decode_line(&tail) {
                items.push(item);
            }
        }
        self.pending.clear();
        self.done = true;
        items
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
            return Some(SseItem::Done {
                finish_reason: self.finish_reason.take(),
            });
        }
        self.decode_data(payload)
    }

    /// 单个 `data:` 载荷：捕获 `choices[0].finish_reason`、取增量内容、识别
    /// 服务端错误；`None` = 这行不产出东西（例如只带 role 的首块）。
    fn decode_data(&mut self, payload: &str) -> Option<SseItem> {
        let Ok(chunk) = serde_json::from_str::<StreamChunk>(payload) else {
            return Some(SseItem::Failed(GlossError::EngineResponse(format!(
                "unparsable stream chunk: {}",
                truncate(payload)
            ))));
        };
        if let Some(error) = chunk.error {
            return Some(SseItem::Failed(map_api_error(&error)));
        }
        // 本客户端每次只发一条 user 消息，choices 恒为一条；结束原因只认
        // 第一条 choice，增量取第一条携带 content 的 choice（一并把结束
        // 原因随 choice 所有权带出，不再复制字符串）。
        let mut finish_reason = None;
        let delta = chunk
            .choices
            .into_iter()
            .enumerate()
            .find_map(|(index, choice)| {
                if index == 0 {
                    finish_reason = choice.finish_reason;
                }
                choice.delta.and_then(|delta| delta.content)
            })
            .filter(|content| !content.is_empty());
        if finish_reason.is_some() {
            self.finish_reason = finish_reason;
        }
        delta.map(SseItem::Delta)
    }
}

/// 服务端错误对象 → [`GlossError`]：能识别的按语义分类，其余按协议错误透出
/// 服务端给的诊断文本（只取 `error.message`，不转述整个响应体）。
fn map_api_error(error: &ApiError) -> GlossError {
    // `code` 与 `type` 分别匹配：OpenAI 用 type、DeepSeek 等用 code，兼容实现
    // 可能只填其中一个，或 code 是通用值而细分类在 type 里。
    let codes: Vec<&str> = [error.code.as_deref(), error.kind.as_deref()]
        .into_iter()
        .flatten()
        .filter(|code| !code.is_empty())
        .collect();
    const AUTH: [&str; 3] = [
        "invalid_api_key",
        "authentication_error",
        "permission_denied",
    ];
    const RATE_LIMITED: [&str; 2] = ["rate_limit_exceeded", "rate_limit_error"];
    if codes.iter().any(|code| AUTH.contains(code)) {
        return GlossError::EngineAuth;
    }
    if codes.iter().any(|code| RATE_LIMITED.contains(code)) {
        return GlossError::EngineRateLimited;
    }
    let message = error
        .message
        .as_deref()
        .map_or_else(|| "unspecified".to_owned(), truncate);
    // 服务端只给 message 时别造出 「: message」这种带前导冒号的文案。
    match codes.first() {
        Some(code) => GlossError::EngineResponse(format!("{code}: {message}")),
        None => GlossError::EngineResponse(message),
    }
}

/// 诊断文本上限（按字符截断，避免切坏多字节字符）。
fn truncate(text: &str) -> String {
    if text.chars().count() <= MAX_DIAGNOSTIC_CHARS {
        return text.to_owned();
    }
    let head: String = text.chars().take(MAX_DIAGNOSTIC_CHARS).collect();
    format!("{head}…") // i18n:allow 诊断截断的省略号，非 locale 文案
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
    /// 结束原因：恒为 `None`，只在最后一个内容块上非空（`stop` /
    /// `length` / `content_filter`…）。
    #[serde(default)]
    finish_reason: Option<String>,
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

    fn sample_stream() -> &'static str {
        concat!(
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"},\"index\":0}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"光\"},\"index\":0}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"泽\"},\"index\":0}]}\n\n",
            "data: [DONE]\n\n",
        )
    }

    fn decode_data_line(payload: &str) -> Option<SseItem> {
        SseDecoder::new().decode_data(payload)
    }

    #[test]
    fn decodes_a_complete_stream() {
        let mut decoder = SseDecoder::new();
        let items = decoder.push(sample_stream().as_bytes());
        assert_eq!(
            items,
            vec![
                SseItem::Delta("光".into()),
                SseItem::Delta("泽".into()),
                SseItem::Done {
                    finish_reason: None
                },
            ]
        );
    }

    #[test]
    fn done_carries_the_server_finish_reason() {
        let stream = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"好\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        );
        let mut decoder = SseDecoder::new();
        assert_eq!(
            decoder.push(stream.as_bytes()),
            vec![
                SseItem::Delta("好".into()),
                SseItem::Done {
                    finish_reason: Some("stop".into())
                },
            ]
        );

        let truncated = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"截\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\n",
            "data: [DONE]\n\n",
        );
        let mut decoder = SseDecoder::new();
        assert_eq!(
            decoder.push(truncated.as_bytes()),
            vec![
                SseItem::Delta("截".into()),
                SseItem::Done {
                    finish_reason: Some("length".into())
                },
            ]
        );

        let mut decoder = SseDecoder::new();
        decoder.push("data: {\"choices\":[{\"delta\":{\"content\":\"无\"}}]}\n\n".as_bytes());
        assert_eq!(
            decoder.push(b"data: [DONE]\n\n"),
            vec![SseItem::Done {
                finish_reason: None
            }],
            "a stream that never declares a finish reason ends with None"
        );
    }

    #[test]
    fn decodes_identically_for_every_chunk_split() {
        let payload = sample_stream().as_bytes();
        let expected = vec![
            SseItem::Delta("光".into()),
            SseItem::Delta("泽".into()),
            SseItem::Done {
                finish_reason: None,
            },
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
            vec![
                SseItem::Delta("好".into()),
                SseItem::Done {
                    finish_reason: None
                }
            ]
        );
    }

    #[test]
    fn maps_server_errors_by_code() {
        let rate_limited = decode_data_line(
            r#"{"error":{"message":"Rate limit reached","type":"rate_limit_exceeded"}}"#,
        );
        assert_eq!(
            rate_limited,
            Some(SseItem::Failed(GlossError::EngineRateLimited)),
            "rate limit must be retryable"
        );

        let auth =
            decode_data_line(r#"{"error":{"message":"Invalid key","code":"invalid_api_key"}}"#);
        assert_eq!(auth, Some(SseItem::Failed(GlossError::EngineAuth)));

        let other =
            decode_data_line(r#"{"error":{"message":"model not found","code":"model_not_found"}}"#);
        assert_eq!(
            other,
            Some(SseItem::Failed(GlossError::EngineResponse(
                "model_not_found: model not found".into()
            )))
        );
    }

    #[test]
    fn unparsable_payload_reports_bounded_diagnostic() {
        let Some(SseItem::Failed(error)) = decode_data_line("not json") else {
            panic!("expected failure");
        };
        assert!(
            matches!(error, GlossError::EngineResponse(_)),
            "got {error:?}"
        );

        let long_message = "x".repeat(MAX_DIAGNOSTIC_CHARS * 2);
        let Some(SseItem::Failed(error)) = decode_data_line(&format!(
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

    #[test]
    fn ignores_a_data_line_without_payload() {
        let mut decoder = SseDecoder::new();
        assert_eq!(
            decoder.push(b"data:\n\ndata: [DONE]\n"),
            vec![SseItem::Done {
                finish_reason: None
            }]
        );
    }

    #[test]
    fn ignores_bytes_after_done() {
        let mut decoder = SseDecoder::new();
        assert_eq!(
            decoder.push(b"data: [DONE]\n"),
            vec![SseItem::Done {
                finish_reason: None
            }]
        );
        assert_eq!(
            decoder.push(b"data: {\"choices\":[{\"delta\":{\"content\":\"late\"}}]}\n"),
            Vec::new()
        );
    }

    #[test]
    fn finish_drops_a_truncated_tail() {
        let mut decoder = SseDecoder::new();
        assert_eq!(
            decoder.push(b"data: {\"choices\":[{\"delta\":{\"cont"),
            Vec::new()
        );
        assert_eq!(decoder.finish(), Vec::new());
        assert_eq!(decoder.push(b"ent\":\"x\"}}]}\n"), Vec::new());
    }

    #[test]
    fn finish_keeps_a_complete_unterminated_line() {
        let mut decoder = SseDecoder::new();
        assert_eq!(
            decoder.push("data: {\"choices\":[{\"delta\":{\"content\":\"尾\"}}]}".as_bytes()),
            Vec::new()
        );
        assert_eq!(decoder.finish(), vec![SseItem::Delta("尾".into())]);
    }

    #[test]
    fn rejects_a_line_beyond_the_limit() {
        let mut decoder = SseDecoder::new();
        let flood = vec![b'a'; MAX_LINE_BYTES + 1];
        assert_eq!(
            decoder.push(&flood),
            vec![SseItem::Failed(GlossError::EngineResponse(
                "sse line exceeds limit".into()
            ))]
        );
        assert_eq!(decoder.push(b"data: [DONE]\n"), Vec::new());
    }

    #[test]
    fn error_without_code_reports_the_message_only() {
        assert_eq!(
            decode_data_line(r#"{"error":{"message":"boom"}}"#),
            Some(SseItem::Failed(GlossError::EngineResponse("boom".into())))
        );
    }

    #[test]
    fn error_classification_checks_code_and_type() {
        assert_eq!(
            decode_data_line(
                r#"{"error":{"message":"slow down","code":"server_error","type":"rate_limit_exceeded"}}"#
            ),
            Some(SseItem::Failed(GlossError::EngineRateLimited))
        );
        assert_eq!(
            decode_data_line(
                r#"{"error":{"message":"bad key","code":"invalid_api_key","type":"invalid_request_error"}}"#
            ),
            Some(SseItem::Failed(GlossError::EngineAuth))
        );
    }
}
