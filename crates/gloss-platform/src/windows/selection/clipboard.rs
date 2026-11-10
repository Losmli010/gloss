//! 剪贴板兜底读取（模拟复制）：兜底读取器。
//!
//! 用户界面自动化（UIA）读不到选区时的兜底通道：快照剪贴板内容 → 注入
//! 复制快捷键 → 以剪贴板序列号轮询确认目标应用写入 → 读回 → 读回成功且
//! 现值未变才按快照恢复（失败轮一律不恢复——板上要么未变，要么是本轮
//! 之外的新写入或注入副产物，都不是可覆盖的内容）。确认语义以序列号差
//! 承载：注入被静默丢弃（对提权进程的注入，用户界面特权隔离（UIPI））
//! 或目标未响应时序列号不动，重注入一轮再等一个完整超时窗口后按普通
//! 获取失败上抛，不区分提权与不支持（不做提权）。快照的保全与放弃边界
//! 见快照模块文档。产物与收尾分离：恢复决策与写盘归因装在恢复收尾结构
//! 里，调用方在取材产物上抛之后收口——恢复写盘不挡在弹窗链路上。写盘
//! 轮次经取材轮次闩锁向哨兵自报计数归因（见剪贴板模块的轮次写入闩锁）。
//! 纯时序逻辑独立成函数（单测覆盖），系统接口交互归实现子模块。
//!
//! 线程约束：读取在平台事件线程上调用（与用户界面自动化（UIA）主路同线程）。

use std::time::{Duration, Instant};

mod restore;
mod snapshot;

const WRITE_CONFIRM_TIMEOUT: Duration = Duration::from_millis(800);

const WRITE_POLL_INTERVAL: Duration = Duration::from_millis(30);

fn wait_for_write(
    baseline: u64,
    mut probe: impl FnMut() -> Option<u64>,
    deadline: Instant,
) -> bool {
    loop {
        if probe().is_some_and(|current| current != baseline) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(WRITE_POLL_INTERVAL);
    }
}

mod imp {
    use std::time::Instant;

    use arboard::Clipboard;

    use gloss_core::log::{debug, thread};
    use gloss_core::model::GlossError;

    use crate::clipboard::{WatchRoundFlag, clipboard_sequence_number};
    use crate::ffi::inject;

    use super::snapshot::{PendingRestore, snapshot_board};
    use super::{WRITE_CONFIRM_TIMEOUT, WRITE_POLL_INTERVAL, wait_for_write};

    pub struct FallbackSettle {
        pending: Option<PendingRestore>,
        round: WatchRoundFlag,
        pre: u64,
        acquired: u64,
        restore: bool,
    }

    impl FallbackSettle {
        fn idle(round: WatchRoundFlag) -> Self {
            Self {
                pending: None,
                round,
                pre: 0,
                acquired: 0,
                restore: false,
            }
        }

        fn planned(
            pending: PendingRestore,
            round: WatchRoundFlag,
            pre: u64,
            acquired: bool,
            restore: bool,
        ) -> Self {
            Self {
                pending: Some(pending),
                round,
                pre,
                acquired: u64::from(acquired),
                restore,
            }
        }

        pub fn finish(mut self) {
            let Some(pending) = self.pending.as_mut() else {
                return;
            };
            let restored = if self.restore {
                pending.restore_now()
            } else {
                pending.skip_restore();
                0
            };
            self.round.enter(crate::clipboard::RoundWrites {
                pre: self.pre,
                ours: self.acquired + restored,
            });
        }

        #[cfg(test)]
        pub(super) fn restores_on_unwind(&self) -> bool {
            self.pending.is_some()
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

    #[derive(Debug, Default, Clone)]
    pub struct ClipboardFallbackReader {
        round: WatchRoundFlag,
    }

    impl ClipboardFallbackReader {
        pub fn new(round: WatchRoundFlag) -> Self {
            Self { round }
        }

        pub fn read(&mut self) -> (Result<String, GlossError>, FallbackSettle) {
            let mut clipboard = match Clipboard::new() {
                Ok(clipboard) => clipboard,
                Err(err) => {
                    debug!(
                        thread = thread::EVENT,
                        error = %err,
                        "clipboard unavailable, fallback read skipped"
                    );
                    return (
                        Err(GlossError::SelectionUnavailable),
                        FallbackSettle::idle(self.round.clone()),
                    );
                }
            };
            let pre_count = clipboard_sequence_number().unwrap_or(0);
            let Some(saved) = snapshot_board(&mut clipboard) else {
                debug!(
                    thread = thread::EVENT,
                    "original clipboard content cannot be fully preserved, fallback declined"
                );
                return (
                    Err(GlossError::SelectionUnavailable),
                    FallbackSettle::idle(self.round.clone()),
                );
            };
            let pending = PendingRestore::new(saved);
            let outcome = self.attempt(&mut clipboard);
            let settle = match &outcome {
                Ok(acquired) => {
                    let current_is_ours =
                        matches!(clipboard.get_text(), Ok(current) if &current == acquired);
                    if current_is_ours {
                        FallbackSettle::planned(pending, self.round.clone(), pre_count, true, true)
                    } else {
                        debug!(
                            thread = thread::EVENT,
                            "clipboard changed after read, restore skipped"
                        );
                        FallbackSettle::planned(pending, self.round.clone(), pre_count, true, false)
                    }
                }
                Err(_) => {
                    FallbackSettle::planned(pending, self.round.clone(), pre_count, false, false)
                }
            };
            (outcome, settle)
        }

        fn attempt(&mut self, clipboard: &mut Clipboard) -> Result<String, GlossError> {
            let Some(monitor) = ChangeMonitor::new() else {
                debug!(
                    thread = thread::EVENT,
                    "change monitor unavailable, fallback declined"
                );
                return Err(GlossError::SelectionUnavailable);
            };
            inject_copy_key()?;
            let baseline = monitor.baseline();
            let mut deadline = Instant::now() + WRITE_CONFIRM_TIMEOUT;
            let mut confirmed = wait_for_write(baseline, || monitor.generation(), deadline);
            if !confirmed {
                debug!(
                    thread = thread::EVENT,
                    "copy write not confirmed, retrying injection once"
                );
                inject_copy_key()?;
                deadline = Instant::now() + WRITE_CONFIRM_TIMEOUT;
                confirmed = wait_for_write(baseline, || monitor.generation(), deadline);
            }
            if !confirmed {
                debug!(
                    thread = thread::EVENT,
                    "copy write not confirmed within timeout"
                );
                return Err(GlossError::SelectionUnavailable);
            }
            loop {
                if let Ok(text) = clipboard.get_text()
                    && !text.is_empty()
                {
                    return Ok(text);
                }
                if Instant::now() >= deadline {
                    debug!(
                        thread = thread::EVENT,
                        "copy confirmed but no text readable within timeout"
                    );
                    return Err(GlossError::SelectionUnavailable);
                }
                std::thread::sleep(WRITE_POLL_INTERVAL);
            }
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
}

pub use imp::{ClipboardFallbackReader, FallbackSettle};

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::wait_for_write;

    #[test]
    fn seq_change_confirms_when_generation_bumps() {
        let mut calls = 0;
        let deadline = Instant::now() + Duration::from_secs(5);
        let confirmed = wait_for_write(
            0,
            || {
                calls += 1;
                Some(u64::from(calls >= 3))
            },
            deadline,
        );
        assert!(confirmed);
        assert_eq!(calls, 3);
    }

    #[test]
    fn seq_timeout_returns_false_without_hanging() {
        let start = Instant::now();
        let deadline = start + Duration::from_millis(60);
        assert!(!wait_for_write(0, || Some(0), deadline));
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "timeout path must return promptly"
        );
    }

    #[test]
    fn seq_unavailable_probe_keeps_polling_until_deadline() {
        let start = Instant::now();
        let deadline = start + Duration::from_millis(50);
        assert!(!wait_for_write(0, || None, deadline));
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "timeout path must return promptly"
        );
    }
}

#[cfg(test)]
#[allow(clippy::let_underscore_must_use)]
mod live_tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use arboard::Clipboard;

    use gloss_core::model::GlossError;

    use super::imp::ClipboardFallbackReader;
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
