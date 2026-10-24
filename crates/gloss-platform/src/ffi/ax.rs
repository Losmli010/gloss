//! 辅助功能（ApplicationServices 的 HIServices）：AX 元素读取接口。

use std::ffi::{CStr, c_void};

use super::cf::{
    Boolean, CFDictionaryCreate, CFDictionaryRef, CFStringRef, CFTypeRef, CfGuard, cf_string,
    kCFAllocatorDefault, kCFBooleanTrue, kCFTypeDictionaryKeyCallBacks,
    kCFTypeDictionaryValueCallBacks,
};

pub(crate) type AXUIElementRef = *mut c_void;

pub(crate) const FOCUSED_UI_ELEMENT_ATTRIBUTE: &CStr = c"AXFocusedUIElement";
pub(crate) const FOCUSED_APPLICATION_ATTRIBUTE: &CStr = c"AXFocusedApplication";
pub(crate) const SELECTED_TEXT_ATTRIBUTE: &CStr = c"AXSelectedText";
pub(crate) const MANUAL_ACCESSIBILITY_ATTRIBUTE: &CStr = c"AXManualAccessibility";
pub(crate) const ENHANCED_USER_INTERFACE_ATTRIBUTE: &CStr = c"AXEnhancedUserInterface";
pub(crate) const TRUSTED_CHECK_OPTION_PROMPT: &CStr = c"AXTrustedCheckOptionPrompt";

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> Boolean;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> Boolean;
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> i32;
    fn AXUIElementGetPid(element: AXUIElementRef, pid: *mut i32) -> i32;
    fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, timeout: f32) -> i32;
}

pub(crate) fn is_process_trusted() -> bool {
    // SAFETY: 纯查询型 FFI，无前置条件。
    let trusted = unsafe { AXIsProcessTrusted() };
    trusted != 0
}

pub(crate) fn is_process_trusted_prompting() -> bool {
    let Some(key) = cf_string(TRUSTED_CHECK_OPTION_PROMPT) else {
        return is_process_trusted();
    };
    let key_ref = key.string_ref() as CFTypeRef;
    // SAFETY: 读 extern static 的地址值——kCFBooleanTrue 是系统框架导出的
    // 常量指针，地址本身稳定，取地址不存在数据竞争。
    let value_ref: CFTypeRef = unsafe { kCFBooleanTrue as CFTypeRef };
    // SAFETY: 键值均为有效 CF 引用，类型回调负责 retain/release。
    let options = unsafe {
        CFDictionaryCreate(
            kCFAllocatorDefault,
            &key_ref,
            &value_ref,
            1,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        )
    };
    if options.is_null() {
        return is_process_trusted();
    }
    let _options = CfGuard::new(options as CFTypeRef);
    // SAFETY: `options` 是刚构造的有效字典引用。
    let trusted = unsafe { AXIsProcessTrustedWithOptions(options) };
    trusted != 0
}

pub(crate) unsafe fn create_system_wide() -> AXUIElementRef {
    // SAFETY: 无前置条件。
    unsafe { AXUIElementCreateSystemWide() }
}

pub(crate) unsafe fn copy_attribute(
    element: AXUIElementRef,
    attribute: CFStringRef,
    out: *mut CFTypeRef,
) -> i32 {
    // SAFETY: 前置条件由调用方保证。
    unsafe { AXUIElementCopyAttributeValue(element, attribute, out) }
}

pub(crate) unsafe fn set_bool_attribute(element: AXUIElementRef, attribute: CFStringRef) -> i32 {
    // SAFETY: 前置条件由调用方保证；读 extern static 的地址值与
    // is_process_trusted_prompting 同款——常量指针地址本身稳定。
    unsafe { AXUIElementSetAttributeValue(element, attribute, kCFBooleanTrue as CFTypeRef) }
}

pub(crate) unsafe fn pid_of(element: AXUIElementRef, out: *mut i32) -> i32 {
    // SAFETY: 前置条件由调用方保证。
    unsafe { AXUIElementGetPid(element, out) }
}

pub(crate) unsafe fn set_messaging_timeout(element: AXUIElementRef, timeout: f32) -> i32 {
    // SAFETY: 前置条件由调用方保证。
    unsafe { AXUIElementSetMessagingTimeout(element, timeout) }
}
