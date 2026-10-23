//! OpenAI 兼容 wire 请求体的构造：域形态的消息（[`ChatMessage`]：文本或
//! 多模态部件）在这里映成 wire 形状；图像部件的 base64 与 data URL 前缀
//! 只在请求体构造处编码（域类型不带 base64）。

use crate::ports::EngineRequest;
use crate::prompt::{ChatMessage, ContentPart, MessageContent};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;

/// 组装 OpenAI 兼容请求体（`chat/completions` 的形状）。`max_tokens` 只在
/// 设限时携带：部分严格端点会拒绝显式的 `null`，缺键才是「不设限」的 wire
/// 语义。
///
/// `response_format` json_object **恒携带**（[`EngineRequest`] 不加开关）：
/// 所有请求都是 JSON 契约，不做能力协商——端点不认时以 4xx 拒绝，由传输层
/// 的失败映射识别成 JSON 模式不支持（`GlossError::JsonModeUnsupported`）。
pub(super) fn chat_request_body(request: &EngineRequest) -> serde_json::Value {
    let messages: Vec<serde_json::Value> = request.messages.iter().map(message_to_value).collect();
    let mut body = serde_json::json!({
        "model": request.model,
        "messages": messages,
        "stream": true,
        "response_format": { "type": "json_object" },
    });
    if let Some(max_tokens) = request.max_tokens {
        body["max_tokens"] = serde_json::json!(max_tokens);
    }
    body
}

/// 单条消息 → OpenAI 兼容 JSON：role 用小写契约名（`Role` 的 serde 拼写），
/// 正文按域形态分派（纯文本即 JSON 字符串，部件即 content 数组）。
fn message_to_value(message: &ChatMessage) -> serde_json::Value {
    serde_json::json!({
        "role": message.role,
        "content": content_to_value(&message.content),
    })
}

/// 消息正文 → wire 值：纯文本即字符串，部件数组即 OpenAI 兼容 content
/// 数组（`{"type":"image_url",…}`）。
fn content_to_value(content: &MessageContent) -> serde_json::Value {
    match content {
        MessageContent::Text(text) => serde_json::json!(text),
        MessageContent::Parts(parts) => {
            serde_json::Value::Array(parts.iter().map(part_to_value).collect())
        }
    }
}

/// 单个部件 → wire 值。图像字节在这里才做 base64 编码并拼 data URL 前缀
/// （`data:image/png;base64,…`）——域类型只携带原始字节，编码不出请求体
/// 构造这一步。
fn part_to_value(part: &ContentPart) -> serde_json::Value {
    match part {
        ContentPart::ImagePng(png) => serde_json::json!({
            "type": "image_url",
            "image_url": {
                "url": format!("data:image/png;base64,{}", BASE64_STANDARD.encode(png)),
            },
        }),
    }
}
