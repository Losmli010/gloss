//! 快照写回（恢复序列）：文本写回、图像风格原字节放回或清板，单次失败
//! 短退避后重试。重试骨架与写盘结果形状住平台中立共享层（common）；本模
//! 块只留系统（Windows）的失败环节与单次序列。
//!
//! 系统（Windows）的恢复：文本快照走跨平台剪贴板库原样写回，空板快照
//! 清板，图像风格快照走剪贴板接口（Win32）原格式原字节放回（一次开板
//! 窗口内清板加逐风格放回，字节即捕获字节，不经解码重编码）。写盘数按
//! 剪贴板序列号的真实跳变数自报：图像走写回面原生计量，文本写回开板前
//! 后各取一次序列号求差（成对读数才计量；成功轮读数缺席按下界两跳计，
//! 失败轮读数缺席记零），清板恰为一跳；写盘归因对齐哨兵口径，少报会让
//! 恢复回板的内容被当成用户复制。剪贴板被其它应用短时占有时单次写回会
//! 失败，按系统（macOS）恢复序列的同款节奏重试（每次取全新剪贴板句柄）。

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
        SavedContent::Text(text) => restore_text(text, &fail),
        SavedContent::Empty => match Clipboard::new().and_then(|mut board| board.clear()) {
            Ok(()) => settled,
            Err(_) => fail(RestoreStage::Clear, 0),
        },
    }
}

fn restore_text(
    text: &str,
    fail: &impl Fn(RestoreStage, u64) -> RestoreOutcome<RestoreStage>,
) -> RestoreOutcome<RestoreStage> {
    let before = crate::clipboard::clipboard_sequence_number();
    let written = Clipboard::new().and_then(|mut board| board.set_text(text.to_owned()));
    let after = crate::clipboard::clipboard_sequence_number();
    match written {
        Ok(()) => RestoreOutcome {
            ok: true,
            writes: sequence_jumps(before, after).unwrap_or(2),
            stage: RestoreStage::Clipboard,
            status: 0,
        },
        Err(_) => fail(
            RestoreStage::Set,
            sequence_jumps(before, after).unwrap_or(0),
        ),
    }
}

fn sequence_jumps(before: Option<u64>, after: Option<u64>) -> Option<u64> {
    match (before, after) {
        (Some(before), Some(after)) => Some(after.saturating_sub(before)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::sequence_jumps;

    #[test]
    fn sequence_jumps_measures_paired_readings_and_declines_missing_ones() {
        assert_eq!(sequence_jumps(Some(7), Some(9)), Some(2));
        assert_eq!(sequence_jumps(Some(9), Some(9)), Some(0));
        assert_eq!(sequence_jumps(Some(9), Some(7)), Some(0));
        for (before, after) in [(None, Some(9)), (Some(7), None), (None, None)] {
            assert_eq!(
                sequence_jumps(before, after),
                None,
                "a missing reading carries no measurement"
            );
        }
    }
}
