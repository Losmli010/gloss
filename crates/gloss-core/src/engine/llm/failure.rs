//! 传输失败与 `HTTP` 状态的分类映射：错误分类决定编排层的走向（可重试的
//! 网络/限流、终态的鉴权与参数拒绝），也决定设置页引导的方向；`4xx` 的
//! 启发式识别只认诊断文本，不转述响应体。

use crate::model::GlossError;
use reqwest::StatusCode;

pub(super) const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;

pub(super) fn map_transport_error(err: reqwest::Error) -> GlossError {
    if err.is_builder() {
        GlossError::EngineResponse(format!("request rejected: {err}"))
    } else if err.is_decode() {
        GlossError::EngineResponse(format!("decode response: {err}"))
    } else {
        GlossError::EngineNetwork
    }
}

const JSON_MODE_MARKERS: [&str; 2] = ["response_format", "json_object"];

const MULTIMODAL_MARKERS: [&str; 3] = ["image", "multimodal", "vision"];

pub(super) fn map_failure(status: StatusCode, body: &str) -> GlossError {
    match status.as_u16() {
        401 | 403 => GlossError::EngineAuth,
        429 => GlossError::EngineRateLimited,
        500..=599 => GlossError::EngineNetwork,
        _ => {
            let message = server_message(body);
            let lowered = message.as_deref().unwrap_or("").to_lowercase();
            if JSON_MODE_MARKERS
                .iter()
                .any(|marker| lowered.contains(marker))
            {
                return GlossError::JsonModeUnsupported;
            }
            if MULTIMODAL_MARKERS
                .iter()
                .any(|marker| lowered.contains(marker))
            {
                return GlossError::UnsupportedModality;
            }
            match message {
                Some(message) => GlossError::EngineResponse(format!("HTTP {status}: {message}")),
                None => GlossError::EngineResponse(format!("HTTP {status}")),
            }
        }
    }
}

fn server_message(body: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct ErrorEnvelope {
        error: ErrorBody,
    }
    #[derive(serde::Deserialize)]
    struct ErrorBody {
        message: String,
    }

    let envelope: ErrorEnvelope = serde_json::from_str(body).ok()?;
    let message = envelope.error.message;
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(200).collect())
}
