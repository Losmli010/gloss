//! macOS AX 读选区：`AccessibilityReader`。
//!
//! 经 Accessibility API（HIServices）向系统问询「systemwide 焦点元素 →
//! 选中文本」，是向目标应用发起的同步跨进程调用，按 08 §4.4 的线程模型
//! 必须运行在平台事件线程（调用方保证亲和性）。权限缺失返回
//! [`GlossError::AccessibilityDenied`]；无选区、应用不支持选区属性或系统
//! 调用失败返回 [`GlossError::SelectionUnavailable`]——一律经 `Result`
//! 传播，不允许 panic 逃出。授权引导属设置页/权限引导路径，读取侧只如实
//! 报告，不代为弹窗。
//!
//! 策略取最简一条路径：只读 `kAXSelectedTextAttribute`。部分应用（如个别
//! Electron/Chromium 场景）只暴露 parameterized 选区属性，暂不兜底——
//! 由 CompositeReader 的剪贴板兜底通道覆盖。
//!
//! 平台门控：AX 是 macOS 专属 API，公共类型仅在 macOS 提供（Windows 的
//! UIA 实现落在同目录时再补齐公共面）；「授权状态 + AX 结果 → 统一错误」
//! 的映射决策是纯逻辑，照常全平台单测（Linux CI 只跑单测，先例见
//! `events/mouse.rs` 的手势状态机）。

use gloss_core::model::GlossError;

/// AXError.h 中「辅助功能 API 未对当前进程启用」的返回码（kAXErrorAPIDisabled）。
const AX_ERROR_API_DISABLED: i32 = -25212;

/// 把「是否已授权 + AX 取值结果」映射为统一错误语义（纯逻辑，全平台单测）：
/// 未授权一律 [`GlossError::AccessibilityDenied`]（权限检查先于取值，AX
/// 返回同一码时以更明确的权限语义收口）；其余取不到选区的情形——空选区、
/// 应用不支持属性、系统调用失败——归 [`GlossError::SelectionUnavailable`]。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
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

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::{CStr, c_void};

    use core_foundation_sys::base::{
        Boolean, CFGetTypeID, CFIndex, CFRange, CFRelease, CFTypeRef, kCFAllocatorDefault,
    };
    use core_foundation_sys::string::{
        CFStringCreateWithCString, CFStringGetBytes, CFStringGetLength,
        CFStringGetMaximumSizeForEncoding, CFStringGetTypeID, CFStringRef, kCFStringEncodingUTF8,
    };

    use gloss_core::log::{debug, thread};
    use gloss_core::model::GlossError;

    use super::interpret;

    /// AXError.h 的 kAXErrorFailure：无具体语义的失败兜底。
    const AX_ERROR_FAILURE: i32 = -25200;
    /// AXError.h 的 kAXErrorSuccess。
    const AX_ERROR_SUCCESS: i32 = 0;

    /// HIServices 的 AX 元素句柄：CF 不透明类型，遵守 +1/-1 内存管理规则。
    type AXUIElementRef = *mut c_void;

    /// 「焦点元素」属性名的稳定字符串值（kAXFocusedUIElementAttribute 的
    /// 文档契约，与 System Events/AppleScript 所用同名）。
    const AX_FOCUSED_UI_ELEMENT: &CStr = c"AXFocusedUIElement";
    /// 「选中文本」属性名的稳定字符串值（kAXSelectedTextAttribute 同上）。
    const AX_SELECTED_TEXT: &CStr = c"AXSelectedText";

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        /// 当前进程是否已获辅助功能授权（TCC）；纯查询，无前置条件。
        fn AXIsProcessTrusted() -> Boolean;
        /// 创建 systemwide AX 元素，成功返回 +1 引用。
        fn AXUIElementCreateSystemWide() -> AXUIElementRef;
        /// 复制元素属性：成功时 `value` 收到 +1 引用，失败时不写入。
        fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> i32;
    }

    /// 按名构造属性名 CFString：现行系统不再导出 kAX* 数据符号，改用其
    /// 稳定字符串值。返回 +1 引用，交给 [`CfGuard`] 释放。
    ///
    /// # Safety
    ///
    /// 分配失败返回 NULL；非 NULL 引用必须恰好释放一次。
    unsafe fn create_attr_string(name: &CStr) -> Option<CFStringRef> {
        // SAFETY: `name` 是 NUL 结尾的有效 C 字符串，编码为受支持的 UTF-8；
        // 分配失败返回 NULL，由调用方判别。
        let s = unsafe {
            CFStringCreateWithCString(kCFAllocatorDefault, name.as_ptr(), kCFStringEncodingUTF8)
        };
        (!s.is_null()).then_some(s)
    }

    /// CF 对象守卫：出作用域即 CFRelease，杜绝错误路径上的手工释放遗漏。
    struct CfGuard(CFTypeRef);

    impl Drop for CfGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: `self.0` 来自 AXUIElementCreateSystemWide /
                // AXUIElementCopyAttributeValue 的 +1 引用（NULL 已判空），
                // 此处是唯一释放点，恰好归还一次。
                unsafe { CFRelease(self.0) };
            }
        }
    }

    /// macOS AX 读选区实现：无状态，可按需构造。
    #[derive(Debug, Default, Clone, Copy)]
    pub struct AccessibilityReader;

    impl AccessibilityReader {
        /// 创建读取器。
        pub fn new() -> Self {
            Self
        }

        /// 读取前台应用的选中文本。
        ///
        /// 调用方保证：在平台事件线程上调用（08 §4.4 亲和性）。
        pub fn read(&mut self) -> Result<String, GlossError> {
            // SAFETY: 纯查询型 FFI，无前置条件。未授权时不发起跨进程取值，
            // 直接按权限语义收口。
            let trusted = unsafe { AXIsProcessTrusted() } != 0;
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
        let system_wide = unsafe { AXUIElementCreateSystemWide() };
        if system_wide.is_null() {
            return Err(AX_ERROR_FAILURE);
        }
        let _system_wide = CfGuard(system_wide);

        // 属性名按需构造、用毕即释放；分配失败视为本次读取失败。
        // SAFETY: 入参是 NUL 结尾的字面量，满足 create_attr_string 的契约；
        // 返回的 +1 引用交由守卫恰好释放一次。
        let Some(focused_attr) = (unsafe { create_attr_string(AX_FOCUSED_UI_ELEMENT) }) else {
            return Err(AX_ERROR_FAILURE);
        };
        let _focused_attr = CfGuard(focused_attr as CFTypeRef);
        // SAFETY: 同上，+1 引用交由守卫释放。
        let Some(text_attr) = (unsafe { create_attr_string(AX_SELECTED_TEXT) }) else {
            return Err(AX_ERROR_FAILURE);
        };
        let _text_attr = CfGuard(text_attr as CFTypeRef);

        // 出参在失败时不写入，成功时由守卫接管 +1 引用。
        let mut focused: CFTypeRef = std::ptr::null();
        // SAFETY: 元素与属性名均为有效 +1 引用，出参指向刚声明的栈上变量。
        let err = unsafe { AXUIElementCopyAttributeValue(system_wide, focused_attr, &mut focused) };
        if err != AX_ERROR_SUCCESS {
            return Err(err);
        }
        let _focused = CfGuard(focused);

        let mut value: CFTypeRef = std::ptr::null();
        // SAFETY: `focused` 是上一步成功拷贝的元素引用，其余不变量同上。
        let err = unsafe {
            AXUIElementCopyAttributeValue(focused as AXUIElementRef, text_attr, &mut value)
        };
        if err != AX_ERROR_SUCCESS {
            return Err(err);
        }
        let _value = CfGuard(value);

        // SAFETY: `value` 持有 kAXSelectedTextAttribute 拷贝出的有效 CF 对象，
        // cfstring 内部先验类型再解引用。
        Ok(unsafe { cfstring(value) })
    }

    /// 把 CF 对象转成 Rust 字符串；非 CFString 或编码失败返回 `None`。
    ///
    /// # Safety
    ///
    /// `value` 必须是有效的 CF 对象引用（+1 引用仍在调用方持有，可为 NULL，
    /// 也允许非 CFString 类型——内部先验类型）。
    unsafe fn cfstring(value: CFTypeRef) -> Option<String> {
        let s = value as CFStringRef;
        if s.is_null() {
            return None;
        }
        // SAFETY: `s` 非空且为有效 CF 对象；GetTypeID 对任意 CF 对象安全，
        // 确认类型后才调用 CFString 专属 API。
        if unsafe { CFGetTypeID(s as CFTypeRef) } != unsafe { CFStringGetTypeID() } {
            return None;
        }
        // SAFETY: `s` 已确认为 CFString，长度查询无前置条件。
        let len = unsafe { CFStringGetLength(s) };
        if len == 0 {
            return Some(String::new());
        }
        // SAFETY: UTF-8 为受支持编码，返回非负的编码字节数上界。
        let max = unsafe { CFStringGetMaximumSizeForEncoding(len, kCFStringEncodingUTF8) };
        let Ok(max) = usize::try_from(max) else {
            return None;
        };
        let mut buf = vec![0u8; max];
        let mut used: CFIndex = 0;
        // SAFETY: `s` 是有效 CFString；缓冲区容量恰为上界 `max`，GetBytes 不
        // 越界写；lossByte=0 表示无法完整转换时返回值小于请求长度。
        let converted = unsafe {
            CFStringGetBytes(
                s,
                CFRange {
                    location: 0,
                    length: len,
                },
                kCFStringEncodingUTF8,
                0,
                0,
                buf.as_mut_ptr(),
                max as CFIndex,
                &mut used,
            )
        };
        if converted != len {
            return None;
        }
        let Ok(used) = usize::try_from(used) else {
            return None;
        };
        buf.truncate(used);
        String::from_utf8(buf).ok()
    }
}

#[cfg(target_os = "macos")]
pub use imp::AccessibilityReader;

#[cfg(target_os = "macos")]
#[cfg(test)]
mod live_tests {
    use super::imp::AccessibilityReader;

    /// 手动验收入口：在任意文本编辑器中选中文字后运行
    /// `cargo test -p gloss-platform -- --ignored --nocapture`，断言能读出
    /// 选中文本。CI 无图形会话与授权，不参与常规测试；未授权时本测试
    /// 应失败于权限错误而非 panic。
    #[test]
    #[ignore = "requires a live GUI session, an active selection and accessibility permission"]
    fn reads_live_selection_when_authorized() {
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
        // 未授权时无论取值结果如何都判权限缺失：权限语义优先于一切。
        assert_eq!(
            interpret(false, text("selected")),
            Err(GlossError::AccessibilityDenied)
        );
    }

    #[test]
    fn selected_text_passes_through_verbatim() {
        assert_eq!(interpret(true, text("hello")), Ok("hello".to_string()));
        // 原样透传，不做裁剪策略（空白的取舍归下游管线）。
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
        // 授权检查与取值之间权限可能被收回：以 AX 的 APIDisabled 码为准，
        // 依旧收口到权限语义而非笼统的不可用。
        assert_eq!(
            interpret(true, Err(AX_ERROR_API_DISABLED)),
            Err(GlossError::AccessibilityDenied)
        );
    }

    #[test]
    fn other_ax_errors_map_to_unavailable() {
        // kAXErrorAttributeUnsupported / kAXErrorNoValue / kAXErrorFailure。
        for code in [-25206, -25213, -25200] {
            assert_eq!(
                interpret(true, Err(code)),
                Err(GlossError::SelectionUnavailable),
                "code {code} should map to selection unavailable"
            );
        }
    }
}
