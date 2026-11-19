//! 划词手势的平台中立共享形状：拖拽释放判定状态机与监听线程脚手架只写
//! 一份，两侧平台目录共用；平台侧只留事件创建与宿主运行两个闭包——创建
//! 相在当前线程建立平台监听原语并同步回报建立结果，宿主相阻塞运行到监听
//! 失效。事件时间统一为时长（Duration），平台回调按各自时间戳粒度换算
//! （系统（macOS）为纳秒、系统（Windows）为毫秒），判定只比较时长，单位
//! 分叉不进共享层。
//!
//! 手势取“拖拽释放”：按压时长与位移双达标才算选择，分别滤掉瞬时甩动与
//! 普通点击；最短按压时长、位移阈值与事件队列容量随本模块单源。监听线程
//! 与有界事件队列随本模块：事件经恐慌（panic）屏障入队（满则丢弃，回调
//! 不阻塞不越过语言（C）边界），手势判定在平台事件线程的轮询侧完成——
//! 平台回调只做事件搬运的架构不变量由“回调只触汇点、汇点只入队”承担。
//! 无停止接口，监听线程随进程退出消亡；运行中失效经标志异步置位，启动
//! 失败不经标志表达。本模块零目标条件编译、零外部函数接口（FFI）、零
//! 不安全块。

use std::fmt::Debug;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, bounded};

use gloss_core::log::{debug, info, thread, warn};
use gloss_core::model::ScreenPoint;
use gloss_core::ports::SelectionGestureSource;

const DRAG_THRESHOLD: f64 = 12.0;

const MIN_PRESS: Duration = Duration::from_millis(150);

const QUEUE_CAPACITY: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ButtonEvent {
    pub action: LeftButton,
    pub pos: (f64, f64),
    pub time: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeftButton {
    Pressed,
    Released,
}

pub type Sink = Box<dyn Fn(ButtonEvent)>;

#[derive(Default)]
struct GestureDetector {
    press: Option<Press>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Press {
    pos: (f64, f64),
    time: Duration,
}

impl GestureDetector {
    fn poll(&mut self, events: &Receiver<ButtonEvent>) -> Vec<(f64, f64)> {
        let mut fired = Vec::new();
        while let Ok(event) = events.try_recv() {
            if let Some(pos) = self.feed(event) {
                fired.push(pos);
            }
        }
        fired
    }

    fn feed(&mut self, event: ButtonEvent) -> Option<(f64, f64)> {
        match (event.action, self.press) {
            (LeftButton::Pressed, _) => {
                self.press = Some(Press {
                    pos: event.pos,
                    time: event.time,
                });
                None
            }
            (LeftButton::Released, Some(start)) => {
                self.press = None;
                let dx = event.pos.0 - start.pos.0;
                let dy = event.pos.1 - start.pos.1;
                let held = event.time.saturating_sub(start.time);
                (held >= MIN_PRESS && dx * dx + dy * dy > DRAG_THRESHOLD * DRAG_THRESHOLD)
                    .then_some(event.pos)
            }
            (LeftButton::Released, None) => None,
        }
    }
}

pub struct MouseSource {
    events: Receiver<ButtonEvent>,
    detector: GestureDetector,
}

impl MouseSource {
    pub(crate) fn spawn_listening<C, R, H, E>(create: C, run: R) -> (Option<Self>, Arc<AtomicBool>)
    where
        C: FnOnce(Sink) -> Result<H, E> + Send + 'static,
        R: FnOnce(H) + Send + 'static,
        H: 'static,
        E: Debug + Send + 'static,
    {
        let degraded = Arc::new(AtomicBool::new(false));
        let events = spawn_tap(Arc::clone(&degraded), create, run);
        (
            events.map(|events| Self {
                events,
                detector: GestureDetector::default(),
            }),
            degraded,
        )
    }
}

impl SelectionGestureSource for MouseSource {
    fn poll(&mut self) -> Vec<ScreenPoint> {
        self.detector
            .poll(&self.events)
            .into_iter()
            .map(|(x, y)| ScreenPoint::new(x.round() as i32, y.round() as i32))
            .collect()
    }
}

fn spawn_tap<C, R, H, E>(
    degraded: Arc<AtomicBool>,
    create: C,
    run: R,
) -> Option<Receiver<ButtonEvent>>
where
    C: FnOnce(Sink) -> Result<H, E> + Send + 'static,
    R: FnOnce(H) + Send + 'static,
    H: 'static,
    E: Debug + Send + 'static,
{
    let (tx, rx) = bounded(QUEUE_CAPACITY);
    let (created_tx, created_rx) = bounded::<Result<(), E>>(1);
    let spawned: Result<JoinHandle<()>, _> = std::thread::Builder::new()
        .name("gloss-mouse-tap".into())
        .spawn(move || tap_loop(tx, created_tx, degraded, create, run));
    match spawned {
        Ok(_join) => match created_rx.recv() {
            Ok(Ok(())) => Some(rx),
            Ok(Err(err)) => {
                warn!(
                    thread = thread::MOUSE_TAP,
                    error = ?err,
                    "mouse tap creation failed, selection gesture disabled"
                );
                None
            }
            Err(_) => {
                warn!(
                    thread = thread::MOUSE_TAP,
                    "mouse tap thread vanished before reporting creation"
                );
                None
            }
        },
        Err(err) => {
            warn!(
                thread = thread::MOUSE_TAP,
                error = %err,
                "failed to spawn mouse tap thread, selection gesture disabled"
            );
            None
        }
    }
}

fn tap_loop<C, R, H, E>(
    tx: Sender<ButtonEvent>,
    created_tx: Sender<Result<(), E>>,
    degraded: Arc<AtomicBool>,
    create: C,
    run: R,
) where
    C: FnOnce(Sink) -> Result<H, E> + Send + 'static,
    R: FnOnce(H) + Send + 'static,
    H: 'static,
    E: Send + 'static,
{
    let sink: Sink = Box::new(move |event: ButtonEvent| {
        #[allow(clippy::let_underscore_must_use)]
        let _ = catch_unwind(AssertUnwindSafe(|| {
            if tx.try_send(event).is_err() {
                debug!(
                    thread = thread::MOUSE_TAP,
                    "mouse event queue full, event dropped"
                );
            }
        }));
    });
    match create(sink) {
        Ok(handle) => {
            drop(created_tx.send(Ok(())));
            run(handle);
            degraded.store(true, Ordering::Relaxed);
            info!(thread = thread::MOUSE_TAP, "mouse listener stopped");
        }
        Err(err) => {
            drop(created_tx.send(Err(err)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;

    fn pressed(pos: (f64, f64), at: Duration) -> ButtonEvent {
        ButtonEvent {
            action: LeftButton::Pressed,
            pos,
            time: at,
        }
    }

    fn released(pos: (f64, f64), at: Duration) -> ButtonEvent {
        ButtonEvent {
            action: LeftButton::Released,
            pos,
            time: at,
        }
    }

    fn secs(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }

    #[test]
    fn drag_release_emits_selection() {
        let mut detector = GestureDetector::default();
        assert_eq!(detector.feed(pressed((10.0, 10.0), secs(1))), None);
        assert_eq!(
            detector.feed(released(
                (140.0, 30.0),
                secs(1) + Duration::from_millis(400)
            )),
            Some((140.0, 30.0))
        );
    }

    #[test]
    fn plain_click_and_jitter_do_not_trigger() {
        let mut detector = GestureDetector::default();
        assert_eq!(detector.feed(pressed((10.0, 10.0), secs(1))), None);
        assert_eq!(
            detector.feed(released((10.0, 10.0), secs(1) + Duration::from_millis(400))),
            None,
            "no displacement, no gesture even with a long press"
        );

        assert_eq!(detector.feed(pressed((50.0, 50.0), secs(1))), None);
        assert_eq!(
            detector.feed(released((56.0, 55.0), secs(1) + Duration::from_millis(400))),
            None,
            "jitter below threshold"
        );
    }

    #[test]
    fn flick_shorter_than_the_minimum_press_is_filtered() {
        let mut detector = GestureDetector::default();
        let press = secs(2);
        assert_eq!(detector.feed(pressed((10.0, 10.0), press)), None);
        assert_eq!(
            detector.feed(released((140.0, 30.0), press + Duration::from_millis(10))),
            None,
            "a 10ms flick with ample displacement must not fire"
        );
    }

    #[test]
    fn press_duration_is_observed_at_the_boundary() {
        let mut detector = GestureDetector::default();
        let press = secs(3);
        assert_eq!(detector.feed(pressed((0.0, 0.0), press)), None);
        assert_eq!(
            detector.feed(released(
                (200.0, 0.0),
                press + MIN_PRESS - Duration::from_nanos(1)
            )),
            None,
            "one nanosecond short of the floor stays filtered"
        );

        let mut detector = GestureDetector::default();
        assert_eq!(detector.feed(pressed((0.0, 0.0), press)), None);
        assert_eq!(
            detector.feed(released(
                (200.0, 0.0),
                press + MIN_PRESS - Duration::from_millis(1)
            )),
            None,
            "one millisecond short of the floor stays filtered"
        );

        let mut detector = GestureDetector::default();
        assert_eq!(detector.feed(pressed((0.0, 0.0), press)), None);
        assert_eq!(
            detector.feed(released((200.0, 0.0), press + MIN_PRESS)),
            Some((200.0, 0.0)),
            "exactly the floor counts as a deliberate drag"
        );
    }

    #[test]
    fn displacement_comes_from_the_events_themselves() {
        let mut detector = GestureDetector::default();
        assert_eq!(detector.feed(pressed((100.0, 100.0), secs(1))), None);
        assert_eq!(
            detector.feed(released(
                (400.0, 100.0),
                secs(1) + Duration::from_millis(400)
            )),
            Some((400.0, 100.0)),
            "release position is the release event's own position"
        );
    }

    #[test]
    fn state_resets_after_each_gesture() {
        let mut detector = GestureDetector::default();
        assert_eq!(detector.feed(pressed((0.0, 0.0), secs(1))), None);
        assert_eq!(
            detector.feed(released((200.0, 0.0), secs(1) + Duration::from_millis(400))),
            Some((200.0, 0.0))
        );
        assert_eq!(detector.feed(pressed((500.0, 500.0), secs(1))), None);
        assert_eq!(
            detector.feed(released(
                (700.0, 500.0),
                secs(1) + Duration::from_millis(800)
            )),
            Some((700.0, 500.0))
        );
    }

    #[test]
    fn stray_events_are_ignored() {
        let mut detector = GestureDetector::default();
        assert_eq!(detector.feed(released((1.0, 1.0), secs(1))), None);
        assert_eq!(detector.feed(pressed((0.0, 0.0), secs(1))), None);
        assert_eq!(detector.feed(pressed((100.0, 100.0), secs(1))), None);
        assert_eq!(
            detector.feed(released(
                (200.0, 100.0),
                secs(1) + Duration::from_millis(400)
            )),
            Some((200.0, 100.0))
        );
    }

    #[test]
    fn poll_drains_channel_through_state_machine() {
        let (tx, rx) = unbounded::<ButtonEvent>();
        let mut detector = GestureDetector::default();
        tx.send(pressed((0.0, 0.0), secs(1))).unwrap();
        tx.send(released((100.0, 0.0), secs(1) + Duration::from_millis(400)))
            .unwrap();
        assert_eq!(detector.poll(&rx), vec![(100.0, 0.0)]);
        assert!(detector.poll(&rx).is_empty(), "drained queue stays empty");
    }

    #[test]
    fn a_failed_creation_leaves_the_source_absent_and_the_flag_cold() {
        let (source, degraded) = MouseSource::spawn_listening(
            |_sink: Sink| -> Result<(), &'static str> { Err("synthetic failure") },
            |_handle: ()| {},
        );
        assert!(source.is_none(), "a failed creation yields no source");
        assert!(
            !degraded.load(Ordering::Relaxed),
            "startup failure must not set the mid-run flag"
        );
    }

    #[test]
    fn events_beyond_the_queue_capacity_are_dropped_without_blocking() {
        let gestures = QUEUE_CAPACITY / 2;
        let (source, _degraded) = MouseSource::spawn_listening(
            move |sink: Sink| -> Result<(), ()> {
                for i in 0..=gestures {
                    let x = i as f64;
                    sink(pressed((x, 0.0), secs(1)));
                    sink(released(
                        (x + 100.0, 0.0),
                        secs(1) + Duration::from_millis(400),
                    ));
                }
                Ok(())
            },
            |_handle: ()| {},
        );
        let mut source = source.expect("the synthetic creation must succeed");
        assert_eq!(
            source.poll().len(),
            gestures,
            "capacity holds whole gesture pairs; the overflowing pair is dropped"
        );
        assert!(source.poll().is_empty(), "the overflow stays dropped");
    }
}
