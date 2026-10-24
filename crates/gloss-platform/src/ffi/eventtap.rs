//! 图形核心框架（CoreGraphics）的事件监听（tap）：只订阅、只读取事件的接口。

use std::ffi::c_void;

use super::cf::CFMachPortRef;

pub(crate) type CGEventRef = *const c_void;
pub(crate) type CGEventTapProxy = *const c_void;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct CGPoint {
    pub(crate) x: f64,
    pub(crate) y: f64,
}

pub(crate) const K_CG_HID_EVENT_TAP: u32 = 0;
pub(crate) const K_CG_HEAD_INSERT_EVENT_TAP: u32 = 0;
pub(crate) const K_CG_EVENT_TAP_OPTION_LISTEN_ONLY: u32 = 1;

pub(crate) type TapCallback =
    unsafe extern "C" fn(CGEventTapProxy, u32, CGEventRef, *mut c_void) -> CGEventRef;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    pub(crate) fn CGEventTapCreate(
        location: u32,
        placement: u32,
        options: u32,
        mask: u64,
        callback: TapCallback,
        user_info: *mut c_void,
    ) -> CFMachPortRef;
    pub(crate) fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    pub(crate) fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    pub(crate) fn CGEventGetTimestamp(event: CGEventRef) -> u64;
}
