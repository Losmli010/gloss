//! 模拟复制兜底流程的平台中立共享形状：兜底读取的装配骨架（板创建守卫、
//! 文本读取接线与确认超时单源）、写入确认轮询、兜底读取一轮（快照 → 注入
//! → 确认 → 读回 → 恢复收尾）、恢复重试骨架与写盘归因的收尾结构只写一份，
//! 两侧平台目录共用。本模块零目标条件编译、
//! 零外部函数接口（FFI）、零不安全块，板创建与文本读取经
//! 跨平台兜底库（arboard）触板——与共享层剪贴板文本写入同款依赖；
//! 其余平台差异只经板面原语束注入：基线
//! 计数、快照与还原、写入确认监视、注入复制快捷键。快照内容类型（系统
//! （macOS）逐条目逐风格原始字节、系统（Windows）文本或空板或图像风格）留
//! 在平台侧，快照的保全与放弃边界以各侧快照函数的文档为准。
//!
//! 流程语义：主路（辅助功能读取、用户界面自动化（UIA）读取）读不到选区时的
//! 兜底通道——快照原剪贴板，注入复制快捷键，轮询确认目标应用写入，读回；
//! 读回成功且现值未变才按快照恢复，失败轮一律不恢复——板上要么未变，要么
//! 是本轮之外的新写入或注入副产物，都不是可覆盖的内容。未确认时重注入一
//! 轮再等一个完整超时窗口。产物与收尾分离：恢复决策与写盘归因装在恢复收
//! 尾结构里，调用方在取材产物上抛之后收口——恢复写盘不挡在弹窗链路上。
//! 写盘轮次经取材轮次闩锁向哨兵自报计数归因（见剪贴板监视共享模块的轮次
//! 写入闩锁）。读取须在平台事件线程上调用，收尾同线程。
//!
//! 监视缝协议：注入前首调返回确认基线，此后每调返回代数现值，读不出返回
//! 空、继续轮询。基线与代数同源：系统（macOS）为同步标志闩锁后的 0／1，
//! 系统（Windows）为剪贴板序列号差。恢复写盘数按实际发生的写盘（系统
//! （macOS）的清板数、系统（Windows）的序列号跳变数）自报，供计数归因对
//! 齐哨兵口径。时序与预算常量随本模块单源。

use std::time::{Duration, Instant};

use arboard::{Clipboard, Error};

use gloss_core::log::{debug, thread};
use gloss_core::model::GlossError;

use super::clipboard_watch::{RoundWrites, WatchRoundFlag};

const WRITE_CONFIRM_TIMEOUT: Duration = Duration::from_millis(800);

const WRITE_POLL_INTERVAL: Duration = Duration::from_millis(30);

pub(crate) const RESTORE_ATTEMPTS: usize = 6;

pub(crate) const RESTORE_RETRY_DELAY: Duration = Duration::from_millis(200);

pub(crate) struct RestoreOutcome<Stage> {
    pub(crate) ok: bool,
    pub(crate) writes: u64,
    pub(crate) stage: Stage,
    pub(crate) status: i32,
}

pub(crate) fn restore_with_retry<Stage>(
    attempts: usize,
    delay: Duration,
    mut once: impl FnMut() -> RestoreOutcome<Stage>,
) -> RestoreOutcome<Stage>
where
    Stage: Default,
{
    let mut writes = 0;
    let mut last: Option<RestoreOutcome<Stage>> = None;
    for attempt in 0..attempts {
        let outcome = once();
        writes += outcome.writes;
        if outcome.ok {
            return RestoreOutcome {
                ok: true,
                writes,
                stage: Stage::default(),
                status: 0,
            };
        }
        last = Some(outcome);
        if attempt + 1 < attempts {
            std::thread::sleep(delay);
        }
    }
    match last {
        Some(RestoreOutcome { stage, status, .. }) => RestoreOutcome {
            ok: false,
            writes,
            stage,
            status,
        },
        None => RestoreOutcome {
            ok: false,
            writes,
            stage: Stage::default(),
            status: 0,
        },
    }
}

pub(crate) struct PendingRestore<S> {
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

pub(crate) struct FallbackSettle<S> {
    pending: Option<PendingRestore<S>>,
    round: WatchRoundFlag,
    pre: u64,
    acquired: u64,
    restore: bool,
}

impl<S> FallbackSettle<S> {
    pub(crate) fn idle(round: WatchRoundFlag) -> Self {
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
    pub(crate) fn restores_on_unwind(&self) -> bool {
        self.pending.is_some()
    }
}

pub(crate) struct FallbackBoard<S, P, Sn, M, I, T> {
    pub(crate) pre_count: P,
    pub(crate) snapshot: Sn,
    pub(crate) restore: fn(&S) -> u64,
    pub(crate) monitor: M,
    pub(crate) inject: I,
    pub(crate) read_text: T,
}

pub(crate) struct FallbackBindings<S, P, Sn, M, I> {
    pub(crate) pre_count: P,
    pub(crate) snapshot: Sn,
    pub(crate) restore: fn(&S) -> u64,
    pub(crate) monitor: M,
    pub(crate) inject: I,
}

pub(crate) fn read_with_fallback<S, P, Sn, M, I>(
    round: WatchRoundFlag,
    bindings: FallbackBindings<S, P, Sn, M, I>,
) -> (Result<String, GlossError>, FallbackSettle<S>)
where
    P: FnOnce() -> u64,
    Sn: FnOnce(Result<String, Error>) -> Option<S>,
    M: FnMut() -> Option<u64>,
    I: Fn() -> Result<(), GlossError>,
{
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
                FallbackSettle::idle(round.clone()),
            );
        }
    };
    let FallbackBindings {
        pre_count,
        snapshot,
        restore,
        monitor,
        inject,
    } = bindings;
    read_with(
        round,
        WRITE_CONFIRM_TIMEOUT,
        FallbackBoard {
            pre_count,
            snapshot,
            restore,
            monitor,
            inject,
            read_text: move || clipboard.get_text(),
        },
    )
}

pub(crate) fn read_with<S, P, Sn, M, I, T>(
    round: WatchRoundFlag,
    confirm_timeout: Duration,
    board: FallbackBoard<S, P, Sn, M, I, T>,
) -> (Result<String, GlossError>, FallbackSettle<S>)
where
    P: FnOnce() -> u64,
    Sn: FnOnce(Result<String, Error>) -> Option<S>,
    M: FnMut() -> Option<u64>,
    I: Fn() -> Result<(), GlossError>,
    T: FnMut() -> Result<String, Error>,
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
    read_text: &mut impl FnMut() -> Result<String, Error>,
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

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::time::{Duration, Instant};

    use gloss_core::model::GlossError;

    use super::super::clipboard_watch::{RoundWrites, WatchRoundFlag};

    use super::{FallbackBoard, RestoreOutcome, read_with, restore_with_retry, wait_for_write};

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

    #[derive(Debug, Default, PartialEq)]
    enum Stage {
        #[default]
        Fresh,
        Set,
    }

    fn outcome(ok: bool, writes: u64) -> RestoreOutcome<Stage> {
        RestoreOutcome {
            ok,
            writes,
            stage: Stage::Fresh,
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
        assert_eq!(outcome.writes, 1);
        assert_eq!(outcome.stage, Stage::Fresh);
        assert_eq!(outcome.status, 0);
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn retry_counts_the_writes_of_every_attempt_until_success() {
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
        assert_eq!(outcome.writes, 2);
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn exhausted_retries_carry_the_last_attempt_stage_status_and_total_writes() {
        let calls = std::cell::Cell::new(0u32);
        let outcome = restore_with_retry(2, Duration::ZERO, || {
            calls.set(calls.get() + 1);
            match calls.get() {
                1 => RestoreOutcome {
                    ok: false,
                    writes: 1,
                    stage: Stage::Fresh,
                    status: 5,
                },
                _ => RestoreOutcome {
                    ok: false,
                    writes: 0,
                    stage: Stage::Set,
                    status: 42,
                },
            }
        });
        assert!(!outcome.ok);
        assert_eq!(outcome.writes, 1);
        assert_eq!(outcome.stage, Stage::Set);
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
            "归因按恢复的实际写盘数计（重试成功会多写盘）"
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
