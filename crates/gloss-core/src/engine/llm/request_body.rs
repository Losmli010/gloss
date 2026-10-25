//! 开放接口（OpenAI）兼容传输请求体的构造：域形态的消息（文本或
//! 多模态部件）在这里映成传输形状；图像部件的编码（Base64）与数据地址
//! 前缀只在请求体构造处编码（域类型不带内联数据）。

use crate::ports::EngineRequest;
use crate::prompt::{ChatMessage, ContentPart, MessageContent};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;

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

fn message_to_value(message: &ChatMessage) -> serde_json::Value {
    serde_json::json!({
        "role": message.role,
        "content": content_to_value(&message.content),
    })
}

fn content_to_value(content: &MessageContent) -> serde_json::Value {
    match content {
        MessageContent::Text(text) => serde_json::json!(text),
        MessageContent::Parts(parts) => {
            serde_json::Value::Array(parts.iter().map(part_to_value).collect())
        }
    }
}

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
