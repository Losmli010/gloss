//! 快照写回（恢复序列）：预建全部数据风格数据 → 清板 → 逐条写回，单次
//! 序列失败短退避后重试（每次开全新粘贴板引用）。重试骨架与写盘结果形状
//! 住平台中立共享层（common）；本模块只留系统（macOS）的失败环节与单次
//! 序列。

use crate::common::fallback::{
    RESTORE_ATTEMPTS, RESTORE_RETRY_DELAY, RestoreOutcome, restore_with_retry,
};
use crate::ffi::cf::{CFDataCreate, CFIndex, CFStringRef, CFTypeRef, CfGuard, kCFAllocatorDefault};
use crate::ffi::pasteboard::{PasteboardClear, PasteboardItemID, PasteboardPutItemFlavor};

use gloss_core::log::{debug, error, thread};

use super::snapshot::{SavedContent, open_pasteboard_reported};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum RestoreStage {
    #[default]
    Open,
    Alloc,
    Clear,
    Put,
}

fn restore_pasteboard(saved: &SavedContent) -> RestoreOutcome<RestoreStage> {
    restore_with_retry(RESTORE_ATTEMPTS, RESTORE_RETRY_DELAY, || {
        restore_pasteboard_once(saved)
    })
}

pub(super) fn restore_and_log(saved: &SavedContent) -> u64 {
    let outcome = restore_pasteboard(saved);
    if !outcome.ok {
        error!(
            thread = thread::EVENT,
            stage = ?outcome.stage,
            status = outcome.status,
            clears = outcome.writes,
            "failed to restore clipboard"
        );
    }
    outcome.writes
}

fn restore_pasteboard_once(saved: &SavedContent) -> RestoreOutcome<RestoreStage> {
    let fail = |stage: RestoreStage, status: i32, writes: u64| {
        debug!(
            thread = thread::EVENT,
            ?stage,
            status,
            "restore attempt failed"
        );
        RestoreOutcome {
            ok: false,
            writes,
            stage,
            status,
        }
    };
    let pb = match open_pasteboard_reported() {
        Ok(pb) => pb,
        Err(status) => return fail(RestoreStage::Open, status, 0),
    };
    let Some(staged) = stage_flavor_data(saved) else {
        return fail(RestoreStage::Alloc, 0, 0);
    };
    // SAFETY: `raw` is a valid +1 reference that stays alive until this struct
    // is destroyed.
    let status = unsafe { PasteboardClear(pb.raw) };
    if status != 0 {
        return fail(RestoreStage::Clear, status, 0);
    }
    for (id, name, data) in &staged {
        // SAFETY: `raw`/`id`/`name`/`data` are all valid references staged in
        // phase one; `Put` does not take ownership of the `data` reference, which
        // is released by the guard in `staged`.
        let status = unsafe { PasteboardPutItemFlavor(pb.raw, *id, *name, data.data_ref(), 0) };
        if status != 0 {
            return fail(RestoreStage::Put, status, 1);
        }
    }
    RestoreOutcome {
        ok: true,
        writes: 1,
        stage: RestoreStage::Open,
        status: 0,
    }
}

fn stage_flavor_data(
    saved: &SavedContent,
) -> Option<Vec<(PasteboardItemID, CFStringRef, CfGuard)>> {
    let mut staged = Vec::new();
    for item in &saved.0 {
        for flavor in &item.flavors {
            // SAFETY: the `data` byte buffer stays alive for the duration of this
            // call and is not dangling, with the length matching the buffer;
            // allocation failure returns NULL and is checked right after.
            let data = unsafe {
                CFDataCreate(
                    kCFAllocatorDefault,
                    flavor.data.as_ptr(),
                    flavor.data.len() as CFIndex,
                )
            };
            if data.is_null() {
                return None;
            }
            staged.push((
                item.id,
                flavor.name.string_ref(),
                CfGuard::new(data as CFTypeRef),
            ));
        }
    }
    Some(staged)
}
