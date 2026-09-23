//! Accessibility（ApplicationServices 的 HIServices）：AX 元素读取接口。

use std::ffi::{CStr, c_void};

use super::cf::{Boolean, CFStringRef, CFTypeRef};

/// HIServices 的 AX 元素句柄：CF 不透明类型，遵守 +1/-1 内存管理规则。
pub(crate) type AXUIElementRef = *mut c_void;

/// 「焦点元素」属性名的稳定字符串值（kAXFocusedUIElementAttribute 的文档
/// 契约，与 System Events/AppleScript 所用同名）。
pub(crate) const FOCUSED_UI_ELEMENT_ATTRIBUTE: &CStr = c"AXFocusedUIElement";
/// 「选中文本」属性名的稳定字符串值（kAXSelectedTextAttribute 同上）。
pub(crate) const SELECTED_TEXT_ATTRIBUTE: &CStr = c"AXSelectedText";

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    /// 当前进程是否已获辅助功能授权（TCC）。
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

/// 当前进程是否已获辅助功能授权（TCC）；纯查询，无前置条件。
pub(crate) fn is_process_trusted() -> bool {
    // SAFETY: 纯查询型 FFI，无前置条件。
    let trusted = unsafe { AXIsProcessTrusted() };
    trusted != 0
}

/// 创建 systemwide AX 元素；系统异常时返回 NULL（调用方判空），否则是 +1 引用。
///
/// # Safety
///
/// 无前置条件；非 NULL 返回值必须恰好释放一次（交给 [`super::cf::CfGuard`]）。
pub(crate) unsafe fn create_system_wide() -> AXUIElementRef {
    // SAFETY: 无前置条件。
    unsafe { AXUIElementCreateSystemWide() }
}

/// 复制元素的一个属性值：成功时把 +1 引用写进 `out`，失败时不写入。
///
/// # Safety
///
/// `element` 必须是有效的 AX 元素引用，`attribute` 必须是有效的 CFString 引用；
/// `out` 必须指向可写内存；返回 0 时其中是必须恰好释放一次的 +1 引用。
pub(crate) unsafe fn copy_attribute(
    element: AXUIElementRef,
    attribute: CFStringRef,
    out: *mut CFTypeRef,
) -> i32 {
    // SAFETY: 前置条件由调用方保证。
    unsafe { AXUIElementCopyAttributeValue(element, attribute, out) }
}
