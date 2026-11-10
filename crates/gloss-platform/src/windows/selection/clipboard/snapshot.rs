//! 原剪贴板的快照：内容保全与放弃判定。
//!
//! 快照的保全边界：可读文本原样保全；板上无文本且无任何格式（空板）按
//! 空板保全（恢复轮清板）；板上无文本但有格式（图片等非文本内容）时按
//! 图像风格尽力保全：便携式网络图形（PNG）注册格式、版本 5 位图
//! （CF_DIBV5）与位图（CF_DIB）按在板逐个捕获原始字节，超限或读不出的
//! 风格单独跳过（与系统（macOS）的“读不出数据的风格单独跳过”同界），
//! 一个都保不住才整体放弃、不注入——注入后原内容无法如实写回，恢复轮会
//! 清空剪贴板；板状态读不出同样放弃。恢复决策与写盘归因随读取结果拆分
//! 移交恢复收尾结构，调用方在取材产物上抛（弹窗链路启动）之后收口——
//! 恢复不挡在弹窗前。

use arboard::Clipboard;

use gloss_core::log::{debug, error, thread};

use super::restore::{restore_and_log, restore_board};
use crate::clipboard::FlavorCapture;

const SNAPSHOT_IMAGE_MAX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SavedContent {
    Empty,
    Text(String),
    Flavors(Vec<FlavorCapture>),
}

pub(super) fn snapshot_decision(
    read_text: Result<String, arboard::Error>,
    format_count: impl FnOnce() -> Option<u32>,
    capture_flavors: impl FnOnce() -> Vec<FlavorCapture>,
) -> Option<SavedContent> {
    match read_text {
        Ok(text) => Some(SavedContent::Text(text)),
        Err(arboard::Error::ContentNotAvailable) => {
            let Some(count) = format_count() else {
                debug!(
                    thread = thread::EVENT,
                    "clipboard state unreadable, fallback declined"
                );
                return None;
            };
            if count == 0 {
                return Some(SavedContent::Empty);
            }
            let flavors = capture_flavors();
            if flavors.is_empty() {
                debug!(
                    thread = thread::EVENT,
                    "non-text board content cannot be preserved, fallback declined"
                );
                return None;
            }
            Some(SavedContent::Flavors(flavors))
        }
        Err(_) => {
            debug!(
                thread = thread::EVENT,
                "clipboard text read failed, fallback declined"
            );
            None
        }
    }
}

pub(super) fn snapshot_board(clipboard: &mut Clipboard) -> Option<SavedContent> {
    snapshot_decision(
        clipboard.get_text(),
        crate::clipboard::clipboard_format_count,
        || crate::clipboard::clipboard_capture_image_flavors(SNAPSHOT_IMAGE_MAX_BYTES),
    )
}

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
        let outcome = restore_board(&self.saved);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn has_text(text: &str) -> Result<String, arboard::Error> {
        Ok(text.to_owned())
    }

    fn capture_of(formats: &[u32]) -> Vec<FlavorCapture> {
        formats
            .iter()
            .map(|&format| FlavorCapture {
                format,
                bytes: vec![7u8; format as usize],
            })
            .collect()
    }

    #[test]
    fn snapshot_keeps_readable_text_verbatim() {
        assert_eq!(
            snapshot_decision(has_text("preset"), || Some(3), Vec::new),
            Some(SavedContent::Text("preset".to_owned()))
        );
    }

    #[test]
    fn snapshot_accepts_a_confirmed_empty_board() {
        assert_eq!(
            snapshot_decision(
                Err(arboard::Error::ContentNotAvailable),
                || Some(0),
                Vec::new
            ),
            Some(SavedContent::Empty),
            "an empty board is faithfully restorable by clearing"
        );
    }

    #[test]
    fn snapshot_preserves_read_image_flavors_for_write_back() {
        let flavors = capture_of(&[17, 8]);
        assert_eq!(
            snapshot_decision(
                Err(arboard::Error::ContentNotAvailable),
                || Some(4),
                || flavors.clone()
            ),
            Some(SavedContent::Flavors(flavors)),
            "an image board is preserved raw so the injected copy stays affordable"
        );
    }

    #[test]
    fn snapshot_declines_a_non_text_board_it_cannot_restore() {
        assert_eq!(
            snapshot_decision(
                Err(arboard::Error::ContentNotAvailable),
                || Some(2),
                Vec::new
            ),
            None,
            "no board flavor was preservable, so no injection"
        );
    }

    #[test]
    fn snapshot_declines_unreadable_board_state() {
        assert_eq!(
            snapshot_decision(Err(arboard::Error::ContentNotAvailable), || None, Vec::new),
            None,
            "an unreadable format count leaves the board state unknown"
        );
        assert_eq!(
            snapshot_decision(
                Err(arboard::Error::Unknown {
                    description: "x".into()
                }),
                || Some(1),
                || capture_of(&[8])
            ),
            None,
            "a text read failure other than content-not-available declines the round"
        );
    }
}
