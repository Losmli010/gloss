//! 剪贴板兜底读取（模拟复制）：系统（Windows）绑定。
//!
//! 用户界面自动化（UIA）读不到选区时的兜底通道的绑定面：装配骨架与共享
//! 流程（平台中立共享层）只经绑定束与板面原语束触板，本模块提供系统
//! （Windows）的原语——写盘归因基线取剪贴板序列号；快照按内容类型判定
//! （可读文本、确认的空板或图像风格原字节，边界见快照模块文档）并按快照
//! 还原；写入确认监视以剪贴板序列号差承载代数——序列号是系统级单调计数，
//! 基线取建立监视时的现值；注入复制快捷键走按键注入（SendInput），注入被
//! 静默丢弃（对提权进程的注入，用户界面特权隔离（UIPI））或目标未响应时
//! 序列号不动，共享流程据此重注入一轮，不区分提权与不支持（不做提权）。
//! 读取在平台事件线程上调用（与用户界面自动化（UIA）主路同线程），收尾
//! 同线程。

mod restore;
mod snapshot;

use crate::clipboard::{WatchRoundFlag, clipboard_sequence_number};
use crate::common::fallback::{FallbackBindings, FallbackSettle, read_with_fallback};
use crate::ffi::inject;

use gloss_core::log::{debug, thread};
use gloss_core::model::GlossError;

use restore::restore_and_log;
use snapshot::{SNAPSHOT_IMAGE_MAX_BYTES, SavedContent, snapshot_decision};

#[derive(Debug, Default, Clone)]
pub struct ClipboardFallbackReader {
    round: WatchRoundFlag,
}

impl ClipboardFallbackReader {
    pub fn new(round: WatchRoundFlag) -> Self {
        Self { round }
    }

    pub(crate) fn read(&mut self) -> (Result<String, GlossError>, FallbackSettle<SavedContent>) {
        read_with_fallback(
            self.round.clone(),
            FallbackBindings {
                pre_count: || clipboard_sequence_number().unwrap_or(0),
                snapshot: |text| {
                    snapshot_decision(text, crate::clipboard::clipboard_format_count, || {
                        crate::clipboard::clipboard_capture_image_flavors(SNAPSHOT_IMAGE_MAX_BYTES)
                    })
                },
                restore: restore_and_log,
                monitor: {
                    let mut monitor: Option<ChangeMonitor> = None;
                    move || {
                        let fresh = monitor.is_none();
                        if fresh {
                            monitor = ChangeMonitor::new();
                        }
                        let change = monitor.as_ref()?;
                        if fresh {
                            Some(change.baseline())
                        } else {
                            change.generation()
                        }
                    }
                },
                inject: inject_copy_key,
            },
        )
    }
}

struct ChangeMonitor {
    baseline: u64,
}

impl ChangeMonitor {
    fn new() -> Option<Self> {
        Some(Self {
            baseline: clipboard_sequence_number()?,
        })
    }

    fn baseline(&self) -> u64 {
        self.baseline
    }

    fn generation(&self) -> Option<u64> {
        clipboard_sequence_number()
    }
}

fn inject_copy_key() -> Result<(), GlossError> {
    match inject::inject_copy() {
        Ok(()) => Ok(()),
        Err(failure) => {
            debug!(
                thread = thread::EVENT,
                injected = failure.injected,
                "copy key injection failed"
            );
            Err(GlossError::SelectionUnavailable)
        }
    }
}

#[cfg(test)]
#[allow(clippy::let_underscore_must_use)]
mod live_tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use arboard::Clipboard;

    use gloss_core::model::GlossError;

    use super::ClipboardFallbackReader;
    use crate::clipboard::{WatchRoundFlag, clipboard_has_image};

    static CLIPBOARD_LIVE_LOCK: Mutex<()> = Mutex::new(());

    const UNWIND_RESTORE_EVENT_TIMEOUT: Duration = Duration::from_secs(5);
    const UNWIND_RESTORE_EVENT_POLL: Duration = Duration::from_millis(50);

    #[test]
    #[ignore = "需真机交互：测试会向前台应用注入真实 Ctrl+C 并覆写剪贴板（前台需有选中文本）"]
    fn fallback_read_restores_original_clipboard_on_windows() {
        let _clipboard = CLIPBOARD_LIVE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut clipboard = Clipboard::new().expect("clipboard should be available");
        let preset = format!("gloss-restore-{}", std::process::id());
        clipboard.set_text(preset.clone()).expect("preset text");

        let round = WatchRoundFlag::default();
        let mut reader = ClipboardFallbackReader::new(round.clone());
        let (outcome, settle) = reader.read();
        settle.finish();
        let acquired = outcome.expect("fallback should read the live selection");
        assert!(!acquired.is_empty());

        let restored = clipboard.get_text().expect("clipboard should be readable");
        assert!(
            restored == preset,
            "original clipboard content must survive (got {} bytes)",
            restored.len()
        );
    }

    #[test]
    #[ignore = "需真机交互：测试会向前台应用注入真实 Ctrl+C 并在收尾前覆写剪贴板（前台需有选中文本）"]
    fn fallback_read_unwind_restores_the_clipboard_without_finish_on_windows() {
        let _clipboard = CLIPBOARD_LIVE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut clipboard = Clipboard::new().expect("clipboard should be available");
        let preset = format!("gloss-unwind-restore-{}", std::process::id());
        clipboard.set_text(preset.clone()).expect("preset text");
        let clobber = "gloss-injected-copy";

        let mut reader = ClipboardFallbackReader::new(WatchRoundFlag::default());
        let (outcome, settle) = reader.read();
        let restore_contracted = settle.restores_on_unwind();

        clipboard
            .set_text(clobber.to_owned())
            .expect("simulate the injected copy clobbering the board");

        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _settle = settle;
            panic!("simulated caller panic before finish");
        }));
        assert!(
            unwound.is_err(),
            "the simulated caller panic must unwind through the settle"
        );

        if !restore_contracted {
            let restored = clipboard.get_text().expect("clipboard should be readable");
            assert!(
                restored == clobber,
                "a declined round must not inject or restore; the board keeps the external \
                 write (got {} bytes, outcome ok={})",
                restored.len(),
                outcome.is_ok()
            );
            return;
        }

        let deadline = std::time::Instant::now() + UNWIND_RESTORE_EVENT_TIMEOUT;
        loop {
            if matches!(clipboard.get_text(), Ok(text) if text == preset) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the unwind Drop must restore the snapshot (outcome ok={})",
                outcome.is_ok()
            );
            std::thread::sleep(UNWIND_RESTORE_EVENT_POLL);
        }
    }

    #[test]
    #[ignore = "需真机交互：测试覆写真实剪贴板为测试数据且不恢复，并向前台应用注入真实 Ctrl+C（建议前台停在编辑器）"]
    fn a_fallback_round_on_an_image_board_preserves_the_image_on_windows() {
        let _clipboard = CLIPBOARD_LIVE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let probe = b"gloss-image-board-probe".to_vec();
        assert!(
            crate::clipboard::clipboard_write_flavors(&[crate::clipboard::FlavorCapture {
                format: crate::ffi::board::CF_DIBV5,
                bytes: probe.clone(),
            }])
            .0,
            "the probe image must land on the board"
        );

        let mut reader = ClipboardFallbackReader::new(WatchRoundFlag::default());
        let (outcome, settle) = reader.read();
        let proceeded = settle.restores_on_unwind();
        settle.finish();

        assert!(
            proceeded,
            "an image on the board must not decline the fallback round: \
             the snapshot preserves it and the injection proceeds"
        );
        assert!(
            matches!(outcome, Err(GlossError::SelectionUnavailable)),
            "without a live selection the round ends as an ordinary acquisition \
             failure, got {outcome:?}"
        );
        assert!(
            clipboard_has_image(),
            "the preserved image must survive the skipped restore"
        );
        let recaptured = crate::clipboard::clipboard_capture_image_flavors(usize::MAX);
        assert_eq!(
            recaptured.len(),
            1,
            "exactly the probe flavor stays on the board"
        );
        assert_eq!(
            recaptured[0].bytes, probe,
            "the image bytes survive verbatim"
        );
    }
}
