//! 辅助功能（AX）元素读取接口，来自应用服务框架（ApplicationServices）的界面服务子框架（HIServices）。

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
    // SAFETY: a pure query FFI with no preconditions.
    let trusted = unsafe { AXIsProcessTrusted() };
    trusted != 0
}

pub(crate) fn is_process_trusted_prompting() -> bool {
    let Some(key) = cf_string(TRUSTED_CHECK_OPTION_PROMPT) else {
        return is_process_trusted();
    };
    let key_ref = key.string_ref() as CFTypeRef;
    // SAFETY: reading the address of an extern static — `kCFBooleanTrue` is a
    // constant pointer exported by the system framework, its address is stable
    // in itself, and taking the address involves no data race.
    let value_ref: CFTypeRef = unsafe { kCFBooleanTrue as CFTypeRef };
    // SAFETY: both the key and the value are valid CF references, and the type
    // callbacks take care of retaining and releasing.
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
    // SAFETY: `options` is a valid dictionary reference just constructed above.
    let trusted = unsafe { AXIsProcessTrustedWithOptions(options) };
    trusted != 0
}

pub(crate) unsafe fn create_system_wide() -> AXUIElementRef {
    // SAFETY: no preconditions.
    unsafe { AXUIElementCreateSystemWide() }
}

pub(crate) unsafe fn copy_attribute(
    element: AXUIElementRef,
    attribute: CFStringRef,
    out: *mut CFTypeRef,
) -> i32 {
    // SAFETY: the preconditions are upheld by the caller.
    unsafe { AXUIElementCopyAttributeValue(element, attribute, out) }
}

pub(crate) unsafe fn set_bool_attribute(element: AXUIElementRef, attribute: CFStringRef) -> i32 {
    // SAFETY: the preconditions are upheld by the caller; reading the address of
    // an extern static is the same pattern as `is_process_trusted_prompting` —
    // the constant pointer's address is stable in itself.
    unsafe { AXUIElementSetAttributeValue(element, attribute, kCFBooleanTrue as CFTypeRef) }
}

pub(crate) unsafe fn pid_of(element: AXUIElementRef, out: *mut i32) -> i32 {
    // SAFETY: the preconditions are upheld by the caller.
    unsafe { AXUIElementGetPid(element, out) }
}

pub(crate) unsafe fn set_messaging_timeout(element: AXUIElementRef, timeout: f32) -> i32 {
    // SAFETY: the preconditions are upheld by the caller.
    unsafe { AXUIElementSetMessagingTimeout(element, timeout) }
}
