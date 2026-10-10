//! 剪贴板图片哨兵源：只读 `NSPasteboard.changeCount` 的廉价监听。
//!
//! 随平台事件线程的 tick 轮询（`EventSource`），源内自节流：changeCount 是
//! 纯元数据查询，前进后再查一次 `types` 是否含图像才产出观察事件——纯文本
//! 复制不触发、不占代数。构造时记一次 changeCount 作基线，启动时的既有
//! 剪贴板内容不触发；开关（共享 `AtomicBool`）关时整源不查询（零开销），
//! 重开时重记基线——关闭期间的旧内容在重开后不触发。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::{pasteboard_change_count, pasteboard_has_image};
use crate::events::EventSource;

/// 哨兵的自节流节奏：观察延迟远低于复制到弹卡的端到端预期，节流省下的是
/// 事件线程每轮 tick 的查询。
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// 哨兵本轮的观察产物：剪贴板出现了新图片（无载荷——内容在取材命令②再
/// 读）。组装点把它映射为 `PlatformEvent::PasteboardImageObserved`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasteboardImageObserved;

/// ① 发出时刻的 changeCount 观察记录：哨兵写入、取材读取器比对——②到达
/// 时现值与记录不一致即剪贴板在①②之间被覆盖（竞态），取材静默丢弃。
/// 两侧同在事件线程上运行，原子量只为跨闭包共享；初始值 0（changeCount
/// 恒非负）与任何真实读数不匹配，未经哨兵的取材一律按竞态丢弃。
#[derive(Debug, Clone, Default)]
pub struct PasteboardObserver {
    count: Arc<AtomicU64>,
}

impl PasteboardObserver {
    fn record(&self, count: u64) {
        self.count.store(count, Ordering::Relaxed);
    }

    /// ② 取材时刻的现值是否仍是①观察记录的那次变化。
    pub fn matches(&self, current: u64) -> bool {
        self.count.load(Ordering::Relaxed) == current
    }
}

/// 剪贴板图片哨兵源：事件线程每轮 tick 抽干一次（[`EventSource::poll`]），
/// 源内按 [`POLL_INTERVAL`] 节流查询；开关关时只读共享位、不碰系统 API。
pub struct ClipboardWatchSource {
    /// 触发开关（配置快照初始化、设置页热切换）；关态不查询。
    enabled: Arc<AtomicBool>,
    /// ① 发出时刻的 changeCount 观察记录，供②比对竞态。
    observer: PasteboardObserver,
    /// 已记基线的 changeCount；`None` = 未武装（构造时读不到或开关刚重开），
    /// 下一轮到查询只记基线、不触发。
    seen: Option<u64>,
    /// 下一次允许查询 changeCount 的时刻（节流闸门）。
    next_query: Option<Instant>,
    /// 查询节奏（注入测试驱动时序断言）。
    interval: Duration,
}

impl ClipboardWatchSource {
    /// 生产构造：构造时读一次 changeCount 记基线（启动时的既有剪贴板内容
    /// 不触发）；读不到保持未武装，首轮 poll 懒记。开关位按启动配置快照
    /// 初始化，运行中由设置页置位热切换。
    pub fn new(enabled: Arc<AtomicBool>) -> Self {
        let mut source = Self {
            enabled,
            observer: PasteboardObserver::default(),
            seen: None,
            next_query: None,
            interval: POLL_INTERVAL,
        };
        source.rebaseline(pasteboard_change_count);
        source
    }

    /// ① 观察记录的共享句柄：交给同一次装配的 [`ClipboardImageReader`]，
    /// ② 取材时比对现值判定竞态。
    pub fn observer(&self) -> PasteboardObserver {
        self.observer.clone()
    }

    /// 记基线：读到值即武装（构造时的既有内容不触发）；读不到保持未武装。
    fn rebaseline(&mut self, count: impl FnOnce() -> Option<u64>) {
        if let Some(count) = count() {
            self.seen = Some(count);
        }
    }

    /// 一轮观察（时间与系统读数注入，纯逻辑单测覆盖）：
    ///
    /// - 开关关：不查询（关态零开销）；基线作废，重开时重记——关闭期间的
    ///   旧内容在重开后不触发；
    /// - 未武装：只记基线，本轮不触发；
    /// - 到节奏才查 changeCount：没前进不动作；前进即消费基线（文本复制
    ///   不复查 types），复查出图像才产出观察事件，并记下该次变化的
    ///   changeCount 供②比对竞态。
    fn poll_at(
        &mut self,
        now: Instant,
        mut count: impl FnMut() -> Option<u64>,
        has_image: impl FnOnce() -> bool,
    ) -> Vec<PasteboardImageObserved> {
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

impl EventSource<PasteboardImageObserved> for ClipboardWatchSource {
    fn poll(&mut self) -> Vec<PasteboardImageObserved> {
        self.poll_at(
            Instant::now(),
            pasteboard_change_count,
            pasteboard_has_image,
        )
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
    fn an_unarmed_source_arms_without_firing() {
        let enabled = Arc::new(AtomicBool::new(true));
        let mut watch = ClipboardWatchSource {
            enabled,
            observer: PasteboardObserver::default(),
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

#[cfg(test)]
mod live_tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use arboard::{Clipboard, ImageData};

    use super::*;
    use crate::clipboard::ClipboardImageReader;

    #[test]
    #[ignore = "需图形会话：写真实剪贴板并观察哨兵与取材读取器"]
    fn watch_and_reader_carry_a_real_image_copy() {
        let mut clipboard = Clipboard::new().expect("clipboard should be available");
        let preset = format!("gloss-clip-watch-{}", std::process::id());
        clipboard.set_text(preset.clone()).expect("preset text");

        let mut watch = ClipboardWatchSource::new(Arc::new(AtomicBool::new(true)));
        clipboard
            .set_image(ImageData {
                width: 2,
                height: 2,
                bytes: vec![
                    255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 0,
                ]
                .into(),
            })
            .expect("image write");

        let events = watch.poll();
        assert_eq!(
            events.len(),
            1,
            "the image copy must produce exactly one observation"
        );
        assert!(
            watch.poll().is_empty(),
            "no further observation without a change"
        );

        let mut reader = ClipboardImageReader::new(watch.observer());
        let png = reader
            .read()
            .expect("read must succeed")
            .expect("the observed image must still be on the board");
        let decoded = image::load_from_memory(&png).expect("valid png");
        assert_eq!((decoded.width(), decoded.height()), (2, 2));

        clipboard.set_text(preset).expect("restore text");
    }
}
