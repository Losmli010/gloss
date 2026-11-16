//! 快照写回（恢复序列）：文本写回、图像风格原字节放回或清板，单次失败
//! 短退避后重试。重试骨架与写盘结果形状住平台中立共享层（common）；本模
//! 块只留系统（Windows）的失败环节与单次序列。
//!
//! 系统（Windows）的恢复：文本快照走跨平台剪贴板库原样写回，空板快照
//! 清板，图像风格快照走剪贴板接口（Win32）原格式原字节放回（一次开板
//! 算一次写盘，字节即捕获字节，不经解码重编码）；剪贴板被其它应用短时
//! 占有时单次写回会失败，按系统（macOS）恢复序列的同款节奏重试（每次
//! 取全新剪贴板句柄）。

use arboard::Clipboard;

use gloss_core::log::{debug, error, thread};

use crate::clipboard::clipboard_write_flavors;
use crate::common::fallback::{
    RESTORE_ATTEMPTS, RESTORE_RETRY_DELAY, RestoreOutcome, restore_with_retry,
};

use super::snapshot::SavedContent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum RestoreStage {
    #[default]
    Clipboard,
    Set,
    Write,
    Clear,
}

fn restore_board(saved: &SavedContent) -> RestoreOutcome<RestoreStage> {
    restore_with_retry(RESTORE_ATTEMPTS, RESTORE_RETRY_DELAY, || {
        restore_once(saved)
    })
}

pub(super) fn restore_and_log(saved: &SavedContent) -> u64 {
    let outcome = restore_board(saved);
    if !outcome.ok {
        error!(
            thread = thread::EVENT,
            stage = ?outcome.stage,
            writes = outcome.writes,
            "failed to restore clipboard"
        );
    }
    outcome.writes
}

fn restore_once(saved: &SavedContent) -> RestoreOutcome<RestoreStage> {
    let fail = |stage: RestoreStage, writes: u64| {
        debug!(thread = thread::EVENT, ?stage, "restore attempt failed");
        RestoreOutcome {
            ok: false,
            writes,
            stage,
            status: 0,
        }
    };
    let settled = RestoreOutcome {
        ok: true,
        writes: 1,
        stage: RestoreStage::Clipboard,
        status: 0,
    };
    match saved {
        SavedContent::Flavors(flavors) => {
            let (restored, jumps) = clipboard_write_flavors(flavors);
            if restored {
                RestoreOutcome {
                    ok: true,
                    writes: jumps,
                    stage: RestoreStage::Clipboard,
                    status: 0,
                }
            } else {
                fail(RestoreStage::Write, jumps)
            }
        }
        SavedContent::Text(text) => {
            match Clipboard::new().and_then(|mut board| board.set_text(text.clone())) {
                Ok(()) => settled,
                Err(_) => fail(RestoreStage::Set, 0),
            }
        }
        SavedContent::Empty => match Clipboard::new().and_then(|mut board| board.clear()) {
            Ok(()) => settled,
            Err(_) => fail(RestoreStage::Clear, 0),
        },
    }
}
