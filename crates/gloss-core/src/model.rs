//! 通用类型：语言、屏幕坐标与全链路统一错误。

use serde::{Deserialize, Serialize};

/// 任务的**产物**语言（译文给谁看）：变体名即 ISO 639-1 语言码，`Zh` 指
/// 简体；`Other` 承接手填语言名，原样当显示名用（不归一化，可能带空白）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Lang {
    Zh,
    En,
    Ja,
    Ko,
    Fr,
    /// 其余语言，携带语言名原文。
    Other(String),
}

/// 应用自身的**界面**语言：prompt 模板与界面文案的选表依据。
///
/// 与 [`Lang`] 分工不同——`Lang` 决定译文语言，`Locale` 决定应用跟用户说什么
/// 话，两者解耦。`Config::language` 的 `System` 在壳侧按系统语言落定后只剩
/// 下面两个具体值，所以这里没有第三态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Locale {
    /// 出厂默认。
    #[default]
    Zh,
    En,
}

/// 屏幕逻辑坐标点。多显示器下坐标可为负，按手势/系统返回值原样传递。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenPoint {
    /// 逻辑像素横坐标。
    pub x: i32,
    /// 逻辑像素纵坐标。
    pub y: i32,
}

impl ScreenPoint {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// 全链路统一错误：状态机 Error 态与重试策略都按变体分支，不允许 panic 逃出主循环。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlossError {
    /// 选区读不到（应用不支持选区属性或 AX/兜底通道失败）。
    SelectionUnavailable,
    /// 选区为空（应用支持选区属性、如实报告了空值）。与「读不到」分开建档：
    /// 状态机对手势触发静默收回。
    SelectionEmpty,
    /// 辅助功能权限缺失。
    AccessibilityDenied,
    /// 剪贴板图片读不出（板上无可用 flavor、粘贴授权被拒、粘贴板不可观察）
    /// 或解不了码。与 [`GlossError::SelectionUnavailable`] 分开建档：图像任务
    /// 没有可选的文本，失败指引是「重新复制」。
    ImageUnavailable,
    /// 剪贴板图片超出字节总量或像素面积上限（超限报错，不截断）。
    ImageTooLarge,
    /// 任务所需模态与配置的模型能力不匹配（如图像任务未配视觉模型），
    /// 或任务类型与输入模态的组合非法（见 [`crate::task::validate_modality`]）。
    UnsupportedModality,
    /// 模型服务不支持 JSON 输出模式：请求恒带 `response_format` json_object
    /// （不做能力协商），端点以 4xx 拒绝且诊断文本提到它时按此建档——换支持
    /// 该模式的模型服务是唯一出路，重试无益。
    JsonModeUnsupported,
    /// 网络错误，可重试。
    EngineNetwork,
    /// API key 无效或过期，不可重试。
    EngineAuth,
    /// 触发限流，可退避重试。
    EngineRateLimited,
    /// 协议或响应解析异常，携带诊断文本（可能含服务端/用户原文——见
    /// [`Display`](std::fmt::Display) 实现的安全注释：载荷不进日志）。
    EngineResponse(String),
    /// 配置异常，携带诊断文本（同 [`GlossError::EngineResponse`]，载荷不
    /// 进日志）。
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
            // 载荷可能携带远端或用户原文（服务端 `error.message`、解析不出
            // 的协议内容），而 `Display` 是日志出口（`error = %error` 遍布各
            // crate），把载荷打出来就是把敏感文本写进日志——违反 AGENTS.md
            // 「敏感文本不写进日志」的安全红线。载荷只在 `Debug`（`?error`）
            // 与失败卡（`i18n::for_error` 显式解构变体，不走 Display）使用；
            // `Debug` 同样不得进日志——日志宏禁止用 `?error` 打印 GlossError，
            // 它只留给诊断与测试断言。
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
