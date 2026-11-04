//! 原剪贴板的快照：打开系统粘贴板、逐条目保全全部数据风格的原始字节。
//!
//! 快照的放弃边界——哪些失败整体放弃、哪些只跳过该风格——以
//! 快照函数的文档为准。

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
        // SAFETY: `raw` is the +1 reference returned by `PasteboardCreate`
        // (checked non-null at creation time); this is the sole release point and
        // gives the reference back exactly once.
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
    // SAFETY: `name` is a valid CFString reference and the out parameter points
    // to a stack variable; on failure the out parameter is not written.
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
    // SAFETY: `raw` is a valid +1 reference that stays alive until this struct is
    // destroyed; this also consumes the existing modified flag, so historical
    // changes before the snapshot are not counted as the target app's write.
    unsafe { PasteboardSynchronize(pb.raw) };
    let mut count: usize = 0;
    // SAFETY: `raw` is a valid +1 reference and the out parameter points to a
    // stack variable.
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
        // SAFETY: `raw` is a valid +1 reference and the out parameter points to
        // the stack variable just declared.
        let status = unsafe { PasteboardGetItemIdentifier(pb.raw, index, &mut id) };
        if status != 0 {
            debug!(
                thread = thread::EVENT,
                status, "item identifier query failed, fallback declined"
            );
            return None;
        }
        let mut flavors_ref: CFArrayRef = std::ptr::null();
        // SAFETY: `raw` and `id` are both valid references and the out parameter
        // points to a stack variable; on failure the out parameter is not written.
        let status = unsafe { PasteboardCopyItemFlavors(pb.raw, id, &mut flavors_ref) };
        if status != 0 {
            debug!(
                thread = thread::EVENT,
                status, "flavor list query failed, fallback declined"
            );
            return None;
        }
        let _flavors = CfGuard::new(flavors_ref as CFTypeRef);
        // SAFETY: `flavors_ref` is a valid CFArray reference just copied above.
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
        // SAFETY: the index is within the `GetCount` range; the returned element
        // reference is owned by the array and is retained before being stored in
        // the snapshot.
        let flavor =
            unsafe { CFArrayGetValueAtIndex(flavors_ref, flavor_index as CFIndex) } as CFStringRef;
        // SAFETY: `flavor` is a valid CF object reference; after retaining, the
        // guard releases it exactly once.
        let name = CfGuard::new(unsafe { CFRetain(flavor as CFTypeRef) });
        let mut data: CFDataRef = std::ptr::null();
        // SAFETY: `raw`/`id`/`flavor` are all valid references and the out
        // parameter points to a stack variable; the call fails when the data is
        // only promised.
        let status = unsafe { PasteboardCopyItemFlavorData(pb.raw, id, flavor, &mut data) };
        if status != 0 {
            debug!(
                thread = thread::EVENT,
                status, "flavor data unavailable (promised?), flavor left out of the snapshot"
            );
            continue;
        }
        let _data = CfGuard::new(data as CFTypeRef);
        // SAFETY: `data` is a valid CFData reference just copied, with the length
        // and the byte pointer coming from the same object.
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
            // SAFETY: `bytes_ptr` is non-NULL (guaranteed by CFData with a
            // non-zero length) and the readable byte count is exactly `len`; the
            // bytes are copied into the snapshot to outlive the CF lifetime.
            let bytes = unsafe { std::slice::from_raw_parts(bytes_ptr, len) };
            bytes.to_vec()
        };
        flavors.push(SavedFlavor { name, data: bytes });
    }
    Some(flavors)
}
