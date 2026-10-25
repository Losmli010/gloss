//! 辅助功能（AX）读选区：辅助功能读取器。
//!
//! 经辅助功能接口（HIServices）向系统问询“系统级焦点元素 →
//! 选中文本”，是向目标应用发起的同步跨进程调用，按线程模型约束
//! 必须运行在平台事件线程（调用方保证亲和性）。权限缺失返回
//! 权限拒绝错误；“属性可读但选区为空”（空串，含
//! “属性存在但无值”系统错误）返回空选区错误
//! ——组合通道先让渡补读，预算耗尽仍空与“读不到”一样落剪贴板兜底；
//! 应用不支持选区属性或系统调用失败返回读取不可用错误
//! ——一律以错误传播，不允许程序崩溃逃出。授权引导属设置页与
//! 权限引导路径，读取侧只如实报告，不代为弹窗。
//!
//! 策略取最简一条路径：只读选中文本属性。

use gloss_core::model::GlossError;

const AX_ERROR_API_DISABLED: i32 = -25211;

const AX_ERROR_NO_VALUE: i32 = -25212;

const MAX_SELECTION_BYTES: usize = 256 * 1024;

fn interpret(trusted: bool, outcome: Result<Option<String>, i32>) -> Result<String, GlossError> {
    if !trusted {
        return Err(GlossError::AccessibilityDenied);
    }
    match outcome {
        Ok(Some(text)) if !text.is_empty() => Ok(text),
        Ok(Some(_)) => Err(GlossError::SelectionEmpty),
        Ok(None) => Err(GlossError::SelectionUnavailable),
        Err(AX_ERROR_API_DISABLED) => Err(GlossError::AccessibilityDenied),
        Err(AX_ERROR_NO_VALUE) => Err(GlossError::SelectionEmpty),
        Err(_) => Err(GlossError::SelectionUnavailable),
    }
}

mod imp {
    use gloss_core::log::{debug, thread};
    use gloss_core::model::GlossError;

    use crate::ffi::ax::{self, AXUIElementRef};
    use crate::ffi::cf::{self, CFTypeRef, CfGuard};

    use super::{MAX_SELECTION_BYTES, interpret};

    const AX_ERROR_FAILURE: i32 = -25200;
    const AX_ERROR_SUCCESS: i32 = 0;

    #[derive(Debug, Default, Clone, Copy)]
    pub struct AccessibilityReader;

    const AX_MESSAGE_TIMEOUT_SECS: f32 = 0.3;

    impl AccessibilityReader {
        pub fn new() -> Self {
            Self
        }

        pub fn read(&mut self) -> Result<String, GlossError> {
            let trusted = ax::is_process_trusted();
            if !trusted {
                debug!(
                    thread = thread::EVENT,
                    "accessibility permission missing, selection read denied"
                );
                return Err(GlossError::AccessibilityDenied);
            }
            let outcome = copy_selected_text();
            match &outcome {
                Ok(Some(text)) => debug!(
                    thread = thread::EVENT,
                    chars = text.chars().count(),
                    "selected text acquired"
                ),
                Ok(None) => debug!(
                    thread = thread::EVENT,
                    "focused element exposes no selected text"
                ),
                Err(code) => debug!(
                    thread = thread::EVENT,
                    code = *code,
                    "ax copy attribute failed"
                ),
            }
            interpret(true, outcome)
        }
    }

    fn copy_selected_text() -> Result<Option<String>, i32> {
        // SAFETY: no preconditions; the returned +1 reference may be NULL (a
        // system anomaly) and is null-checked right after.
        let system_wide = unsafe { ax::create_system_wide() };
        if system_wide.is_null() {
            return Err(AX_ERROR_FAILURE);
        }
        let _system_wide = CfGuard::new(system_wide as CFTypeRef);
        // SAFETY: `system_wide` is a valid reference just created above.
        unsafe { ax::set_messaging_timeout(system_wide, AX_MESSAGE_TIMEOUT_SECS) };

        let Some(focused_attr) = cf::cf_string(ax::FOCUSED_UI_ELEMENT_ATTRIBUTE) else {
            return Err(AX_ERROR_FAILURE);
        };
        let Some(text_attr) = cf::cf_string(ax::SELECTED_TEXT_ATTRIBUTE) else {
            return Err(AX_ERROR_FAILURE);
        };

        let mut focused: CFTypeRef = std::ptr::null();
        // SAFETY: both the element and the attribute name are valid +1
        // references, and the out parameter points to the stack variable just
        // declared.
        let err =
            unsafe { ax::copy_attribute(system_wide, focused_attr.string_ref(), &mut focused) };
        if err != AX_ERROR_SUCCESS {
            return Err(err);
        }
        let _focused = CfGuard::new(focused);

        let mut value: CFTypeRef = std::ptr::null();
        // SAFETY: `focused` is the element reference successfully copied in the
        // previous step; the remaining invariants are the same as above.
        let err = unsafe {
            ax::copy_attribute(
                focused as AXUIElementRef,
                text_attr.string_ref(),
                &mut value,
            )
        };
        if err != AX_ERROR_SUCCESS {
            return Err(err);
        }
        let _value = CfGuard::new(value);

        // SAFETY: `value` holds a valid CF object copied out for
        // `kAXSelectedTextAttribute`, and `string_from` verifies the type before
        // dereferencing.
        Ok(unsafe { cf::string_from(value, MAX_SELECTION_BYTES) })
    }
}

pub use imp::AccessibilityReader;

#[cfg(test)]
mod live_tests {
    use super::imp::AccessibilityReader;

    #[test]
    #[ignore = "需授权真机：先把运行测试的终端 App 加入 系统设置→隐私与                安全性→辅助功能（未授权时由 live_test_support 快速失败）"]
    fn reads_live_selection_when_authorized() {
        crate::live_test_support::require_accessibility("ax_read_selection");
        let mut reader = AccessibilityReader::new();
        let text = reader
            .read()
            .expect("live selection should be readable when authorized");
        println!("selected: {text}");
        assert!(!text.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Result<Option<String>, i32> {
        Ok(Some(s.to_string()))
    }

    #[test]
    fn untrusted_maps_to_accessibility_denied() {
        assert_eq!(
            interpret(false, text("selected")),
            Err(GlossError::AccessibilityDenied)
        );
        assert_eq!(
            interpret(false, Err(-25200)),
            Err(GlossError::AccessibilityDenied)
        );
    }

    #[test]
    fn selected_text_passes_through_verbatim() {
        assert_eq!(interpret(true, text("hello")), Ok("hello".to_string()));
        assert_eq!(interpret(true, text("  ")), Ok("  ".to_string()));
    }

    #[test]
    fn an_empty_selection_is_empty_but_a_missing_value_stays_unavailable() {
        assert_eq!(
            interpret(true, Ok(Some(String::new()))),
            Err(GlossError::SelectionEmpty),
            "an empty string is an existing attribute reporting no selection"
        );
        assert_eq!(
            interpret(true, Ok(None)),
            Err(GlossError::SelectionUnavailable),
            "None means the value could not be delivered (over the size ceiling, \
             non-string): a fallback is still worth trying"
        );
    }

    #[test]
    fn api_disabled_after_trusted_check_maps_to_denied() {
        assert_eq!(
            interpret(true, Err(AX_ERROR_API_DISABLED)),
            Err(GlossError::AccessibilityDenied)
        );
    }

    #[test]
    fn adjacent_error_codes_are_told_apart() {
        assert_eq!(AX_ERROR_API_DISABLED, -25211);
        assert_eq!(
            interpret(true, Err(-25211)),
            Err(GlossError::AccessibilityDenied)
        );
        assert_eq!(
            interpret(true, Err(-25212)),
            Err(GlossError::SelectionEmpty),
            "kAXErrorNoValue (attribute exists, no value) must not read as permission denied \
             nor as an unsupported attribute"
        );
    }

    #[test]
    fn other_ax_errors_map_to_unavailable() {
        for code in [-25206, -25213, -25200] {
            assert_eq!(
                interpret(true, Err(code)),
                Err(GlossError::SelectionUnavailable),
                "code {code} should map to selection unavailable"
            );
        }
    }
}
