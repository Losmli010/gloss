//! 快照写回（恢复序列）：预建全部 `flavor` 数据 → 清板 → 逐条写回，单次
//! 序列失败短退避后重试（每次开全新粘贴板引用）。

use std::time::Duration;

use crate::ffi::cf::{CFDataCreate, CFIndex, CFStringRef, CFTypeRef, CfGuard, kCFAllocatorDefault};
use crate::ffi::pasteboard::{PasteboardClear, PasteboardItemID, PasteboardPutItemFlavor};

use gloss_core::log::{debug, error, thread};

use super::snapshot::{SavedContent, open_pasteboard_reported};

const RESTORE_ATTEMPTS: usize = 6;
const RESTORE_RETRY_DELAY: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RestoreStage {
    Open,
    Alloc,
    Clear,
    Put,
}

pub(super) struct RestoreOutcome {
    pub(super) ok: bool,
    pub(super) clears: u64,
    pub(super) stage: RestoreStage,
    pub(super) status: i32,
}

pub(super) fn restore_pasteboard(saved: &SavedContent) -> RestoreOutcome {
    restore_with_retry(RESTORE_ATTEMPTS, RESTORE_RETRY_DELAY, || {
        restore_pasteboard_once(saved)
    })
}

pub(super) fn restore_and_log(saved: &SavedContent) {
    let outcome = restore_pasteboard(saved);
    if !outcome.ok {
        error!(
            thread = thread::EVENT,
            stage = ?outcome.stage,
            status = outcome.status,
            clears = outcome.clears,
            "failed to restore clipboard"
        );
    }
}

pub(super) fn restore_with_retry(
    attempts: usize,
    delay: Duration,
    mut once: impl FnMut() -> RestoreOutcome,
) -> RestoreOutcome {
    let mut clears = 0;
    let (mut stage, mut status) = (RestoreStage::Open, 0);
    for attempt in 0..attempts {
        let outcome = once();
        clears += outcome.clears;
        if outcome.ok {
            return RestoreOutcome {
                ok: true,
                clears,
                stage: RestoreStage::Open,
                status: 0,
            };
        }
        stage = outcome.stage;
        status = outcome.status;
        if attempt + 1 < attempts {
            std::thread::sleep(delay);
        }
    }
    RestoreOutcome {
        ok: false,
        clears,
        stage,
        status,
    }
}

fn restore_pasteboard_once(saved: &SavedContent) -> RestoreOutcome {
    let fail = |stage: RestoreStage, status: i32, clears: u64| {
        debug!(
            thread = thread::EVENT,
            ?stage,
            status,
            "restore attempt failed"
        );
        RestoreOutcome {
            ok: false,
            clears,
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
    // SAFETY: `raw` 是有效 +1 引用，存活至本结构销毁。
    let status = unsafe { PasteboardClear(pb.raw) };
    if status != 0 {
        return fail(RestoreStage::Clear, status, 0);
    }
    for (id, name, data) in &staged {
        // SAFETY: `raw`/`id`/`name`/`data` 均为阶段一备齐的有效引用；
        // Put 不接管 `data` 引用，由 staged 的守卫释放。
        let status = unsafe { PasteboardPutItemFlavor(pb.raw, *id, *name, data.data_ref(), 0) };
        if status != 0 {
            return fail(RestoreStage::Put, status, 1);
        }
    }
    RestoreOutcome {
        ok: true,
        clears: 1,
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
            // SAFETY: `data` 字节缓冲在本次调用内存活且非悬空，长度即
            // 缓冲长度；分配失败返回 NULL，随后判别。
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
