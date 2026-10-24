//! 图形核心框架（CoreGraphics）事件注入：模拟复制（Cmd+C）与真机测试的合成事件。
//!
//! 与事件监听（tap）分居两个模块：这里只“造事件、发事件”。
//! 事件经系统事件链入口注入，对目标应用等价于真实键鼠输入；
//! 注入需要辅助功能授权，未授权时事件被系统静默丢弃（事件投递调用无
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
    // SAFETY: a pure construction FFI with no preconditions; failure returns
    // NULL, which is handled as failure here.
    let source = unsafe { CGEventSourceCreate(CG_EVENT_SOURCE_HID_SYSTEM_STATE) };
    (!source.is_null()).then(|| CfGuard::new(source as CFTypeRef))
}

pub(crate) fn post_keyboard_event(
    source: &CfGuard,
    virtual_key: u16,
    key_down: bool,
    flags: u64,
) -> bool {
    // SAFETY: `source` is a valid event source reference; the +1 event returned
    // on success is handed to the guard, and the posting happens while the guard
    // is alive.
    let event = unsafe {
        CGEventCreateKeyboardEvent(source.raw() as CGEventSourceRef, virtual_key, key_down)
    };
    if event.is_null() {
        return false;
    }
    let _event = CfGuard::new(event as CFTypeRef);
    // SAFETY: `event` is a valid event reference just constructed above, whose
    // modifier flag field is being rewritten.
    unsafe { CGEventSetFlags(event, flags) };
    // SAFETY: `event` is a valid event reference just constructed above;
    // `CGEventPost` does not take ownership.
    unsafe { CGEventPost(K_CG_HID_EVENT_TAP, event) };
    true
}

#[cfg(test)]
pub(crate) fn post_mouse_event(source: &CfGuard, kind: u32, position: CGPoint) -> bool {
    // SAFETY: same invariants as `post_keyboard_event` — the source is valid and
    // the +1 event is handed to the guard.
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
    // SAFETY: `event` is a valid event reference just constructed above;
    // `CGEventPost` does not take ownership.
    unsafe { CGEventPost(K_CG_HID_EVENT_TAP, event) };
    true
}

#[cfg(test)]
pub(crate) fn current_position(source: &CfGuard) -> Option<CGPoint> {
    // SAFETY: `source` is a valid event source reference; the +1 event is handed
    // to the guard, and the location query happens while it is alive.
    let event = unsafe { CGEventCreate(source.raw() as CGEventSourceRef) };
    if event.is_null() {
        return None;
    }
    let _event = CfGuard::new(event as CFTypeRef);
    // SAFETY: `event` is a valid event reference and `CGEventGetLocation` is a
    // read-only query.
    Some(unsafe { CGEventGetLocation(event) })
}
