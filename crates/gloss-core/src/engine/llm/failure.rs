//! 传输失败与 HTTP 状态的分类映射：错误分类决定编排层的走向（可重试的
//! 网络/限流、终态的鉴权与参数拒绝），也决定设置页引导的方向；4xx 的
//! 启发式识别只认诊断文本，不转述响应体。

use crate::model::GlossError;
use reqwest::StatusCode;

/// 错误响应体的读取上限：只要 `error.message`，多余字节没有价值，而错误
/// 网关的劫持页可能极大（截断后 JSON 解析失败会退化成「只报状态码」，
/// 与既有非 JSON 路径一致）。
pub(super) const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;

/// 传输层错误 → [`GlossError`]：连不上/超时/连接中断都可重试。
pub(super) fn map_transport_error(err: reqwest::Error) -> GlossError {
    // builder 错是本地构造失败（非法 header 等）——重试永远不会成功，报成
    // EngineNetwork（可重试）会把用户引到错误的方向。
    if err.is_builder() {
        GlossError::EngineResponse(format!("request rejected: {err}"))
    } else if err.is_decode() {
        GlossError::EngineResponse(format!("decode response: {err}"))
    } else {
        GlossError::EngineNetwork
    }
}

/// JSON 模式不支持的诊断标记（小写比较）：请求体恒带 `response_format`
/// json_object，4xx 诊断文本提到它就是端点不认这个开关。
const JSON_MODE_MARKERS: [&str; 2] = ["response_format", "json_object"];

/// 图像输入不支持的诊断标记（小写比较）。
const MULTIMODAL_MARKERS: [&str; 3] = ["image", "multimodal", "vision"];

/// HTTP 状态 → [`GlossError`]（错误分类表）。其余 4xx 按服务端诊断文本做
/// 两个**启发式**识别（环境问题，重发同请求无益，不落可重试的协议错误）：
/// 诊断提到 JSON 模式开关 → [`GlossError::JsonModeUnsupported`]；提到图像
/// 输入 → [`GlossError::UnsupportedModality`]。JSON 模式检查在前：它是请求
/// 信封层的拒绝，命中时对所有任务都成立。识别不出才落
/// [`GlossError::EngineResponse`]。
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

/// 从错误响应体里取服务端诊断文本（仅 `error.message`，OpenAI 兼容形状）。
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
    // 上限与 SSE 诊断一致：错误文本会进 UI 与日志。
    Some(trimmed.chars().take(200).collect())
}
