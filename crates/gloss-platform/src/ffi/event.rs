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

pub(crate) const VK_ANSI_C: u16 = 8;
pub(crate) const VK_COMMAND: u16 = 55;
pub(crate) const K_CG_EVENT_FLAG_MASK_COMMAND: u64 = 1 << 20;

#[cfg(test)]
pub(crate) const CG_EVENT_LEFT_MOUSE_DOWN: u32 = 1;
#[cfg(test)]
pub(crate) const CG_EVENT_LEFT_MOUSE_UP: u32 = 2;
#[cfg(test)]
pub(crate) const CG_EVENT_MOUSE_MOVED: u32 = 5;
#[cfg(test)]
const CG_MOUSE_BUTTON_LEFT: u32 = 0;
const CG_EVENT_SOURCE_HID_SYSTEM_STATE: i32 = 1;

type CGEventSourceRef = *mut c_void;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceCreate(state_id: i32) -> CGEventSourceRef;
    fn CGEventCreateKeyboardEvent(
        source: CGEventSourceRef,
        virtual_key: u16,
        key_down: bool,
    ) -> CGEventRef;
    fn CGEventSetFlags(event: CGEventRef, flags: u64);
    fn CGEventPost(tap: u32, event: CGEventRef);
}

#[cfg(test)]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventCreateMouseEvent(
        source: CGEventSourceRef,
        kind: u32,
        position: CGPoint,
        button: u32,
    ) -> CGEventRef;
    fn CGEventCreate(source: CGEventSourceRef) -> CGEventRef;
}

pub(crate) fn new_hid_source() -> Option<CfGuard> {
    // SAFETY: 纯构造型 FFI，无前置条件；失败返回 NULL，此处按失败处理。
    let source = unsafe { CGEventSourceCreate(CG_EVENT_SOURCE_HID_SYSTEM_STATE) };
    (!source.is_null()).then(|| CfGuard::new(source as CFTypeRef))
}

pub(crate) fn post_keyboard_event(
    source: &CfGuard,
    virtual_key: u16,
    key_down: bool,
    flags: u64,
) -> bool {
    // SAFETY: `source` 是有效事件源引用；成功返回的 +1 事件交给守卫，
    // 注入发生在守卫存活期内。
    let event = unsafe {
        CGEventCreateKeyboardEvent(source.raw() as CGEventSourceRef, virtual_key, key_down)
    };
    if event.is_null() {
        return false;
    }
    let _event = CfGuard::new(event as CFTypeRef);
    // SAFETY: `event` 是刚构造的有效事件引用，改写其修饰位字段。
    unsafe { CGEventSetFlags(event, flags) };
    // SAFETY: `event` 是刚构造的有效事件引用，CGEventPost 不接管所有权。
    unsafe { CGEventPost(K_CG_HID_EVENT_TAP, event) };
    true
}

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
