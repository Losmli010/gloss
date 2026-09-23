//! AX 读选区：`AccessibilityReader`。
//!
//! 经 Accessibility API（HIServices）向系统问询「systemwide 焦点元素 →
//! 选中文本」，是向目标应用发起的同步跨进程调用，按线程模型约束
//! 必须运行在平台事件线程（调用方保证亲和性）。权限缺失返回
//! [`GlossError::AccessibilityDenied`]；无选区、应用不支持选区属性或系统
//! 调用失败返回 [`GlossError::SelectionUnavailable`]——一律经 `Result`
//! 传播，不允许 panic 逃出。授权引导属设置页/权限引导路径，读取侧只如实
//! 报告，不代为弹窗。
//!
//! 策略取最简一条路径：只读 `kAXSelectedTextAttribute`。

use gloss_core::model::GlossError;

/// AXError.h 中「辅助功能 API 未对当前进程启用」的返回码（kAXErrorAPIDisabled）。
/// 注意相邻码：-25212 是 kAXErrorNoValue（属性存在但无值，即「无选区」
/// 的常见返回），二者错一位语义就反转，见 tests 的相邻码对账测试。
const AX_ERROR_API_DISABLED: i32 = -25211;

/// 取值缓冲区的分配上界（字节）：长度来自远端进程报告，无校验的分配
/// 失败会直接 abort 进程而非返回错误，超限按取不到选区处理。
const MAX_SELECTION_BYTES: usize = 8 * 1024 * 1024;

/// 把「是否已授权 + AX 取值结果」映射为统一错误语义（纯逻辑，单测覆盖）：
/// 未授权一律 [`GlossError::AccessibilityDenied`]（权限检查先于取值，AX
/// 返回同一码时以更明确的权限语义收口）；其余取不到选区的情形——空选区、
/// 应用不支持属性、系统调用失败——归 [`GlossError::SelectionUnavailable`]。
fn interpret(trusted: bool, outcome: Result<Option<String>, i32>) -> Result<String, GlossError> {
    if !trusted {
        return Err(GlossError::AccessibilityDenied);
    }
    match outcome {
        Ok(Some(text)) if !text.is_empty() => Ok(text),
        Ok(_) => Err(GlossError::SelectionUnavailable),
        Err(AX_ERROR_API_DISABLED) => Err(GlossError::AccessibilityDenied),
        Err(_) => Err(GlossError::SelectionUnavailable),
    }
}

mod imp {
    use gloss_core::log::{debug, thread};
    use gloss_core::model::GlossError;

    use crate::ffi::ax::{self, AXUIElementRef};
    use crate::ffi::cf::{self, CFTypeRef, CfGuard};

    use super::{MAX_SELECTION_BYTES, interpret};

    /// AXError.h 的 kAXErrorFailure：无具体语义的失败兜底。
    const AX_ERROR_FAILURE: i32 = -25200;
    /// AXError.h 的 kAXErrorSuccess。
    const AX_ERROR_SUCCESS: i32 = 0;

    /// AX 读选区实现：无状态，可按需构造。
    #[derive(Debug, Default, Clone, Copy)]
    pub struct AccessibilityReader;

    impl AccessibilityReader {
        /// 创建读取器。
        pub fn new() -> Self {
            Self
        }

        /// 读取前台应用的选中文本。
        ///
        /// 调用方保证：在平台事件线程上调用。
        pub fn read(&mut self) -> Result<String, GlossError> {
            // 未授权时不发起跨进程取值，直接按权限语义收口。
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

    /// 读选区的 FFI 薄壳：systemwide → 焦点元素 → 选中文本。任何一步失败
    /// 都以 AX 原始错误码返回，语义映射交给 [`interpret`]。
    fn copy_selected_text() -> Result<Option<String>, i32> {
        // SAFETY: 无前置条件；返回的 +1 引用可能为 NULL（系统异常），随后判空。
        let system_wide = unsafe { ax::create_system_wide() };
        if system_wide.is_null() {
            return Err(AX_ERROR_FAILURE);
        }
        let _system_wide = CfGuard::new(system_wide as CFTypeRef);

        // 属性名按需构造、用毕即释放；分配失败视为本次读取失败。
        let Some(focused_attr) = cf::cf_string(ax::FOCUSED_UI_ELEMENT_ATTRIBUTE) else {
            return Err(AX_ERROR_FAILURE);
        };
        let Some(text_attr) = cf::cf_string(ax::SELECTED_TEXT_ATTRIBUTE) else {
            return Err(AX_ERROR_FAILURE);
        };

        // 出参在失败时不写入，成功时由守卫接管 +1 引用。
        let mut focused: CFTypeRef = std::ptr::null();
        // SAFETY: 元素与属性名均为有效 +1 引用，出参指向刚声明的栈上变量。
        let err =
            unsafe { ax::copy_attribute(system_wide, focused_attr.string_ref(), &mut focused) };
        if err != AX_ERROR_SUCCESS {
            return Err(err);
        }
        let _focused = CfGuard::new(focused);

        let mut value: CFTypeRef = std::ptr::null();
        // SAFETY: `focused` 是上一步成功拷贝的元素引用，其余不变量同上。
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

        // SAFETY: `value` 持有 kAXSelectedTextAttribute 拷贝出的有效 CF 对象，
        // string_from 内部先验类型再解引用。
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
    fn empty_and_missing_selection_are_unavailable() {
        assert_eq!(
            interpret(true, Ok(Some(String::new()))),
            Err(GlossError::SelectionUnavailable)
        );
        assert_eq!(
            interpret(true, Ok(None)),
            Err(GlossError::SelectionUnavailable)
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
            Err(GlossError::SelectionUnavailable),
            "kAXErrorNoValue (no selection) must not read as permission denied"
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
