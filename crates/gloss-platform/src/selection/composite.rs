//! 选区读取的组合通道：AX 优先，读不到时降级剪贴板兜底。
//!
//! 降级边界：AX 报权限缺失时不兜底——兜底依赖按键注入，未授权时注入会被
//! 系统静默忽略，白等超时只会拖慢失败路径；此时把权限语义原样上抛，由
//! 上层做权限引导。Windows 暂无 AX/UIA 实现，兜底即主通道。

use gloss_core::model::GlossError;

/// 组合判定（纯逻辑，全平台单测）：AX 成功直接采纳；权限缺失原样上抛且
/// 不评估兜底（见模块注释）；其余读不到的情形才落到兜底结果。`fallback`
/// 是惰性求值——兜底路径含按键注入，未走到就不该有副作用。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn combine(
    ax: Result<String, GlossError>,
    fallback: impl FnOnce() -> Result<String, GlossError>,
) -> Result<String, GlossError> {
    match ax {
        Ok(text) => Ok(text),
        Err(GlossError::AccessibilityDenied) => Err(GlossError::AccessibilityDenied),
        Err(_) => fallback(),
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod imp {
    use gloss_core::model::GlossError;

    #[cfg(target_os = "macos")]
    use super::combine;
    #[cfg(target_os = "macos")]
    use crate::selection::accessibility::AccessibilityReader;
    use crate::selection::clipboard::ClipboardFallbackReader;

    /// 选区读取的组合实现：macOS 上 AX 优先、读不到时降级剪贴板兜底；
    /// Windows 上兜底即主通道。
    #[derive(Debug, Default)]
    pub struct CompositeReader {
        #[cfg(target_os = "macos")]
        ax: AccessibilityReader,
        clipboard: ClipboardFallbackReader,
    }

    impl CompositeReader {
        /// 创建组合读取器。
        pub fn new() -> Self {
            Self::default()
        }

        /// 读取前台应用的选中文本。
        ///
        /// 调用方保证：在平台事件线程上调用（08 §4.4 亲和性）。
        pub fn read(&mut self) -> Result<String, GlossError> {
            #[cfg(target_os = "macos")]
            {
                let ax = self.ax.read();
                combine(ax, || self.clipboard.read())
            }
            #[cfg(not(target_os = "macos"))]
            self.clipboard.read()
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use imp::CompositeReader;

#[cfg(test)]
mod tests {
    use gloss_core::model::GlossError;

    use super::combine;

    fn ax_ok(text: &str) -> Result<String, GlossError> {
        Ok(text.to_owned())
    }

    fn clipboard_ok(text: &str) -> Result<String, GlossError> {
        Ok(text.to_owned())
    }

    /// AX 成功：直接采纳，兜底不求值（兜底路径含按键注入，不能有副作用）。
    #[test]
    fn fallback_is_lazy_on_ax_success() {
        let mut clipboard_called = false;
        let outcome = combine(ax_ok("from ax"), || {
            clipboard_called = true;
            Ok::<String, GlossError>("from clipboard".into())
        });
        assert_eq!(outcome, Ok("from ax".into()));
        assert!(!clipboard_called, "ax success must not touch the clipboard");
    }

    /// 权限缺失：原样上抛且不兜底（未授权时注入必被忽略，白等超时）。
    #[test]
    fn permission_denied_skips_fallback() {
        let mut clipboard_called = false;
        let outcome = combine(Err(GlossError::AccessibilityDenied), || {
            clipboard_called = true;
            Ok::<String, GlossError>("from clipboard".into())
        });
        assert_eq!(outcome, Err(GlossError::AccessibilityDenied));
        assert!(
            !clipboard_called,
            "permission denied must not fall back to the clipboard"
        );
    }

    /// AX 读不到：落到兜底结果；兜底也失败时以兜底的错误收口。
    #[test]
    fn unavailable_ax_falls_back_to_clipboard() {
        let ax = || Err::<String, GlossError>(GlossError::SelectionUnavailable);
        assert_eq!(
            combine(ax(), || clipboard_ok("from clipboard")),
            Ok("from clipboard".into())
        );
        assert_eq!(
            combine(ax(), || Err(GlossError::SelectionUnavailable)),
            Err(GlossError::SelectionUnavailable)
        );
    }
}
