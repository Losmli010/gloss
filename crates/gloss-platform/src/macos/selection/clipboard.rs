//! 剪贴板兜底读取（模拟复制）：兜底读取器。
//!
//! 辅助功能（AX）读不到选区时的兜底通道：快照剪贴板全部条目与数据风格
//! 原始字节 → 注入复制快捷键 → 轮询确认目标应用写入 → 读回 →
//! 读回成功且现值未变才按
//! 快照恢复（失败轮一律不恢复——板上要么未变，要么是本轮之外的新写入
//! 或注入副产物，都不是可覆盖的内容）。快照按条目与风格尽力保全：
//! 读不出数据的风格（承诺式数据等）单独跳过——读不出即无法如实
//! 恢复，整轮放弃只会让兜底在有此类风格的板上整批失效。两类边界仍
//! 整体放弃、不注入：结构读不出（板、条目、风格名单）；板上条目的
//! 风格全部读不出——注入后原内容无法如实写回，恢复轮会清空剪贴板。
//! 未确认时重注入一轮再等一个完整超时窗口。产物与收尾分离：恢复决策与
//! 写盘归因装在恢复收尾结构里，调用方在取材产物上抛之后收口
//! ——恢复写盘不挡在弹窗链路上。写盘轮次
//! 经取材轮次闩锁向哨兵自报计数归因（见剪贴板模块的
//! 轮次写入闩锁）。纯时序逻辑独立成函数
//! （单测覆盖），系统接口交互归实现子模块。

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
    use std::time::{Duration, Instant};

    use arboard::Clipboard;

    use crate::clipboard::{RoundWrites, WatchRoundFlag, pasteboard_change_count};
    use crate::ffi::event;
    use crate::ffi::pasteboard::{K_PASTEBOARD_MODIFIED, PasteboardSynchronize};

    use gloss_core::log::{debug, thread};
    use gloss_core::model::GlossError;

    use super::restore::restore_and_log;
    use super::snapshot::{OpenPasteboard, SavedContent, open_pasteboard, snapshot_pasteboard};
    use super::{WRITE_CONFIRM_TIMEOUT, WRITE_POLL_INTERVAL, wait_for_write};

    pub struct FallbackSettle<S> {
        pending: Option<PendingRestore<S>>,
        round: WatchRoundFlag,
        pre: u64,
        acquired: u64,
        restore: bool,
    }

    impl<S> FallbackSettle<S> {
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
            pending: PendingRestore<S>,
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
            self.round.enter(RoundWrites {
                pre: self.pre,
                ours: self.acquired + restored,
            });
        }

        #[cfg(test)]
        pub(super) fn restores_on_unwind(&self) -> bool {
            self.pending.is_some()
        }
    }

    pub(super) struct PendingRestore<S> {
        saved: S,
        restore: fn(&S) -> u64,
        skip: bool,
    }

    impl<S> PendingRestore<S> {
        fn new(saved: S, restore: fn(&S) -> u64) -> Self {
            Self {
                saved,
                restore,
                skip: false,
            }
        }

        fn restore_now(&mut self) -> u64 {
            self.skip = true;
            (self.restore)(&self.saved)
        }

        fn skip_restore(&mut self) {
            self.skip = true;
        }
    }

    impl<S> Drop for PendingRestore<S> {
        fn drop(&mut self) {
            if !self.skip {
                (self.restore)(&self.saved);
            }
        }
    }

    pub(super) struct FallbackBoard<S, P, Sn, M, I, T> {
        pub(super) pre_count: P,
        pub(super) snapshot: Sn,
        pub(super) restore: fn(&S) -> u64,
        pub(super) monitor: M,
        pub(super) inject: I,
        pub(super) read_text: T,
    }

    pub(super) fn read_with<S, P, Sn, M, I, T>(
        round: WatchRoundFlag,
        confirm_timeout: Duration,
        board: FallbackBoard<S, P, Sn, M, I, T>,
    ) -> (Result<String, GlossError>, FallbackSettle<S>)
    where
        P: FnOnce() -> u64,
        Sn: FnOnce(Result<String, arboard::Error>) -> Option<S>,
        M: FnMut() -> Option<u64>,
        I: Fn() -> Result<(), GlossError>,
        T: FnMut() -> Result<String, arboard::Error>,
    {
        let FallbackBoard {
            pre_count,
            snapshot,
            restore,
            monitor,
            inject,
            mut read_text,
        } = board;
        let pre = pre_count();
        let Some(saved) = snapshot(read_text()) else {
            debug!(
                thread = thread::EVENT,
                "original clipboard content cannot be fully preserved, fallback declined"
            );
            return (
                Err(GlossError::SelectionUnavailable),
                FallbackSettle::idle(round),
            );
        };
        let pending = PendingRestore::new(saved, restore);
        let outcome = attempt(confirm_timeout, &mut read_text, monitor, inject);
        let settle = match &outcome {
            Ok(acquired) => {
                let current_is_ours = matches!(read_text(), Ok(current) if &current == acquired);
                if current_is_ours {
                    FallbackSettle::planned(pending, round, pre, true, true)
                } else {
                    debug!(
                        thread = thread::EVENT,
                        "clipboard changed after read, restore skipped"
                    );
                    FallbackSettle::planned(pending, round, pre, true, false)
                }
            }
            Err(_) => FallbackSettle::planned(pending, round, pre, false, false),
        };
        (outcome, settle)
    }

    fn attempt(
        confirm_timeout: Duration,
        read_text: &mut impl FnMut() -> Result<String, arboard::Error>,
        mut monitor: impl FnMut() -> Option<u64>,
        inject: impl Fn() -> Result<(), GlossError>,
    ) -> Result<String, GlossError> {
        let Some(baseline) = monitor() else {
            debug!(
                thread = thread::EVENT,
                "change monitor unavailable, fallback declined"
            );
            return Err(GlossError::SelectionUnavailable);
        };
        inject()?;
        let mut deadline = Instant::now() + confirm_timeout;
        let mut confirmed = wait_for_write(baseline, &mut monitor, deadline);
        if !confirmed {
            debug!(
                thread = thread::EVENT,
                "copy write not confirmed, retrying injection once"
            );
            inject()?;
            deadline = Instant::now() + confirm_timeout;
            confirmed = wait_for_write(baseline, &mut monitor, deadline);
        }
        if !confirmed {
            debug!(
                thread = thread::EVENT,
                "copy write not confirmed within timeout"
            );
            return Err(GlossError::SelectionUnavailable);
        }
        loop {
            if let Ok(text) = read_text()
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

    struct ChangeMonitor {
        pb: OpenPasteboard,
        latched: bool,
    }

    impl ChangeMonitor {
        fn new() -> Option<Self> {
            let pb = open_pasteboard()?;
            // SAFETY: `raw` is a valid +1 reference that stays alive until this
            // struct is destroyed.
            unsafe { PasteboardSynchronize(pb.raw) };
            Some(Self { pb, latched: false })
        }

        fn baseline(&self) -> u64 {
            0
        }

        fn generation(&mut self) -> u64 {
            // SAFETY: `raw` is a valid +1 reference that stays alive until this
            // struct is destroyed.
            let flags = unsafe { PasteboardSynchronize(self.pb.raw) };
            if flags & K_PASTEBOARD_MODIFIED != 0 {
                self.latched = true;
            }
            u64::from(self.latched)
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

        pub(crate) fn read(
            &mut self,
        ) -> (Result<String, GlossError>, FallbackSettle<SavedContent>) {
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
            read_with(
                self.round.clone(),
                WRITE_CONFIRM_TIMEOUT,
                FallbackBoard {
                    pre_count: || pasteboard_change_count().unwrap_or(0),
                    snapshot: |_| snapshot_pasteboard(),
                    restore: restore_and_log,
                    monitor: {
                        let mut monitor: Option<ChangeMonitor> = None;
                        move || {
                            let fresh = monitor.is_none();
                            if fresh {
                                monitor = ChangeMonitor::new();
                            }
                            let change = monitor.as_mut()?;
                            Some(if fresh {
                                change.baseline()
                            } else {
                                change.generation()
                            })
                        }
                    },
                    inject: inject_copy_key,
                    read_text: move || clipboard.get_text(),
                },
            )
        }
    }

    pub(super) fn keys_to_release(sequence: &[(u16, bool, u64)], index: usize) -> Vec<u16> {
        sequence[..index]
            .iter()
            .rev()
            .filter(|(_, key_down, _)| *key_down)
            .map(|(key, _, _)| *key)
            .collect()
    }

    fn inject_copy_key() -> Result<(), GlossError> {
        let Some(source) = event::new_hid_source() else {
            debug!(
                thread = thread::EVENT,
                "copy key injection failed: no event source"
            );
            return Err(GlossError::SelectionUnavailable);
        };
        let cmd = event::K_CG_EVENT_FLAG_MASK_COMMAND;
        let sequence = [
            (event::VK_COMMAND, true, cmd),
            (event::VK_ANSI_C, true, cmd),
            (event::VK_ANSI_C, false, cmd),
            (event::VK_COMMAND, false, 0),
        ];
        for (index, &(virtual_key, key_down, flags)) in sequence.iter().enumerate() {
            if !event::post_keyboard_event(&source, virtual_key, key_down, flags) {
                debug!(
                    thread = thread::EVENT,
                    virtual_key, key_down, "copy key injection failed"
                );
                for pressed in keys_to_release(&sequence, index) {
                    let _ = event::post_keyboard_event(&source, pressed, false, 0);
                }
                return Err(GlossError::SelectionUnavailable);
            }
        }
        Ok(())
    }
}

pub use imp::{ClipboardFallbackReader, FallbackSettle};

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::time::{Duration, Instant};

    use gloss_core::model::GlossError;

    use crate::clipboard::{RoundWrites, WatchRoundFlag};

    use super::imp::{FallbackBoard, keys_to_release, read_with};
    use super::restore::{RestoreOutcome, RestoreStage, restore_with_retry};
    use super::wait_for_write;

    #[derive(Debug, PartialEq)]
    struct Snapshot(&'static str);

    thread_local! {
        static RESTORE_LOG: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
        static RESTORE_WRITES: Cell<u64> = const { Cell::new(1) };
    }

    fn recorded_restore(saved: &Snapshot) -> u64 {
        RESTORE_LOG.with(|log| log.borrow_mut().push(saved.0));
        RESTORE_WRITES.with(Cell::get)
    }

    fn restore_log() -> Vec<&'static str> {
        RESTORE_LOG.with(|log| log.borrow().clone())
    }

    fn with_restore_writes(writes: u64) {
        RESTORE_WRITES.with(|cell| cell.set(writes));
    }

    fn confirmed_monitor() -> impl FnMut() -> Option<u64> {
        let calls = Cell::new(0u32);
        move || {
            let call = calls.get();
            calls.set(call + 1);
            Some(u64::from(call > 0))
        }
    }

    fn outcome(ok: bool, clears: u64) -> RestoreOutcome {
        RestoreOutcome {
            ok,
            clears,
            stage: RestoreStage::Open,
            status: 0,
        }
    }

    #[test]
    fn retry_stops_at_the_first_success() {
        let calls = std::cell::Cell::new(0u32);
        let outcome = restore_with_retry(3, Duration::ZERO, || {
            calls.set(calls.get() + 1);
            outcome(true, 1)
        });
        assert!(outcome.ok);
        assert_eq!(outcome.clears, 1);
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn retry_counts_the_clears_of_every_attempt_until_success() {
        let calls = std::cell::Cell::new(0u32);
        let outcome = restore_with_retry(3, Duration::ZERO, || {
            calls.set(calls.get() + 1);
            match calls.get() {
                1 => outcome(false, 1),
                2 => outcome(false, 0),
                _ => outcome(true, 1),
            }
        });
        assert!(outcome.ok);
        assert_eq!(outcome.clears, 2);
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn exhausted_retries_carry_the_last_attempt_stage_status_and_total_clears() {
        let calls = std::cell::Cell::new(0u32);
        let outcome = restore_with_retry(2, Duration::ZERO, || {
            calls.set(calls.get() + 1);
            match calls.get() {
                1 => RestoreOutcome {
                    ok: false,
                    clears: 1,
                    stage: RestoreStage::Alloc,
                    status: 5,
                },
                _ => RestoreOutcome {
                    ok: false,
                    clears: 0,
                    stage: RestoreStage::Clear,
                    status: 42,
                },
            }
        });
        assert!(!outcome.ok);
        assert_eq!(outcome.clears, 1);
        assert_eq!(outcome.stage, RestoreStage::Clear);
        assert_eq!(outcome.status, 42);
    }

    #[test]
    fn change_is_detected_when_generation_bumps() {
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
    fn timeout_returns_false_without_hanging() {
        let start = Instant::now();
        let deadline = start + Duration::from_millis(60);
        assert!(!wait_for_write(0, || Some(0), deadline));
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "timeout path must return promptly"
        );
    }

    #[test]
    fn unavailable_probe_keeps_polling_until_deadline() {
        let start = Instant::now();
        let deadline = start + Duration::from_millis(50);
        assert!(!wait_for_write(0, || None, deadline));
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "timeout path must return promptly"
        );
    }

    #[test]
    fn injection_failure_releases_every_pressed_key_in_reverse() {
        let sequence = [
            (1u16, true, 0u64),
            (2, true, 0),
            (2, false, 0),
            (1, false, 0),
        ];
        assert_eq!(
            keys_to_release(&sequence, 0),
            Vec::<u16>::new(),
            "第一个事件失败时还没有键按下"
        );
        assert_eq!(
            keys_to_release(&sequence, 1),
            vec![1],
            "C 按下失败：只需补放修饰键"
        );
        assert_eq!(
            keys_to_release(&sequence, 2),
            vec![2, 1],
            "C 释放失败：先补 C 再补修饰键（只放修饰键会留下 C 按下态）"
        );
        assert_eq!(
            keys_to_release(&sequence, 3),
            vec![2, 1],
            "修饰键释放失败：C 已释放，重复 key-up 幂等"
        );
    }

    #[test]
    fn a_confirmed_write_reads_back_and_restores_with_full_attribution() {
        let inject_calls = Cell::new(0u32);
        let round = WatchRoundFlag::default();
        let (outcome, settle) = read_with(
            round.clone(),
            Duration::ZERO,
            FallbackBoard {
                pre_count: || 7,
                snapshot: |_| Some(Snapshot("saved-content")),
                restore: recorded_restore,
                monitor: confirmed_monitor(),
                inject: || {
                    inject_calls.set(inject_calls.get() + 1);
                    Ok(())
                },
                read_text: || Ok("the selection".to_owned()),
            },
        );
        assert_eq!(outcome, Ok("the selection".to_owned()));
        assert_eq!(inject_calls.get(), 1);
        assert!(settle.restores_on_unwind());
        settle.finish();
        assert_eq!(
            round.peek(),
            Some(RoundWrites { pre: 7, ours: 2 }),
            "归因 = 确认的注入复制 1 加恢复写盘 1"
        );
        assert_eq!(restore_log(), vec!["saved-content"]);
    }

    #[test]
    fn restore_writes_feed_the_round_attribution() {
        with_restore_writes(2);
        let round = WatchRoundFlag::default();
        let (outcome, settle) = read_with(
            round.clone(),
            Duration::ZERO,
            FallbackBoard {
                pre_count: || 7,
                snapshot: |_| Some(Snapshot("saved-content")),
                restore: recorded_restore,
                monitor: confirmed_monitor(),
                inject: || Ok(()),
                read_text: || Ok("the selection".to_owned()),
            },
        );
        assert_eq!(outcome, Ok("the selection".to_owned()));
        settle.finish();
        assert_eq!(
            round.peek(),
            Some(RoundWrites { pre: 7, ours: 3 }),
            "归因按恢复的实际写盘数计（重试成功会多清板）"
        );
    }

    #[test]
    fn an_unconfirmed_write_reinjects_once_and_skips_restore() {
        let inject_calls = Cell::new(0u32);
        let monitor_calls = Cell::new(0u32);
        let text_calls = Cell::new(0u32);
        let round = WatchRoundFlag::default();
        let (outcome, settle) = read_with(
            round.clone(),
            Duration::ZERO,
            FallbackBoard {
                pre_count: || 7,
                snapshot: |_| Some(Snapshot("saved-content")),
                restore: recorded_restore,
                monitor: || {
                    monitor_calls.set(monitor_calls.get() + 1);
                    Some(0u64)
                },
                inject: || {
                    inject_calls.set(inject_calls.get() + 1);
                    Ok(())
                },
                read_text: || {
                    text_calls.set(text_calls.get() + 1);
                    Ok("on the board".to_owned())
                },
            },
        );
        assert_eq!(outcome, Err(GlossError::SelectionUnavailable));
        assert_eq!(inject_calls.get(), 2, "未确认时重注入一轮");
        assert_eq!(monitor_calls.get(), 3, "首调基线加每窗一轮探");
        assert_eq!(text_calls.get(), 1, "未确认不进读回轮询");
        assert!(settle.restores_on_unwind());
        settle.finish();
        assert_eq!(
            round.peek(),
            Some(RoundWrites { pre: 7, ours: 0 }),
            "失败轮不恢复、写盘归因为零"
        );
        assert_eq!(restore_log(), Vec::<&str>::new());
    }

    #[test]
    fn an_empty_read_back_after_confirmation_stays_unrestored() {
        let inject_calls = Cell::new(0u32);
        let text_calls = Cell::new(0u32);
        let round = WatchRoundFlag::default();
        let (outcome, settle) = read_with(
            round.clone(),
            Duration::ZERO,
            FallbackBoard {
                pre_count: || 7,
                snapshot: |_| Some(Snapshot("saved-content")),
                restore: recorded_restore,
                monitor: confirmed_monitor(),
                inject: || {
                    inject_calls.set(inject_calls.get() + 1);
                    Ok(())
                },
                read_text: || {
                    text_calls.set(text_calls.get() + 1);
                    Ok(String::new())
                },
            },
        );
        assert_eq!(outcome, Err(GlossError::SelectionUnavailable));
        assert_eq!(inject_calls.get(), 1);
        assert_eq!(text_calls.get(), 2, "读回轮询一轮后随期限收口");
        settle.finish();
        assert_eq!(
            round.peek(),
            Some(RoundWrites { pre: 7, ours: 0 }),
            "确认了写入却读不出文本按外来放行，不恢复"
        );
        assert_eq!(restore_log(), Vec::<&str>::new());
    }

    #[test]
    fn a_board_changed_after_read_skips_restore_but_keeps_the_acquisition() {
        let text_calls = Cell::new(0u32);
        let round = WatchRoundFlag::default();
        let (outcome, settle) = read_with(
            round.clone(),
            Duration::ZERO,
            FallbackBoard {
                pre_count: || 7,
                snapshot: |_| Some(Snapshot("saved-content")),
                restore: recorded_restore,
                monitor: confirmed_monitor(),
                inject: || Ok(()),
                read_text: || {
                    let call = text_calls.get();
                    text_calls.set(call + 1);
                    match call {
                        0 => Ok("preset".to_owned()),
                        1 => Ok("the selection".to_owned()),
                        _ => Ok("the user's copy".to_owned()),
                    }
                },
            },
        );
        assert_eq!(outcome, Ok("the selection".to_owned()));
        settle.finish();
        assert_eq!(
            round.peek(),
            Some(RoundWrites { pre: 7, ours: 1 }),
            "现值被改只计注入复制，恢复跳过"
        );
        assert_eq!(
            restore_log(),
            Vec::<&str>::new(),
            "用户窗口期的新拷贝不被覆盖"
        );
    }

    #[test]
    fn a_declined_snapshot_stays_idle_without_injecting() {
        let inject_calls = Cell::new(0u32);
        let monitor_calls = Cell::new(0u32);
        let text_calls = Cell::new(0u32);
        let round = WatchRoundFlag::default();
        let (outcome, settle) = read_with(
            round.clone(),
            Duration::ZERO,
            FallbackBoard {
                pre_count: || 7,
                snapshot: |_| None,
                restore: recorded_restore,
                monitor: || {
                    monitor_calls.set(monitor_calls.get() + 1);
                    Some(0u64)
                },
                inject: || {
                    inject_calls.set(inject_calls.get() + 1);
                    Ok(())
                },
                read_text: || {
                    text_calls.set(text_calls.get() + 1);
                    Ok("on the board".to_owned())
                },
            },
        );
        assert_eq!(outcome, Err(GlossError::SelectionUnavailable));
        assert_eq!(inject_calls.get(), 0, "快照放弃轮不注入");
        assert_eq!(monitor_calls.get(), 0);
        assert_eq!(text_calls.get(), 1);
        assert!(!settle.restores_on_unwind(), "放弃轮板未被触碰，无恢复收尾");
        settle.finish();
        assert_eq!(round.peek(), None, "放弃轮不进写盘归因");
        assert_eq!(restore_log(), Vec::<&str>::new());
    }

    #[test]
    fn dropping_a_settle_without_finish_restores_the_snapshot() {
        let round = WatchRoundFlag::default();
        let (outcome, settle) = read_with(
            round.clone(),
            Duration::ZERO,
            FallbackBoard {
                pre_count: || 7,
                snapshot: |_| Some(Snapshot("saved-content")),
                restore: recorded_restore,
                monitor: confirmed_monitor(),
                inject: || Ok(()),
                read_text: || Ok("the selection".to_owned()),
            },
        );
        assert_eq!(outcome, Ok("the selection".to_owned()));
        drop(settle);
        assert_eq!(
            restore_log(),
            vec!["saved-content"],
            "未显式收口时 Drop 兜底按快照恢复"
        );
        assert_eq!(round.peek(), None, "Drop 兜底不进写盘归因");
    }

    #[test]
    fn an_unavailable_monitor_declines_without_injecting() {
        let inject_calls = Cell::new(0u32);
        let monitor_calls = Cell::new(0u32);
        let round = WatchRoundFlag::default();
        let (outcome, settle) = read_with(
            round.clone(),
            Duration::ZERO,
            FallbackBoard {
                pre_count: || 7,
                snapshot: |_| Some(Snapshot("saved-content")),
                restore: recorded_restore,
                monitor: || {
                    monitor_calls.set(monitor_calls.get() + 1);
                    None
                },
                inject: || {
                    inject_calls.set(inject_calls.get() + 1);
                    Ok(())
                },
                read_text: || Ok("on the board".to_owned()),
            },
        );
        assert_eq!(outcome, Err(GlossError::SelectionUnavailable));
        assert_eq!(monitor_calls.get(), 1, "基线建立失败即放弃");
        assert_eq!(inject_calls.get(), 0);
        settle.finish();
        assert_eq!(
            round.peek(),
            Some(RoundWrites { pre: 7, ours: 0 }),
            "监视不可用按失败轮收口，不恢复"
        );
        assert_eq!(restore_log(), Vec::<&str>::new());
    }

    #[test]
    fn a_failed_injection_skips_restore_with_zero_attribution() {
        let inject_calls = Cell::new(0u32);
        let round = WatchRoundFlag::default();
        let (outcome, settle) = read_with(
            round.clone(),
            Duration::ZERO,
            FallbackBoard {
                pre_count: || 7,
                snapshot: |_| Some(Snapshot("saved-content")),
                restore: recorded_restore,
                monitor: confirmed_monitor(),
                inject: || {
                    inject_calls.set(inject_calls.get() + 1);
                    Err(GlossError::SelectionUnavailable)
                },
                read_text: || Ok("on the board".to_owned()),
            },
        );
        assert_eq!(outcome, Err(GlossError::SelectionUnavailable));
        assert_eq!(inject_calls.get(), 1, "注入失败即弃，不重注入");
        settle.finish();
        assert_eq!(
            round.peek(),
            Some(RoundWrites { pre: 7, ours: 0 }),
            "注入失败轮不恢复、写盘归因为零"
        );
        assert_eq!(restore_log(), Vec::<&str>::new());
    }
}

#[cfg(test)]
#[allow(clippy::let_underscore_must_use)]
mod live_tests {
    use std::ffi::{CStr, c_void};
    use std::sync::Mutex;
    use std::time::Duration;

    use arboard::Clipboard;

    use crate::ffi::cf::{self, CFDataGetBytePtr, CFDataGetLength, CFDataRef, CFIndex, CfGuard};
    use crate::ffi::pasteboard::{
        CLIPBOARD_NAME, PasteboardClear, PasteboardCopyItemFlavorData, PasteboardCreate,
        PasteboardGetItemIdentifier, PasteboardItemID, PasteboardPutItemFlavor, PasteboardRef,
        PasteboardSynchronize,
    };

    use super::imp::ClipboardFallbackReader;
    use crate::clipboard::{WatchRoundFlag, pasteboard_change_count};

    static CLIPBOARD_LIVE_LOCK: Mutex<()> = Mutex::new(());

    const UNWIND_RESTORE_EVENT_TIMEOUT: Duration = Duration::from_secs(5);
    const UNWIND_RESTORE_EVENT_POLL: Duration = Duration::from_millis(50);

    #[test]
    fn fallback_read_restores_original_clipboard() {
        let _clipboard = CLIPBOARD_LIVE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut clipboard = Clipboard::new().expect("clipboard should be available");
        let preset = format!("gloss-restore-{}", std::process::id());
        clipboard.set_text(preset.clone()).expect("preset text");

        let round = WatchRoundFlag::default();
        let mut reader = ClipboardFallbackReader::new(round.clone());
        let (_, settle) = reader.read();
        settle.finish();

        let restored = clipboard.get_text().expect("clipboard should be readable");
        assert!(
            restored == preset,
            "original clipboard content must survive (got {} bytes)",
            restored.len()
        );
        let writes = round.peek().expect("the round must report its writes");
        assert!(
            matches!(writes.ours, 0 | 2),
            "ours must be 0 (failed round) or 2 (acquired + restored), got {}",
            writes.ours
        );
        assert!(
            pasteboard_change_count().is_none_or(|current| current >= writes.pre),
            "the round pre count must not exceed the current count"
        );
    }

    #[test]
    fn fallback_read_restores_multiflavor_clipboard() {
        let _clipboard = CLIPBOARD_LIVE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let preset_text = "gloss-multiflavor-preset";
        let custom_flavor: &CStr = c"org.gloss.test.multiflavor";
        let payload = b"\x00\x01\xfe\xff preset payload";
        assert!(
            preset_items(&[(preset_text, custom_flavor, payload)]),
            "preset write should succeed"
        );

        let mut reader = ClipboardFallbackReader::new(WatchRoundFlag::default());
        let (_, settle) = reader.read();
        settle.finish();

        let mut clipboard = Clipboard::new().expect("clipboard should be available");
        let text = clipboard
            .get_text()
            .expect("text flavor should be readable");
        assert!(
            text == preset_text,
            "text flavor must survive verbatim (got {} bytes)",
            text.len()
        );
        assert_eq!(
            read_flavor_data(1, custom_flavor).as_deref(),
            Some(&payload[..]),
            "custom flavor must survive byte-for-byte"
        );
    }

    #[test]
    fn fallback_read_restores_empty_clipboard() {
        let _clipboard = CLIPBOARD_LIVE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(clear_pasteboard_for_test(), "preset clear should succeed");

        let mut reader = ClipboardFallbackReader::new(WatchRoundFlag::default());
        let (_, settle) = reader.read();
        settle.finish();

        let mut clipboard = Clipboard::new().expect("clipboard should be available");
        assert!(
            matches!(
                clipboard.get_text(),
                Err(arboard::Error::ContentNotAvailable)
            ),
            "clipboard must still be empty after the fallback round trip"
        );
    }

    #[test]
    fn fallback_read_restores_multi_item_clipboard() {
        let _clipboard = CLIPBOARD_LIVE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let flavor_x: &CStr = c"org.gloss.test.item-x";
        let flavor_y: &CStr = c"org.gloss.test.item-y";
        assert!(
            preset_items(&[
                ("gloss-item-one", flavor_x, b"payload x".as_slice()),
                ("gloss-item-two", flavor_y, b"payload y".as_slice()),
            ]),
            "preset write should succeed"
        );

        let mut reader = ClipboardFallbackReader::new(WatchRoundFlag::default());
        let (_, settle) = reader.read();
        settle.finish();

        assert_eq!(
            read_flavor_data(1, c"public.utf8-plain-text").as_deref(),
            Some(&b"gloss-item-one"[..]),
            "item 1 text flavor must merge back into item 1"
        );
        assert_eq!(
            read_flavor_data(1, flavor_x).as_deref(),
            Some(&b"payload x"[..]),
            "item 1 custom flavor must merge back into item 1"
        );
        assert_eq!(
            read_flavor_data(2, c"public.utf8-plain-text").as_deref(),
            Some(&b"gloss-item-two"[..]),
            "item 2 text flavor must merge back into item 2"
        );
        assert_eq!(
            read_flavor_data(2, flavor_y).as_deref(),
            Some(&b"payload y"[..]),
            "item 2 custom flavor must merge back into item 2"
        );
    }

    fn wait_for_unwind_restore_event(
        clipboard: &mut Clipboard,
        preset: &str,
        restore_log: &gloss_core::log::CaptureReader,
        log_before_len: usize,
    ) -> (bool, String) {
        let deadline = std::time::Instant::now() + UNWIND_RESTORE_EVENT_TIMEOUT;
        let mut trail = String::new();
        loop {
            if matches!(clipboard.get_text(), Ok(text) if text == preset) {
                return (true, trail);
            }
            trail = restore_log.text()[log_before_len..].to_owned();
            if trail.contains("failed to restore clipboard") {
                return (true, trail);
            }
            if std::time::Instant::now() >= deadline {
                return (false, trail);
            }
            std::thread::sleep(UNWIND_RESTORE_EVENT_POLL);
        }
    }

    #[test]
    fn fallback_read_unwind_restores_the_clipboard_without_finish() {
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

        let restore_log = gloss_core::log::capture_global();
        let log_before_len = restore_log.text().len();
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

        let (settled, trail) =
            wait_for_unwind_restore_event(&mut clipboard, &preset, &restore_log, log_before_len);
        assert!(
            settled,
            "the unwind Drop must restore the snapshot or leave the failure trail; a \
             pasteboard service outage may exhaust the restore retries, and then the \
             failure trail in the log is the contracted observable (outcome ok={}, \
             restore trail in window: {trail})",
            outcome.is_ok()
        );
    }

    fn preset_items(items: &[(&str, &CStr, &[u8])]) -> bool {
        let Some(name_ref) = cf::cf_string(CLIPBOARD_NAME) else {
            return false;
        };
        let mut pb: PasteboardRef = std::ptr::null_mut();
        // SAFETY: `name_ref` is a valid CFString reference, and the out parameter
        // points to a stack variable.
        let status = unsafe { PasteboardCreate(name_ref.string_ref(), &mut pb) };
        if status != 0 || pb.is_null() {
            return false;
        }
        let _pb = CfGuard::new(pb as *const c_void);

        // SAFETY: `pb` is a valid +1 reference.
        let status = unsafe { PasteboardClear(pb) };
        if status != 0 {
            return false;
        }
        for (index, (text, flavor, data)) in items.iter().enumerate() {
            let item_id = (index + 1) as PasteboardItemID;
            if !put_text_and_flavor_for_test(pb, item_id, text, flavor, data) {
                return false;
            }
        }
        true
    }

    fn put_text_and_flavor_for_test(
        pb: PasteboardRef,
        item_id: PasteboardItemID,
        text: &str,
        flavor: &CStr,
        data: &[u8],
    ) -> bool {
        let (Some(text_ref), Some(flavor_ref), Some(text_data), Some(flavor_data)) = (
            cf::cf_string(c"public.utf8-plain-text"),
            cf::cf_string(flavor),
            cf::cf_data(text.as_bytes()),
            cf::cf_data(data),
        ) else {
            return false;
        };
        // SAFETY: the guards for `pb` and the four references all stay alive until
        // the end of the function, and `Put` does not take ownership of the
        // references.
        let status = unsafe {
            PasteboardPutItemFlavor(pb, item_id, text_ref.string_ref(), text_data.data_ref(), 0)
        };
        if status != 0 {
            return false;
        }
        // SAFETY: same as above, the references are still held by their guards.
        let status = unsafe {
            PasteboardPutItemFlavor(
                pb,
                item_id,
                flavor_ref.string_ref(),
                flavor_data.data_ref(),
                0,
            )
        };
        status == 0
    }

    fn clear_pasteboard_for_test() -> bool {
        let Some(name_ref) = cf::cf_string(CLIPBOARD_NAME) else {
            return false;
        };
        let mut pb: PasteboardRef = std::ptr::null_mut();
        // SAFETY: `name_ref` is a valid CFString reference, and the out parameter
        // points to a stack variable.
        let status = unsafe { PasteboardCreate(name_ref.string_ref(), &mut pb) };
        if status != 0 || pb.is_null() {
            return false;
        }
        let _pb = CfGuard::new(pb as *const _);
        // SAFETY: `pb` is a valid +1 reference.
        let status = unsafe { PasteboardClear(pb) };
        status == 0
    }

    fn read_flavor_data(item_index: CFIndex, flavor: &CStr) -> Option<Vec<u8>> {
        let name_ref = cf::cf_string(CLIPBOARD_NAME)?;
        let mut pb: PasteboardRef = std::ptr::null_mut();
        // SAFETY: `name_ref` is a valid CFString reference, and the out parameter
        // points to a stack variable.
        let status = unsafe { PasteboardCreate(name_ref.string_ref(), &mut pb) };
        if status != 0 || pb.is_null() {
            return None;
        }
        let _pb = CfGuard::new(pb as *const _);
        // SAFETY: `pb` is a valid +1 reference, and the local reference may lag behind
        // the global board, so synchronize first.
        unsafe { PasteboardSynchronize(pb) };
        let mut id: PasteboardItemID = std::ptr::null_mut();
        // SAFETY: `pb` is a valid +1 reference, and the out parameter points to a
        // stack variable.
        let status = unsafe { PasteboardGetItemIdentifier(pb, item_index, &mut id) };
        if status != 0 {
            return None;
        }
        let flavor_ref = cf::cf_string(flavor)?;
        let mut data: CFDataRef = std::ptr::null();
        // SAFETY: `pb`/`id` and `flavor_ref` are all valid references, and the out
        // parameter points to a stack variable.
        let status =
            unsafe { PasteboardCopyItemFlavorData(pb, id, flavor_ref.string_ref(), &mut data) };
        if status != 0 || data.is_null() {
            return None;
        }
        // SAFETY: `data` is a valid CFData reference just copied, with the length
        // and the pointer coming from the same object.
        let (len, bytes_ptr) = unsafe { (CFDataGetLength(data), CFDataGetBytePtr(data)) };
        let Ok(len) = usize::try_from(len) else {
            return None;
        };
        if len == 0 {
            return Some(Vec::new());
        }
        // SAFETY: `bytes_ptr` is non-NULL (guaranteed by CFData with a non-zero
        // length), and the readable byte count is exactly `len`.
        let bytes = unsafe { std::slice::from_raw_parts(bytes_ptr, len) };
        Some(bytes.to_vec())
    }

    #[test]
    #[ignore = "需授权真机：先把运行测试的终端 App 加入 系统设置→隐私与                安全性→辅助功能（未授权时由 live_test_support 快速失败）"]
    fn reads_live_selection_via_simulated_copy() {
        let _clipboard = CLIPBOARD_LIVE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        crate::live_test_support::require_accessibility("clipboard_simulated_copy");
        let mut reader = ClipboardFallbackReader::new(WatchRoundFlag::default());
        let (outcome, settle) = reader.read();
        settle.finish();
        let text = outcome.expect("fallback should read the live selection");
        println!("selected: {text}");
        assert!(!text.is_empty());
    }

    #[test]
    #[ignore = "需授权真机：先把运行测试的终端 App 加入 系统设置→隐私与                安全性→辅助功能，并在前台应用里选中文字"]
    fn reads_live_selection_from_rich_clipboard_via_simulated_copy() {
        let _clipboard = CLIPBOARD_LIVE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        crate::live_test_support::require_accessibility("clipboard_rich_clipboard_fallback");
        let preset_text = "gloss-rich-fallback-preset";
        let custom_flavor: &CStr = c"org.gloss.test.richfallback";
        let payload = b"rich fallback payload".as_slice();
        assert!(
            preset_items(&[(preset_text, custom_flavor, payload)]),
            "preset write should succeed"
        );

        let mut reader = ClipboardFallbackReader::new(WatchRoundFlag::default());
        let (outcome, settle) = reader.read();
        settle.finish();
        let text = outcome.expect("rich clipboard must not decline the fallback");
        assert!(!text.is_empty(), "fallback should read the live selection");

        let mut clipboard = Clipboard::new().expect("clipboard should be available");
        let restored = clipboard
            .get_text()
            .expect("text flavor should be readable");
        assert!(
            restored == preset_text,
            "text flavor must be restored verbatim (got {} bytes)",
            restored.len()
        );
        assert_eq!(
            read_flavor_data(1, custom_flavor).as_deref(),
            Some(payload),
            "custom flavor must be restored byte-for-byte"
        );
    }
}
