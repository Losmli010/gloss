//! 粘贴板（ApplicationServices 的 Pasteboard 接口，Carbon 时代沿用的 C API）。

use std::ffi::{CStr, c_void};

use super::cf::{CFArrayRef, CFDataRef, CFIndex, CFStringRef};

/// 系统剪贴板的注册名（kPasteboardClipboard 的字符串值）。
pub(crate) const CLIPBOARD_NAME: &CStr = c"com.apple.pasteboard.clipboard";

/// kPasteboardModified：自上次经本地引用访问以来全局粘贴板已被修改；
/// 标志在 Synchronize 调用时被消费，探针侧需闩锁。
pub(crate) const K_PASTEBOARD_MODIFIED: u32 = 1 << 0;

/// 粘贴板句柄：CF 不透明类型，Create 返回 +1 引用。
pub(crate) type PasteboardRef = *mut c_void;
/// 粘贴板条目标识符：客户端自定义的不透明指针，同一次快照内唯一即可。
pub(crate) type PasteboardItemID = *mut c_void;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    /// 创建指向指定名称全局粘贴板的本地引用（+1），失败返回非零状态码。
    pub(crate) fn PasteboardCreate(name: CFStringRef, out: *mut PasteboardRef) -> i32;
    /// 与全局粘贴板同步，返回标志集（含 kPasteboardModified）。
    pub(crate) fn PasteboardSynchronize(pasteboard: PasteboardRef) -> u32;
    /// 返回粘贴板条目数，失败返回非零状态码。出参是 ItemCount
    /// （MacTypes.h 的 unsigned long，Darwin LP64 下 8 字节）。
    pub(crate) fn PasteboardGetItemCount(pasteboard: PasteboardRef, out_count: *mut usize) -> i32;
    /// 取条目标识符（index 从 1 起，见 Pasteboard.h），失败返回非零状态码。
    pub(crate) fn PasteboardGetItemIdentifier(
        pasteboard: PasteboardRef,
        index: CFIndex,
        out: *mut PasteboardItemID,
    ) -> i32;
    /// 拷贝条目的全部 flavor 名（CFArray，元素为 CFString，数组 +1 引用），
    /// 失败返回非零状态码。
    pub(crate) fn PasteboardCopyItemFlavors(
        pasteboard: PasteboardRef,
        item: PasteboardItemID,
        out: *mut CFArrayRef,
    ) -> i32;
    /// 拷贝条目指定 flavor 的原始数据（+1 引用）；数据尚未物化（promised）
    /// 时失败，失败返回非零状态码。
    pub(crate) fn PasteboardCopyItemFlavorData(
        pasteboard: PasteboardRef,
        item: PasteboardItemID,
        flavor: CFStringRef,
        out: *mut CFDataRef,
    ) -> i32;
    /// 清空粘贴板全部条目，失败返回非零状态码。
    pub(crate) fn PasteboardClear(pasteboard: PasteboardRef) -> i32;
    /// 向条目写入 flavor 数据；不接管 `data` 引用，调用方保活至调用返回。
    pub(crate) fn PasteboardPutItemFlavor(
        pasteboard: PasteboardRef,
        item: PasteboardItemID,
        flavor: CFStringRef,
        data: CFDataRef,
        flags: u32,
    ) -> i32;
}
