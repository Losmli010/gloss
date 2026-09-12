//! 通用类型：语言、屏幕坐标与全链路统一错误。

/// 任务的语言参数；UI 固定常用 5 语种，「自动检测」由 `Option<Lang>` 留空表达。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lang {
    /// 简体中文。
    Zh,
    /// 英语。
    En,
    /// 日语。
    Ja,
    /// 韩语。
    Ko,
    /// 法语。
    Fr,
    /// 以上之外的语言，携带语言代码或名称原文。
    Other(String),
}

/// 屏幕逻辑坐标矩形。多显示器下原点可为负，坐标按手势/系统返回值原样传递。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenRect {
    /// 左上角横坐标（逻辑像素）。
    pub x: i32,
    /// 左上角纵坐标（逻辑像素）。
    pub y: i32,
    /// 宽（逻辑像素）。
    pub width: u32,
    /// 高（逻辑像素）。
    pub height: u32,
}

/// 全链路统一错误：状态机 Error 态与重试策略都按变体分支，不允许 panic 逃出主循环。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlossError {
    /// 选区读不到（权限缺失或空选区）。
    SelectionUnavailable,
    /// macOS 辅助功能权限缺失。
    AccessibilityDenied,
    /// macOS 屏幕录制权限缺失 / Windows 抓屏失败。
    ScreenCaptureDenied,
    /// 框选区域超出屏幕或阈值。
    RegionTooLarge,
    /// 任务所需模态与配置的模型能力不匹配（如图像任务未配视觉模型）。
    UnsupportedModality,
    /// 网络错误，可重试。
    EngineNetwork,
    /// API key 无效或过期。
    EngineAuth,
    /// 触发限流，可退避重试。
    EngineRateLimited,
    /// 协议或响应解析异常，携带诊断文本。
    EngineResponse(String),
    /// 配置缺失或非法。
    Config(String),
}

impl std::fmt::Display for GlossError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SelectionUnavailable => write!(f, "selection unavailable"),
            Self::AccessibilityDenied => write!(f, "accessibility permission denied"),
            Self::ScreenCaptureDenied => write!(f, "screen capture permission denied"),
            Self::RegionTooLarge => write!(f, "screen region too large"),
            Self::UnsupportedModality => write!(f, "model capability does not match task modality"),
            Self::EngineNetwork => write!(f, "engine network error"),
            Self::EngineAuth => write!(f, "engine authentication failed"),
            Self::EngineRateLimited => write!(f, "engine rate limited"),
            Self::EngineResponse(s) => write!(f, "engine response error: {s}"),
            Self::Config(s) => write!(f, "config error: {s}"),
        }
    }
}

impl std::error::Error for GlossError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_rect_compares_by_value() {
        let rect = ScreenRect {
            x: -1920,
            y: 0,
            width: 800,
            height: 600,
        };
        assert_eq!(
            rect,
            ScreenRect {
                x: -1920,
                y: 0,
                width: 800,
                height: 600
            }
        );
        assert_ne!(
            rect,
            ScreenRect {
                x: 0,
                y: 0,
                width: 800,
                height: 600
            }
        );
    }

    #[test]
    fn error_display_is_diagnostic_text() {
        assert_eq!(
            GlossError::SelectionUnavailable.to_string(),
            "selection unavailable"
        );
        assert_eq!(
            GlossError::EngineResponse("bad json".into()).to_string(),
            "engine response error: bad json"
        );
        assert_eq!(
            GlossError::Config("missing key".into()).to_string(),
            "config error: missing key"
        );
    }

    #[test]
    fn error_is_std_error() {
        let err: Box<dyn std::error::Error> = Box::new(GlossError::EngineAuth);
        assert_eq!(err.to_string(), "engine authentication failed");
    }
}
