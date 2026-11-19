//! 剪贴板图片哨兵源的平台中立共享形状：基线武装、查询节流与取材轮次的
//! 计数归因只写一份，两侧平台目录共用；平台侧只提供“读变更计数”与
//! “是否有图”两个原语，沿用时间与读数的闭包注入形态进入轮询。本模块零
//! 目标条件编译、零外部函数接口（FFI）、零不安全块，不触任何系统接口。
//!
//! 随平台事件线程的节拍轮询（事件源契约），源内自节流：计数读数是纯元数据
//! 查询，前进后再查一次是否存在图像才产出观察事件——纯文本复制不触发、
//! 不占代数。构造时记一次计数作基线，启动时的既有剪贴板内容不触发；开关
//! （共享原子布尔）关时整源不查询（零开销），重开时重记基线——关闭期间的
//! 旧内容在重开后不触发。取材轮次（轮次写入标记）写盘后按“计数归因”消费：
//! 本应用写入吞掉、外来写入触发是防误触发的核心不变量——超出本轮自报写盘
//! 数的变化是轮次内外来写入（用户的真实复制），照常触发；其余（注入复制与
//! 恢复写回等本应用副产物）吞掉并重记基线。

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasteboardImageObserved;

#[derive(Debug, Clone, Default)]
pub struct PasteboardObserver {
    count: Arc<AtomicU64>,
}

impl PasteboardObserver {
    pub(crate) fn record(&self, count: u64) {
        self.count.store(count, Ordering::Relaxed);
    }

    pub fn matches(&self, current: u64) -> bool {
        self.count.load(Ordering::Relaxed) == current
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoundWrites {
    pub pre: u64,
    pub ours: u64,
}

#[derive(Debug, Clone, Default)]
pub struct WatchRoundFlag {
    pending: Arc<Mutex<Option<RoundWrites>>>,
}

impl WatchRoundFlag {
    pub fn enter(&self, writes: RoundWrites) {
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *pending = Some(match pending.take() {
            Some(prev) => RoundWrites {
                pre: prev.pre.min(writes.pre),
                ours: prev.ours + writes.ours,
            },
            None => writes,
        });
    }

    fn take(&self) -> Option<RoundWrites> {
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
    }

    #[cfg(test)]
    pub(crate) fn peek(&self) -> Option<RoundWrites> {
        let pending = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *pending
    }
}

pub struct ClipboardWatchSource {
    enabled: Arc<AtomicBool>,
    observer: PasteboardObserver,
    round: WatchRoundFlag,
    seen: Option<u64>,
    next_query: Option<Instant>,
    interval: Duration,
}

impl ClipboardWatchSource {
    pub(crate) fn arm(
        enabled: Arc<AtomicBool>,
        baseline_count: impl FnOnce() -> Option<u64>,
    ) -> Self {
        let mut source = Self {
            enabled,
            observer: PasteboardObserver::default(),
            round: WatchRoundFlag::default(),
            seen: None,
            next_query: None,
            interval: POLL_INTERVAL,
        };
        if let Some(count) = baseline_count() {
            source.seen = Some(count);
        }
        source
    }

    pub fn observer(&self) -> PasteboardObserver {
        self.observer.clone()
    }

    pub fn round_flag(&self) -> WatchRoundFlag {
        self.round.clone()
    }

    pub(crate) fn poll_at(
        &mut self,
        now: Instant,
        mut count: impl FnMut() -> Option<u64>,
        has_image: impl FnOnce() -> bool,
    ) -> Vec<PasteboardImageObserved> {
        if let Some(writes) = self.round.take() {
            let Some(current) = count() else {
                self.seen = None;
                return Vec::new();
            };
            let baseline = self.seen.unwrap_or(writes.pre);
            if current.saturating_sub(baseline) > writes.ours {
            } else {
                self.seen = Some(current);
                return Vec::new();
            }
        }
        if !self.enabled.load(Ordering::Relaxed) {
            self.seen = None;
            return Vec::new();
        }
        let Some(baseline) = self.seen else {
            self.seen = count();
            return Vec::new();
        };
        if self.next_query.is_some_and(|next| now < next) {
            return Vec::new();
        }
        self.next_query = Some(now + self.interval);
        let Some(current) = count() else {
            return Vec::new();
        };
        if current == baseline {
            return Vec::new();
        }
        self.seen = Some(current);
        if !has_image() {
            return Vec::new();
        }
        self.observer.record(current);
        vec![PasteboardImageObserved]
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn source(enabled: bool, seen: u64) -> ClipboardWatchSource {
        ClipboardWatchSource {
            enabled: Arc::new(AtomicBool::new(enabled)),
            observer: PasteboardObserver::default(),
            round: WatchRoundFlag::default(),
            seen: Some(seen),
            next_query: None,
            interval: POLL_INTERVAL,
        }
    }

    fn t(seconds: u64) -> Instant {
        Instant::now() + Duration::from_secs(seconds)
    }

    #[test]
    fn advance_with_an_image_fires_once_and_records_the_change() {
        let mut watch = source(true, 7);
        let events = watch.poll_at(t(0), || Some(8), || true);
        assert_eq!(events, vec![PasteboardImageObserved]);
        assert!(watch.observer.matches(8), "the observed count is recorded");
        assert!(!watch.observer.matches(7));

        let mut rechecks = 0;
        let events = watch.poll_at(
            t(1),
            || Some(8),
            || {
                rechecks += 1;
                true
            },
        );
        assert!(events.is_empty(), "the same change must not refire");
        assert_eq!(rechecks, 0, "a consumed change must not recheck types");
    }

    #[test]
    fn text_advance_is_consumed_without_firing_or_rechecking() {
        let mut watch = source(true, 7);
        let mut checks = 0;
        let events = watch.poll_at(
            t(0),
            || Some(8),
            || {
                checks += 1;
                false
            },
        );
        assert!(events.is_empty());
        assert_eq!(checks, 1, "types are checked exactly once per change");
        assert!(!watch.observer.matches(8), "no observation is recorded");

        let events = watch.poll_at(
            t(1),
            || Some(8),
            || {
                checks += 1;
                false
            },
        );
        assert!(events.is_empty());
        assert_eq!(checks, 1, "a consumed text change must not recheck types");
    }

    #[test]
    fn queries_are_throttled_within_the_interval() {
        let mut watch = source(true, 7);
        let mut queries = 0;
        let start = t(0);
        watch.poll_at(start, || Some(7), || false);
        queries += 1;
        watch.poll_at(
            start + Duration::from_millis(100),
            || {
                queries += 1;
                Some(7)
            },
            || false,
        );
        assert_eq!(queries, 1, "a poll within the interval must not query");
        watch.poll_at(
            start + POLL_INTERVAL,
            || {
                queries += 1;
                Some(7)
            },
            || false,
        );
        assert_eq!(queries, 2, "the next poll at the interval queries again");
    }

    #[test]
    fn a_disabled_switch_never_queries_and_drops_the_baseline() {
        let mut watch = source(false, 7);
        let mut queries = 0;
        let events = watch.poll_at(
            t(0),
            || {
                queries += 1;
                Some(9)
            },
            || panic!("types must not be checked while disabled"),
        );
        assert!(events.is_empty());
        assert_eq!(queries, 0, "a disabled source must not query");

        watch.enabled.store(true, Ordering::Relaxed);
        let events = watch.poll_at(
            t(1),
            || Some(9),
            || panic!("re-arming must only record the baseline"),
        );
        assert!(
            events.is_empty(),
            "content copied while disabled must not fire after re-enabling"
        );
        let events = watch.poll_at(t(2), || Some(10), || true);
        assert_eq!(events, vec![PasteboardImageObserved], "a fresh copy fires");
    }

    #[test]
    fn an_acquire_round_latch_outlives_the_command_and_swallows_the_round() {
        let mut watch = source(true, 7);
        watch.round.enter(RoundWrites { pre: 7, ours: 1 });
        let events = watch.poll_at(
            t(0),
            || Some(8),
            || panic!("types must not be checked during our own acquire round"),
        );
        assert!(
            events.is_empty(),
            "changes attributable to the round are not observations"
        );
        assert_eq!(
            watch.seen,
            Some(8),
            "the baseline moves to the post-round count"
        );

        let events = watch.poll_at(t(1), || Some(9), || true);
        assert_eq!(events, vec![PasteboardImageObserved], "a fresh copy fires");
    }

    #[test]
    fn a_foreign_write_inside_the_round_fires_after_the_round() {
        let mut watch = source(true, 7);
        watch.round.enter(RoundWrites { pre: 7, ours: 0 });
        let events = watch.poll_at(t(0), || Some(8), || true);
        assert_eq!(
            events,
            vec![PasteboardImageObserved],
            "a user copy inside the round is not swallowed"
        );
        assert!(watch.observer.matches(8), "the observed count is recorded");
    }

    #[test]
    fn a_foreign_write_beyond_our_own_writes_fires() {
        let mut watch = source(true, 7);
        watch.round.enter(RoundWrites { pre: 7, ours: 1 });
        let events = watch.poll_at(t(0), || Some(9), || true);
        assert_eq!(
            events,
            vec![PasteboardImageObserved],
            "an advance beyond our own writes is a user copy"
        );
    }

    #[test]
    fn a_second_round_merges_into_the_pending_latch() {
        let mut watch = source(true, 7);
        watch.round.enter(RoundWrites { pre: 7, ours: 2 });
        watch.round.enter(RoundWrites { pre: 9, ours: 2 });
        assert_eq!(
            watch.round.peek(),
            Some(RoundWrites { pre: 7, ours: 4 }),
            "the second round merges instead of overwriting"
        );
        let events = watch.poll_at(
            t(0),
            || Some(11),
            || panic!("both rounds' own writes must not be checked as foreign"),
        );
        assert!(events.is_empty(), "attributed writes are swallowed");
        assert_eq!(watch.seen, Some(11), "the baseline moves past both rounds");
    }

    #[test]
    fn a_round_latch_on_an_unarmed_source_falls_back_to_the_round_pre_count() {
        let mut watch = source(true, 7);
        watch.seen = None;
        watch.round.enter(RoundWrites { pre: 7, ours: 1 });
        let events = watch.poll_at(
            t(0),
            || Some(8),
            || panic!("attributed writes must not be checked as foreign"),
        );
        assert!(
            events.is_empty(),
            "the round pre count stands in for the missing baseline"
        );
        assert_eq!(
            watch.seen,
            Some(8),
            "the baseline arms at the current count"
        );

        let mut watch = source(true, 7);
        watch.seen = None;
        watch.round.enter(RoundWrites { pre: 7, ours: 0 });
        let events = watch.poll_at(
            t(0),
            || Some(9),
            || panic!("the fall-through must arm without firing"),
        );
        assert!(events.is_empty(), "a foreign write arms the source quietly");
        assert_eq!(watch.seen, Some(9));
    }

    #[test]
    fn an_unreadable_count_at_the_round_latch_stays_quiet_and_rearms() {
        let mut watch = source(true, 7);
        watch.round.enter(RoundWrites { pre: 7, ours: 0 });
        let events = watch.poll_at(
            t(0),
            || None,
            || panic!("types must not be checked without a count"),
        );
        assert!(events.is_empty());
        assert_eq!(watch.seen, None, "an unreadable count drops the baseline");
        let events = watch.poll_at(
            t(1),
            || Some(8),
            || panic!("arming must only record the baseline"),
        );
        assert!(events.is_empty());
        let events = watch.poll_at(t(2), || Some(9), || true);
        assert_eq!(events, vec![PasteboardImageObserved], "recovers afterwards");
    }

    #[test]
    fn a_text_round_restoring_an_image_board_must_not_refire_it() {
        let mut watch = source(true, 7);
        let events = watch.poll_at(t(0), || Some(8), || true);
        assert_eq!(events, vec![PasteboardImageObserved]);

        watch.round.enter(RoundWrites { pre: 8, ours: 1 });
        let events = watch.poll_at(t(1), || Some(9), || true);
        assert!(
            events.is_empty(),
            "the restored image must not be observed as a user copy"
        );
        assert_eq!(watch.seen, Some(9), "the baseline moves past the restore");

        let events = watch.poll_at(
            t(2),
            || Some(9),
            || panic!("a consumed change must not recheck types"),
        );
        assert!(events.is_empty());
        let events = watch.poll_at(t(3), || Some(10), || true);
        assert_eq!(events, vec![PasteboardImageObserved], "a fresh copy fires");
    }

    #[test]
    fn a_restore_self_reporting_full_jumps_is_swallowed() {
        let mut watch = source(true, 7);
        watch.round.enter(RoundWrites { pre: 7, ours: 5 });
        let events = watch.poll_at(t(0), || Some(12), || true);
        assert!(
            events.is_empty(),
            "a flavor restore counted as empty-plus-sets must not be observed"
        );
        assert_eq!(watch.seen, Some(12), "the baseline moves past the restore");
    }

    #[test]
    fn a_restore_underreporting_jumps_fires_a_spurious_observation() {
        let mut watch = source(true, 7);
        watch.round.enter(RoundWrites { pre: 7, ours: 2 });
        let events = watch.poll_at(t(0), || Some(12), || true);
        assert_eq!(
            events,
            vec![PasteboardImageObserved],
            "an underreported latch lets the restored image masquerade as a user copy"
        );
    }

    #[test]
    fn an_unarmed_source_arms_without_firing() {
        let enabled = Arc::new(AtomicBool::new(true));
        let mut watch = ClipboardWatchSource {
            enabled,
            observer: PasteboardObserver::default(),
            round: WatchRoundFlag::default(),
            seen: None,
            next_query: None,
            interval: POLL_INTERVAL,
        };
        let events = watch.poll_at(t(0), || Some(100), || panic!("arming must not check types"));
        assert!(
            events.is_empty(),
            "the construction-time clipboard content must not fire"
        );
        let events = watch.poll_at(t(1), || Some(101), || true);
        assert_eq!(events, vec![PasteboardImageObserved]);
    }

    #[test]
    fn a_failed_count_query_stays_quiet_and_recovers() {
        let mut watch = source(true, 7);
        let events = watch.poll_at(
            t(0),
            || None,
            || panic!("types must not be checked without a count"),
        );
        assert!(events.is_empty());
        let events = watch.poll_at(t(1), || Some(8), || true);
        assert_eq!(events, vec![PasteboardImageObserved], "recovers afterwards");
    }

    #[test]
    fn observer_mismatch_flags_the_race() {
        let observer = PasteboardObserver::default();
        assert!(observer.matches(0));
        assert!(!observer.matches(1));
        observer.record(42);
        assert!(observer.matches(42));
        assert!(!observer.matches(43), "a re-copied board must not match");
    }
}
