//! 原剪贴板的快照：打开系统粘贴板、逐条目保全全部 flavor 的原始字节。
//!
//! 快照的放弃边界（哪些失败整体放弃、哪些只跳过该 flavor）以
//! [`snapshot_pasteboard`] 的文档为准。

use crate::ffi::cf::{
    self, CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef, CFDataGetBytePtr, CFDataGetLength,
    CFDataRef, CFIndex, CFRelease, CFRetain, CFStringRef, CFTypeRef, CfGuard,
};
use crate::ffi::pasteboard::{
    CLIPBOARD_NAME, PasteboardCopyItemFlavorData, PasteboardCopyItemFlavors, PasteboardCreate,
    PasteboardGetItemCount, PasteboardGetItemIdentifier, PasteboardItemID, PasteboardRef,
    PasteboardSynchronize,
};

use gloss_core::log::{debug, error, thread};

use super::restore::{restore_and_log, restore_pasteboard};

pub(super) struct OpenPasteboard {
    pub(super) raw: PasteboardRef,
    _name: CfGuard,
}

impl Drop for OpenPasteboard {
    fn drop(&mut self) {
        // SAFETY: `raw` 是 PasteboardCreate 返回的 +1 引用（非空已在
        // 创建时判别），此处是唯一释放点，恰好归还一次。
        unsafe { CFRelease(self.raw as CFTypeRef) };
    }
}

pub(super) fn open_pasteboard() -> Option<OpenPasteboard> {
    open_pasteboard_reported().ok()
}

pub(super) fn open_pasteboard_reported() -> Result<OpenPasteboard, i32> {
    let Some(name) = cf::cf_string(CLIPBOARD_NAME) else {
        return Err(-1);
    };
    let mut raw: PasteboardRef = std::ptr::null_mut();
    // SAFETY: `name` 是有效 CFString 引用，出参指向栈上变量；失败时不
    // 写入出参。
    let status = unsafe { PasteboardCreate(name.string_ref(), &mut raw) };
    if status != 0 || raw.is_null() {
        return Err(status);
    }
    Ok(OpenPasteboard { raw, _name: name })
}

pub(super) struct SavedFlavor {
    pub(super) name: CfGuard,
    pub(super) data: Vec<u8>,
}

pub(super) struct SavedItem {
    pub(super) id: PasteboardItemID,
    pub(super) flavors: Vec<SavedFlavor>,
}

pub(super) struct SavedContent(pub(super) Vec<SavedItem>);

pub(super) struct PendingRestore {
    saved: SavedContent,
    skip: bool,
}

impl PendingRestore {
    pub(super) fn new(saved: SavedContent) -> Self {
        Self { saved, skip: false }
    }

    pub(super) fn restore_now(&mut self) -> u64 {
        self.skip = true;
        let outcome = restore_pasteboard(&self.saved);
        if !outcome.ok {
            error!(
                thread = thread::EVENT,
                stage = ?outcome.stage,
                status = outcome.status,
                clears = outcome.clears,
                "failed to restore clipboard"
            );
        }
        outcome.clears
    }

    pub(super) fn skip_restore(&mut self) {
        self.skip = true;
    }
}

impl Drop for PendingRestore {
    fn drop(&mut self) {
        if !self.skip {
            restore_and_log(&self.saved);
        }
    }
}

pub(super) fn snapshot_pasteboard() -> Option<SavedContent> {
    let Some(pb) = open_pasteboard() else {
        debug!(
            thread = thread::EVENT,
            "pasteboard unavailable, fallback declined"
        );
        return None;
    };
    // SAFETY: `raw` 是有效 +1 引用，存活至本结构销毁；顺带消费既有
    // modified 标志，避免把快照前的历史改动算作目标应用的写入。
    unsafe { PasteboardSynchronize(pb.raw) };
    let mut count: usize = 0;
    // SAFETY: `raw` 是有效 +1 引用，出参指向栈上变量。
    let status = unsafe { PasteboardGetItemCount(pb.raw, &mut count) };
    if status != 0 {
        debug!(
            thread = thread::EVENT,
            status, "item count query failed, fallback declined"
        );
        return None;
    }
    let Ok(count) = CFIndex::try_from(count) else {
        debug!(
            thread = thread::EVENT,
            items = count,
            "item count out of range, fallback declined"
        );
        return None;
    };
    let items = snapshot_items(&pb, count)?;
    if count > 0 && items.is_empty() {
        debug!(
            thread = thread::EVENT,
            "no flavor on the board is restorable, fallback declined"
        );
        return None;
    }
    Some(SavedContent(items))
}

fn snapshot_items(pb: &OpenPasteboard, count: CFIndex) -> Option<Vec<SavedItem>> {
    let mut items = Vec::new();
    for index in 1..=count {
        let mut id: PasteboardItemID = std::ptr::null_mut();
        // SAFETY: `raw` 是有效 +1 引用，出参指向刚声明的栈上变量。
        let status = unsafe { PasteboardGetItemIdentifier(pb.raw, index, &mut id) };
        if status != 0 {
            debug!(
                thread = thread::EVENT,
                status, "item identifier query failed, fallback declined"
            );
            return None;
        }
        let mut flavors_ref: CFArrayRef = std::ptr::null();
        // SAFETY: `raw` 与 `id` 均为有效引用，出参指向栈上变量；失败时
        // 不写入出参。
        let status = unsafe { PasteboardCopyItemFlavors(pb.raw, id, &mut flavors_ref) };
        if status != 0 {
            debug!(
                thread = thread::EVENT,
                status, "flavor list query failed, fallback declined"
            );
            return None;
        }
        let _flavors = CfGuard::new(flavors_ref as CFTypeRef);
        // SAFETY: `flavors_ref` 是刚拷贝的有效 CFArray 引用。
        let flavor_count = unsafe { CFArrayGetCount(flavors_ref) };
        let Ok(flavor_count) = usize::try_from(flavor_count) else {
            debug!(
                thread = thread::EVENT,
                "flavor count out of range, fallback declined"
            );
            return None;
        };
        let flavors = snapshot_flavors(pb, id, flavors_ref, flavor_count)?;
        if flavors.is_empty() {
            debug!(
                thread = thread::EVENT,
                "no restorable flavor on the pasteboard item, item skipped"
            );
            continue;
        }
        items.push(SavedItem { id, flavors });
    }
    Some(items)
}

fn snapshot_flavors(
    pb: &OpenPasteboard,
    id: PasteboardItemID,
    flavors_ref: CFArrayRef,
    flavor_count: usize,
) -> Option<Vec<SavedFlavor>> {
    let mut flavors = Vec::with_capacity(flavor_count);
    for flavor_index in 0..flavor_count {
        // SAFETY: 索引在 GetCount 范围内；返回的元素引用由数组持有，
        // 存入快照前先 Retain 自持。
        let flavor =
            unsafe { CFArrayGetValueAtIndex(flavors_ref, flavor_index as CFIndex) } as CFStringRef;
        // SAFETY: `flavor` 是有效 CF 对象引用，Retain 后交由守卫
        // 恰好释放一次。
        let name = CfGuard::new(unsafe { CFRetain(flavor as CFTypeRef) });
        let mut data: CFDataRef = std::ptr::null();
        // SAFETY: `raw`/`id`/`flavor` 均为有效引用，出参指向栈上
        // 变量；数据未物化（promised）时返回失败。
        let status = unsafe { PasteboardCopyItemFlavorData(pb.raw, id, flavor, &mut data) };
        if status != 0 {
            debug!(
                thread = thread::EVENT,
                status, "flavor data unavailable (promised?), flavor left out of the snapshot"
            );
            continue;
        }
        let _data = CfGuard::new(data as CFTypeRef);
        // SAFETY: `data` 是刚拷贝的有效 CFData 引用，长度与字节
        // 指针来自同一对象。
        let (len, bytes_ptr) = unsafe { (CFDataGetLength(data), CFDataGetBytePtr(data)) };
        let Ok(len) = usize::try_from(len) else {
            debug!(
                thread = thread::EVENT,
                "flavor data length out of range, fallback declined"
            );
            return None;
        };
        let bytes = if len == 0 {
            Vec::new()
        } else {
            // SAFETY: `bytes_ptr` 非 NULL（长度非零的 CFData 保证），
            // 可读字节数恰为 `len`；复制进快照以脱离 CF 生命周期。
            let bytes = unsafe { std::slice::from_raw_parts(bytes_ptr, len) };
            bytes.to_vec()
        };
        flavors.push(SavedFlavor { name, data: bytes });
    }
    Some(flavors)
}
