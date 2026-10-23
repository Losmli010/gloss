//! 通用类型：语言、屏幕坐标与全链路统一错误。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Lang {
    Zh,
    En,
    Ja,
    Ko,
    Fr,
    Other(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Locale {
    #[default]
    Zh,
    En,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenPoint {
    pub x: i32,
    pub y: i32,
}

impl ScreenPoint {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlossError {
    SelectionUnavailable,
    SelectionEmpty,
    AccessibilityDenied,
    ImageUnavailable,
    ImageTooLarge,
    UnsupportedModality,
    JsonModeUnsupported,
    EngineNetwork,
    EngineAuth,
    EngineRateLimited,
    EmptyCompletion,
    EngineResponse(String),
    Config(String),
}

impl std::fmt::Display for GlossError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SelectionUnavailable => write!(f, "selection unavailable"),
            Self::SelectionEmpty => write!(f, "selection is empty"),
            Self::AccessibilityDenied => write!(f, "accessibility permission denied"),
            Self::ImageUnavailable => write!(f, "pasteboard image unavailable"),
            Self::ImageTooLarge => write!(f, "pasteboard image too large"),
            Self::UnsupportedModality => write!(f, "model capability does not match task modality"),
            Self::JsonModeUnsupported => {
                write!(f, "model service does not support the JSON output mode")
            }
            Self::EngineNetwork => write!(f, "engine network error"),
            Self::EngineAuth => write!(f, "engine authentication failed"),
            Self::EngineRateLimited => write!(f, "engine rate limited"),
            Self::EmptyCompletion => write!(f, "engine returned an empty completion"),
            Self::EngineResponse(_) => write!(f, "engine response error"),
            Self::Config(_) => write!(f, "config error"),
        }
    }
}

impl std::error::Error for GlossError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_names_the_variant_without_the_payload() {
        assert_eq!(
            GlossError::SelectionUnavailable.to_string(),
            "selection unavailable"
        );
        assert_eq!(GlossError::SelectionEmpty.to_string(), "selection is empty");
        assert_eq!(
            GlossError::ImageUnavailable.to_string(),
            "pasteboard image unavailable"
        );
        assert_eq!(
            GlossError::JsonModeUnsupported.to_string(),
            "model service does not support the JSON output mode"
        );
        assert_eq!(
            GlossError::EmptyCompletion.to_string(),
            "engine returned an empty completion"
        );
        assert_eq!(
            GlossError::EngineResponse("bad json".into()).to_string(),
            "engine response error"
        );
        assert_eq!(
            GlossError::Config("missing key".into()).to_string(),
            "config error"
        );
    }

    #[test]
    fn error_display_never_carries_the_payload() {
        assert_eq!(
            GlossError::EngineResponse("sk-secret-remote-echo".into()).to_string(),
            "engine response error"
        );
        assert_eq!(
            GlossError::Config("sk-secret-remote-echo".into()).to_string(),
            "config error"
        );
    }

    #[test]
    fn error_is_std_error() {
        let err: Box<dyn std::error::Error> = Box::new(GlossError::EngineAuth);
        assert_eq!(err.to_string(), "engine authentication failed");
    }
}
