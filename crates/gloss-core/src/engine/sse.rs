//! 流式事件（SSE）增量解码，服务端为开放接口（OpenAI）兼容的对话补全流式响应。
//!
//! 纯逻辑、不碰网络：喂字节、吐增量。因此单测可以把同一段响应按任意字节边界
//! 切开，钉住“跨块不丢不重、多字节字符不被切坏”这类只有分块才会暴露的问题。
//!
//! 只实现用得到的流式事件（SSE）子集，不做通用实现：
//! - 数据行一行一个事件；约定的完成哨兵行表示流结束；
//! - 注释与心跳行、事件名、编号等字段一律忽略；
//! - 正文增量字段是文本增量；只带角色信息的首块没有正文，
//!   不产出增量；
//! - 结束原因字段随块捕获，在完成哨兵上随完成事件
//!   交出（服务端没给完成哨兵就关流的，结束原因未知，不产出完成事件）；
//! - 服务端在流中报错时给错误对象，按错误码映射到领域错误。

use crate::model::GlossError;

const MAX_DIAGNOSTIC_CHARS: usize = 200;

const MAX_LINE_BYTES: usize = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseItem {
    Delta(String),
    Done { finish_reason: Option<String> },
    Failed(GlossError),
}

#[derive(Debug, Default)]
pub struct SseDecoder {
    pending: Vec<u8>,
    done: bool,
    finish_reason: Option<String>,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, bytes: &[u8]) -> Vec<SseItem> {
        if self.done {
            return Vec::new();
        }
        self.pending.extend_from_slice(bytes);
        let mut items = Vec::new();
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
        if !self.done && self.pending.len() > MAX_LINE_BYTES {
            self.pending.clear();
            self.done = true;
            items.push(SseItem::Failed(GlossError::EngineResponse(
                "sse line exceeds limit".into(),
            )));
        }
        items
    }

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

fn map_api_error(error: &ApiError) -> GlossError {
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
    match codes.first() {
        Some(code) => GlossError::EngineResponse(format!("{code}: {message}")),
        None => GlossError::EngineResponse(message),
    }
}

fn truncate(text: &str) -> String {
    if text.chars().count() <= MAX_DIAGNOSTIC_CHARS {
        return text.to_owned();
    }
    let head: String = text.chars().take(MAX_DIAGNOSTIC_CHARS).collect();
    format!("{head}…") // i18n:allow 诊断截断的省略号，非 locale 文案
}

#[derive(serde::Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
    #[serde(default)]
    error: Option<ApiError>,
}

#[derive(serde::Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: Option<StreamDelta>,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(serde::Deserialize)]
struct StreamDelta {
    #[serde(default)]
    content: Option<String>,
}

#[derive(serde::Deserialize)]
struct ApiError {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    code: Option<String>,
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
