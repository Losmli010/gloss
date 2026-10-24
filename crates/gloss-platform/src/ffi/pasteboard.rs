//! 粘贴板（ApplicationServices 的 Pasteboard 接口，Carbon 时代沿用的 C API）。

use std::ffi::{CStr, c_void};

use super::cf::{CFArrayRef, CFDataRef, CFIndex, CFStringRef};

pub(crate) const CLIPBOARD_NAME: &CStr = c"com.apple.pasteboard.clipboard";

pub(crate) const K_PASTEBOARD_MODIFIED: u32 = 1 << 0;

pub(crate) type PasteboardRef = *mut c_void;
pub(crate) type PasteboardItemID = *mut c_void;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    pub(crate) fn PasteboardCreate(name: CFStringRef, out: *mut PasteboardRef) -> i32;
    pub(crate) fn PasteboardSynchronize(pasteboard: PasteboardRef) -> u32;
    pub(crate) fn PasteboardGetItemCount(pasteboard: PasteboardRef, out_count: *mut usize) -> i32;
    pub(crate) fn PasteboardGetItemIdentifier(
        pasteboard: PasteboardRef,
        index: CFIndex,
        out: *mut PasteboardItemID,
    ) -> i32;
    pub(crate) fn PasteboardCopyItemFlavors(
        pasteboard: PasteboardRef,
        item: PasteboardItemID,
        out: *mut CFArrayRef,
    ) -> i32;
    pub(crate) fn PasteboardCopyItemFlavorData(
        pasteboard: PasteboardRef,
        item: PasteboardItemID,
        flavor: CFStringRef,
        out: *mut CFDataRef,
    ) -> i32;
    pub(crate) fn PasteboardClear(pasteboard: PasteboardRef) -> i32;
    pub(crate) fn PasteboardPutItemFlavor(
        pasteboard: PasteboardRef,
        item: PasteboardItemID,
        flavor: CFStringRef,
        data: CFDataRef,
        flags: u32,
    ) -> i32;
}
