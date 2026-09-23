//! CoreGraphics 事件 tap：只订阅、只读取事件的监听接口。

use std::ffi::c_void;

use super::cf::CFMachPortRef;

/// CoreGraphics 的事件引用：回调期间由系统借用，我们只读它的位置。
pub(crate) type CGEventRef = *const c_void;
/// tap 回调的 proxy 参数：只观察、不改写事件流的实现用不到。
pub(crate) type CGEventTapProxy = *const c_void;

/// 指针位置的返回结构（CGGeometry.h 的 CGPoint）。
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct CGPoint {
    pub(crate) x: f64,
    pub(crate) y: f64,
}

/// CGEventTapLocation 的 kCGHIDEventTap：事件链最早的一层。
pub(crate) const K_CG_HID_EVENT_TAP: u32 = 0;
/// CGEventTapPlacement 的 kCGHeadInsertEventTap。
pub(crate) const K_CG_HEAD_INSERT_EVENT_TAP: u32 = 0;
/// CGEventTapOptions 的 kCGEventTapOptionListenOnly：只观察，不改写事件流。
pub(crate) const K_CG_EVENT_TAP_OPTION_LISTEN_ONLY: u32 = 1;

/// tap 回调的 C 签名。
pub(crate) type TapCallback =
    unsafe extern "C" fn(CGEventTapProxy, u32, CGEventRef, *mut c_void) -> CGEventRef;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    /// 建立事件 tap；未获辅助功能授权（或系统 tap 名额耗尽）时返回 NULL。
    pub(crate) fn CGEventTapCreate(
        location: u32,
        placement: u32,
        options: u32,
        mask: u64,
        callback: TapCallback,
        user_info: *mut c_void,
    ) -> CFMachPortRef;
    /// 启用/停用 tap。
    pub(crate) fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    /// 事件发生时的指针位置（全局坐标，原点在左上）。
    pub(crate) fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
}
