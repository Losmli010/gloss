//! CoreGraphics 事件注入：模拟复制（Cmd+C）与真机测试的合成事件。
//!
//! 与监听（[`super::eventtap`]）分居两个模块：这里只「造事件、发事件」。
//! 事件经 kCGHIDEventTap 注入系统事件链，对目标应用等价于真实键鼠输入；
//! 注入需要辅助功能授权，未授权时事件被系统静默丢弃（`CGEventPost` 无
//! 返回状态）——成败由调用方的写入确认窗口分辨，不在这里猜。

use std::ffi::c_void;

use super::cf::{CFTypeRef, CfGuard};
#[cfg(test)]
use super::eventtap::{CGEventGetLocation, CGPoint};
use super::eventtap::{CGEventRef, K_CG_HID_EVENT_TAP};

/// HID 虚拟键码（HIToolbox 的 Events.h）：C 键。
pub(crate) const VK_ANSI_C: u16 = 8;
/// HID 虚拟键码：Command 修饰键。
pub(crate) const VK_COMMAND: u16 = 55;

/// CGEventType 的 kCGEventLeftMouseDown（真机测试注入用）。
#[cfg(test)]
pub(crate) const CG_EVENT_LEFT_MOUSE_DOWN: u32 = 1;
/// CGEventType 的 kCGEventLeftMouseUp（真机测试注入用）。
#[cfg(test)]
pub(crate) const CG_EVENT_LEFT_MOUSE_UP: u32 = 2;
/// CGEventType 的 kCGEventMouseMoved（真机测试注入用）。
#[cfg(test)]
pub(crate) const CG_EVENT_MOUSE_MOVED: u32 = 5;
/// CGMouseButton 的 kCGMouseButtonLeft。
#[cfg(test)]
const CG_MOUSE_BUTTON_LEFT: u32 = 0;
/// CGEventSourceStateID 的 kCGEventSourceStateHIDSystemState。
const CG_EVENT_SOURCE_HID_SYSTEM_STATE: i32 = 1;

/// HID 系统状态的事件源引用（+1 引用，由守卫释放）。
type CGEventSourceRef = *mut c_void;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    /// 建立事件源：成功返回 +1 引用，失败返回 NULL。
    fn CGEventSourceCreate(state_id: i32) -> CGEventSourceRef;
    /// 构造键盘事件：`key_down` 决定按下/释放；成功返回 +1 引用，失败 NULL。
    fn CGEventCreateKeyboardEvent(
        source: CGEventSourceRef,
        virtual_key: u16,
        key_down: bool,
    ) -> CGEventRef;
    /// 把合成事件注入 `tap` 层，无返回状态。
    fn CGEventPost(tap: u32, event: CGEventRef);
}

// 真机测试注入专用的鼠标事件构造：生产路径只注入键盘（模拟复制）。
#[cfg(test)]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    /// 构造鼠标事件：`kind` 为 CGEventType 常量；成功返回 +1 引用，失败 NULL。
    fn CGEventCreateMouseEvent(
        source: CGEventSourceRef,
        kind: u32,
        position: CGPoint,
        button: u32,
    ) -> CGEventRef;
    /// 构造空事件：仅用于查询当前位置；成功返回 +1 引用，失败 NULL。
    fn CGEventCreate(source: CGEventSourceRef) -> CGEventRef;
}

/// 建立 HID 系统状态的事件源（合成事件的来源标注）；失败返回 `None`。
pub(crate) fn new_hid_source() -> Option<CfGuard> {
    // SAFETY: 纯构造型 FFI，无前置条件；失败返回 NULL，此处按失败处理。
    let source = unsafe { CGEventSourceCreate(CG_EVENT_SOURCE_HID_SYSTEM_STATE) };
    (!source.is_null()).then(|| CfGuard::new(source as CFTypeRef))
}

/// 构造一个键盘事件（`key_down` 区分按下/释放）并注入 HID tap：事件
/// 构造失败（分配）返回 `false`，注入本身无返回状态。
pub(crate) fn post_keyboard_event(source: &CfGuard, virtual_key: u16, key_down: bool) -> bool {
    // SAFETY: `source` 是有效事件源引用；成功返回的 +1 事件交给守卫，
    // 注入发生在守卫存活期内。
    let event = unsafe {
        CGEventCreateKeyboardEvent(source.raw() as CGEventSourceRef, virtual_key, key_down)
    };
    if event.is_null() {
        return false;
    }
    let _event = CfGuard::new(event as CFTypeRef);
    // SAFETY: `event` 是刚构造的有效事件引用，CGEventPost 不接管所有权。
    unsafe { CGEventPost(K_CG_HID_EVENT_TAP, event) };
    true
}

/// 构造一个鼠标事件（`kind` 为 CGEventType 常量）并注入 HID tap：事件
/// 构造失败（分配）返回 `false`，注入本身无返回状态。
#[cfg(test)]
pub(crate) fn post_mouse_event(source: &CfGuard, kind: u32, position: CGPoint) -> bool {
    // SAFETY: 不变量同 post_keyboard_event——source 有效、+1 事件交守卫。
    let event = unsafe {
        CGEventCreateMouseEvent(
            source.raw() as CGEventSourceRef,
            kind,
            position,
            CG_MOUSE_BUTTON_LEFT,
        )
    };
    if event.is_null() {
        return false;
    }
    let _event = CfGuard::new(event as CFTypeRef);
    // SAFETY: `event` 是刚构造的有效事件引用，CGEventPost 不接管所有权。
    unsafe { CGEventPost(K_CG_HID_EVENT_TAP, event) };
    true
}

/// 当前指针位置（全局坐标，原点在左上）；临时事件构造失败返回 `None`。
#[cfg(test)]
pub(crate) fn current_position(source: &CfGuard) -> Option<CGPoint> {
    // SAFETY: `source` 是有效事件源引用；+1 事件交给守卫，位置查询在
    // 存活期内。
    let event = unsafe { CGEventCreate(source.raw() as CGEventSourceRef) };
    if event.is_null() {
        return None;
    }
    let _event = CfGuard::new(event as CFTypeRef);
    // SAFETY: `event` 是有效事件引用，CGEventGetLocation 为只读查询。
    Some(unsafe { CGEventGetLocation(event) })
}
