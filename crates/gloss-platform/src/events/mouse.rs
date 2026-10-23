//! 全局鼠标监听：左键按下/释放 → 划词手势判定。
//!
//! 手势取「拖拽释放」：按压时长与位移双达标才算选择，分别滤掉瞬时甩动与
//! 普通点击。订阅面只有左键按下与释放，键盘事件不进入本模块。
//!
//! 线程约束：tap 在**当前线程**建立并阻塞运行（回调跑在它自己的 RunLoop
//! 上），不能放进平台事件线程；没有停止 API，监听线程随进程退出消亡。
//! 需要辅助功能授权，建立结果同步返回，运行中失效才经标志异步置位。

use crossbeam_channel::Receiver;

const DRAG_THRESHOLD: f64 = 12.0;

const MIN_PRESS_NANOS: u64 = 150_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeftButton {
    Pressed,
    Released,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ButtonEvent {
    action: LeftButton,
    pos: (f64, f64),
    time: u64,
}

const CG_EVENT_LEFT_MOUSE_DOWN: u32 = 1;
const CG_EVENT_LEFT_MOUSE_UP: u32 = 2;

#[cfg(test)]
const CG_EVENT_KEY_DOWN: u32 = 10;
#[cfg(test)]
const CG_EVENT_KEY_UP: u32 = 11;
#[cfg(test)]
const CG_EVENT_FLAGS_CHANGED: u32 = 12;
#[cfg(test)]
const CG_EVENT_SCROLL_WHEEL: u32 = 22;

const SUBSCRIBED_EVENT_TYPES: [(u32, LeftButton); 2] = [
    (CG_EVENT_LEFT_MOUSE_DOWN, LeftButton::Pressed),
    (CG_EVENT_LEFT_MOUSE_UP, LeftButton::Released),
];

fn classify(raw_type: u32) -> Option<LeftButton> {
    SUBSCRIBED_EVENT_TYPES
        .iter()
        .find(|(raw, _)| *raw == raw_type)
        .map(|(_, action)| *action)
}

#[derive(Default)]
struct GestureDetector {
    press: Option<Press>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Press {
    pos: (f64, f64),
    time: u64,
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
                (held >= MIN_PRESS_NANOS && dx * dx + dy * dy > DRAG_THRESHOLD * DRAG_THRESHOLD)
                    .then_some(event.pos)
            }
            (LeftButton::Released, None) => None,
        }
    }
}

mod tap {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::JoinHandle;

    use crossbeam_channel::{Receiver, Sender, bounded};

    use gloss_core::log::{debug, info, thread, warn};
    use gloss_core::model::ScreenPoint;

    use super::{ButtonEvent, GestureDetector};
    use crate::events::EventSource;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum MouseGesture {
        Selection { pos: ScreenPoint },
    }

    #[derive(Debug)]
    enum ListenError {
        EventTap,
        RunLoopSource,
    }

    pub struct MouseSource {
        events: Receiver<ButtonEvent>,
        detector: GestureDetector,
    }

    impl MouseSource {
        pub fn spawn() -> (Option<Self>, Arc<AtomicBool>) {
            let degraded = Arc::new(AtomicBool::new(false));
            let events = spawn_tap(Arc::clone(&degraded));
            (
                events.map(|events| Self {
                    events,
                    detector: GestureDetector::default(),
                }),
                degraded,
            )
        }
    }

    impl EventSource<MouseGesture> for MouseSource {
        fn poll(&mut self) -> Vec<MouseGesture> {
            self.detector
                .poll(&self.events)
                .into_iter()
                .map(|(x, y)| MouseGesture::Selection {
                    pos: ScreenPoint::new(x.round() as i32, y.round() as i32),
                })
                .collect()
        }
    }

    fn spawn_tap(degraded: Arc<AtomicBool>) -> Option<Receiver<ButtonEvent>> {
        let (tx, rx) = bounded(256);
        let (created_tx, created_rx) = bounded::<Result<(), ListenError>>(1);
        let spawned: Result<JoinHandle<()>, _> = std::thread::Builder::new()
            .name("gloss-mouse-tap".into())
            .spawn(move || tap_loop(tx, created_tx, degraded));
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

    fn tap_loop(
        tx: Sender<ButtonEvent>,
        created_tx: Sender<Result<(), ListenError>>,
        degraded: Arc<AtomicBool>,
    ) {
        let sink = move |event: ButtonEvent| {
            #[allow(clippy::let_underscore_must_use)]
            let _ = catch_unwind(AssertUnwindSafe(|| {
                if tx.try_send(event).is_err() {
                    debug!(
                        thread = thread::MOUSE_TAP,
                        "mouse event queue full, event dropped"
                    );
                }
            }));
        };
        match imp::create(sink) {
            Ok(handle) => {
                drop(created_tx.send(Ok(())));
                imp::run(handle);
                degraded.store(true, Ordering::Relaxed);
                info!(thread = thread::MOUSE_TAP, "mouse listener stopped");
            }
            Err(err) => {
                drop(created_tx.send(Err(err)));
            }
        }
    }

    #[cfg(test)]
    pub(super) fn subscribed_mask() -> u64 {
        imp::subscribed_mask()
    }

    mod imp {
        use std::ffi::c_void;

        use crate::ffi::cf::{
            CFMachPortCreateRunLoopSource, CFRelease, CFRunLoopAddSource, CFRunLoopGetCurrent,
            CFRunLoopRun, kCFAllocatorDefault, kCFRunLoopCommonModes,
        };
        use crate::ffi::eventtap::{
            CGEventGetLocation, CGEventGetTimestamp, CGEventRef, CGEventTapCreate,
            CGEventTapEnable, CGEventTapProxy, CGPoint, K_CG_EVENT_TAP_OPTION_LISTEN_ONLY,
            K_CG_HEAD_INSERT_EVENT_TAP, K_CG_HID_EVENT_TAP,
        };

        use super::super::{ButtonEvent, SUBSCRIBED_EVENT_TYPES, classify};
        use super::ListenError;

        pub(super) fn subscribed_mask() -> u64 {
            SUBSCRIBED_EVENT_TYPES
                .iter()
                .fold(0, |mask, (raw, _)| mask | (1_u64 << raw))
        }

        pub struct TapHandle;

        pub fn create(sink: impl Fn(ButtonEvent) + 'static) -> Result<TapHandle, ListenError> {
            let ctx: *mut Box<dyn Fn(ButtonEvent)> = Box::into_raw(Box::new(Box::new(sink)));

            // SAFETY: 参数都是常量或本进程内的有效指针；`raw_callback` 的签名与
            // CGEventTapCallback 一致；`ctx` 刚由 Box::into_raw 得到，与 tap 同寿。
            let tap = unsafe {
                CGEventTapCreate(
                    K_CG_HID_EVENT_TAP,
                    K_CG_HEAD_INSERT_EVENT_TAP,
                    K_CG_EVENT_TAP_OPTION_LISTEN_ONLY,
                    subscribed_mask(),
                    raw_callback,
                    ctx.cast(),
                )
            };
            if tap.is_null() {
                // SAFETY: `ctx` 来自上面的 Box::into_raw，此刻仍是唯一引用。
                drop(unsafe { Box::from_raw(ctx) });
                return Err(ListenError::EventTap);
            }

            // SAFETY: `tap` 非空（上面判过）；NULL allocator 表示默认分配器；
            // 返回 +1 引用，下面交给 run loop 后归还。
            let source = unsafe { CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0) };
            if source.is_null() {
                // SAFETY: `tap` 是上面 CGEventTapCreate 的 +1 引用，归我们归还；
                // `ctx` 仍是 Box::into_raw 得到的唯一引用。
                unsafe {
                    CFRelease(tap.cast::<c_void>());
                    drop(Box::from_raw(ctx));
                }
                return Err(ListenError::RunLoopSource);
            }
            // SAFETY: 取当前线程的 run loop（不转移所有权）；`source` 有效，加入
            // 后 run loop 自己持有一份，故随后释放我们这份 +1 引用。
            unsafe {
                let run_loop = CFRunLoopGetCurrent();
                CFRunLoopAddSource(run_loop, source, kCFRunLoopCommonModes);
                CFRelease(source.cast::<c_void>());
                CGEventTapEnable(tap, true);
            }
            Ok(TapHandle)
        }

        pub fn run(_handle: TapHandle) {
            // SAFETY: `create` 已把 source 挂到当前线程的 run loop 上，这里
            // 只是进入阻塞运行。参数只承载两相顺序约束，进入运行即完成使命。
            unsafe { CFRunLoopRun() };
        }

        unsafe extern "C" fn raw_callback(
            _proxy: CGEventTapProxy,
            raw_type: u32,
            event: CGEventRef,
            user_info: *mut c_void,
        ) -> CGEventRef {
            if let Some(action) = classify(raw_type) {
                // SAFETY: `user_info` 是 `create` 经 `Box::into_raw` 传入、有意泄漏到进程结束的
                // Box<dyn Fn(ButtonEvent)> 指针；C 侧不释放它。
                let sink = unsafe { &*(user_info as *const Box<dyn Fn(ButtonEvent)>) };
                // SAFETY: `event` 是系统在回调期间借给我们的事件引用，只读位置
                // 与时间戳，不转移所有权。
                let CGPoint { x, y } = unsafe { CGEventGetLocation(event) };
                // SAFETY: 同上，`event` 在回调期间有效；时间戳是纯读取。
                let time = unsafe { CGEventGetTimestamp(event) };
                sink(ButtonEvent {
                    action,
                    pos: (x, y),
                    time,
                });
            }
            event
        }
    }
}

pub use tap::{MouseGesture, MouseSource};

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;

    fn pressed(pos: (f64, f64), at_ns: u64) -> ButtonEvent {
        ButtonEvent {
            action: LeftButton::Pressed,
            pos,
            time: at_ns,
        }
    }

    fn released(pos: (f64, f64), at_ns: u64) -> ButtonEvent {
        ButtonEvent {
            action: LeftButton::Released,
            pos,
            time: at_ns,
        }
    }

    const SLOW_PRESS: u64 = 1_000_000_000;
    const SLOW_RELEASE: u64 = 1_400_000_000;

    #[test]
    fn drag_release_emits_selection() {
        let mut detector = GestureDetector::default();
        assert_eq!(detector.feed(pressed((10.0, 10.0), SLOW_PRESS)), None);
        assert_eq!(
            detector.feed(released((140.0, 30.0), SLOW_RELEASE)),
            Some((140.0, 30.0))
        );
    }

    #[test]
    fn plain_click_and_jitter_do_not_trigger() {
        let mut detector = GestureDetector::default();
        assert_eq!(detector.feed(pressed((10.0, 10.0), SLOW_PRESS)), None);
        assert_eq!(
            detector.feed(released((10.0, 10.0), SLOW_RELEASE)),
            None,
            "no displacement, no gesture even with a long press"
        );

        assert_eq!(detector.feed(pressed((50.0, 50.0), SLOW_PRESS)), None);
        assert_eq!(
            detector.feed(released((56.0, 55.0), SLOW_RELEASE)),
            None,
            "jitter below threshold"
        );
    }

    #[test]
    fn flick_shorter_than_the_minimum_press_is_filtered() {
        let mut detector = GestureDetector::default();
        let press = 2_000_000_000;
        assert_eq!(detector.feed(pressed((10.0, 10.0), press)), None);
        assert_eq!(
            detector.feed(released((140.0, 30.0), press + 10_000_000)),
            None,
            "a 10ms flick with ample displacement must not fire"
        );
    }

    #[test]
    fn press_duration_is_observed_at_the_boundary() {
        let mut detector = GestureDetector::default();
        let press = 3_000_000_000;
        assert_eq!(detector.feed(pressed((0.0, 0.0), press)), None);
        assert_eq!(
            detector.feed(released((200.0, 0.0), press + MIN_PRESS_NANOS - 1)),
            None,
            "one nanosecond short of the floor stays filtered"
        );

        assert_eq!(detector.feed(pressed((0.0, 0.0), press)), None);
        assert_eq!(
            detector.feed(released((200.0, 0.0), press + MIN_PRESS_NANOS)),
            Some((200.0, 0.0)),
            "exactly the floor counts as a deliberate drag"
        );
    }

    #[test]
    fn displacement_comes_from_the_events_themselves() {
        let mut detector = GestureDetector::default();
        assert_eq!(detector.feed(pressed((100.0, 100.0), SLOW_PRESS)), None);
        assert_eq!(
            detector.feed(released((400.0, 100.0), SLOW_RELEASE)),
            Some((400.0, 100.0)),
            "release position is the release event's own position"
        );
    }

    #[test]
    fn state_resets_after_each_gesture() {
        let mut detector = GestureDetector::default();
        assert_eq!(detector.feed(pressed((0.0, 0.0), SLOW_PRESS)), None);
        assert_eq!(
            detector.feed(released((200.0, 0.0), SLOW_RELEASE)),
            Some((200.0, 0.0))
        );
        assert_eq!(detector.feed(pressed((500.0, 500.0), SLOW_RELEASE)), None);
        assert_eq!(
            detector.feed(released((700.0, 500.0), SLOW_RELEASE + 400_000_000)),
            Some((700.0, 500.0))
        );
    }

    #[test]
    fn stray_events_are_ignored() {
        let mut detector = GestureDetector::default();
        assert_eq!(detector.feed(released((1.0, 1.0), SLOW_PRESS)), None);
        assert_eq!(detector.feed(pressed((0.0, 0.0), SLOW_PRESS)), None);
        assert_eq!(detector.feed(pressed((100.0, 100.0), SLOW_PRESS)), None);
        assert_eq!(
            detector.feed(released((200.0, 100.0), SLOW_RELEASE)),
            Some((200.0, 100.0))
        );
    }

    #[test]
    fn poll_drains_channel_through_state_machine() {
        let (tx, rx) = unbounded::<ButtonEvent>();
        let mut detector = GestureDetector::default();
        tx.send(pressed((0.0, 0.0), SLOW_PRESS)).unwrap();
        tx.send(released((100.0, 0.0), SLOW_RELEASE)).unwrap();
        assert_eq!(detector.poll(&rx), vec![(100.0, 0.0)]);
        assert!(detector.poll(&rx).is_empty(), "drained queue stays empty");
    }

    #[test]
    fn spawn_resolves_synchronously_and_startup_failure_stays_off_the_flag() {
        let (source, degraded) = MouseSource::spawn();
        assert!(
            !degraded.load(std::sync::atomic::Ordering::Relaxed),
            "startup failure must not set the mid-run flag"
        );
        drop(source);
    }

    #[test]
    fn keyboard_events_are_never_subscribed() {
        assert_eq!(
            SUBSCRIBED_EVENT_TYPES.map(|(raw, _)| raw),
            [CG_EVENT_LEFT_MOUSE_DOWN, CG_EVENT_LEFT_MOUSE_UP],
            "the subscription list holds the left button only"
        );
        assert_eq!(
            classify(CG_EVENT_LEFT_MOUSE_DOWN),
            Some(LeftButton::Pressed)
        );
        assert_eq!(classify(CG_EVENT_LEFT_MOUSE_UP), Some(LeftButton::Released));

        for raw in [
            CG_EVENT_KEY_DOWN,
            CG_EVENT_KEY_UP,
            CG_EVENT_FLAGS_CHANGED,
            CG_EVENT_SCROLL_WHEEL,
        ] {
            assert_eq!(classify(raw), None, "type {raw} must never be handled");
        }
        let mask = tap::subscribed_mask();
        assert_eq!(
            mask,
            (1 << CG_EVENT_LEFT_MOUSE_DOWN) | (1 << CG_EVENT_LEFT_MOUSE_UP)
        );
        for raw in [
            CG_EVENT_KEY_DOWN,
            CG_EVENT_KEY_UP,
            CG_EVENT_FLAGS_CHANGED,
            CG_EVENT_SCROLL_WHEEL,
        ] {
            assert_eq!(mask & (1 << raw), 0, "mask bit {raw} must stay clear");
        }
        assert_eq!(classify(0), None, "unsubscribed types fall through");
        assert_eq!(classify(999), None);
    }
}

#[cfg(test)]
mod injected_gesture_live_tests {
    use std::time::{Duration, Instant};

    use super::tap::MouseSource;
    use crate::events::EventSource;
    use crate::ffi::event::{
        self, CG_EVENT_LEFT_MOUSE_DOWN, CG_EVENT_LEFT_MOUSE_UP, CG_EVENT_MOUSE_MOVED,
    };
    use crate::ffi::eventtap::CGPoint;

    #[test]
    #[ignore = "注入真实全局鼠标事件：先把运行测试的终端 App 加入 系统设置→隐私与安全性→辅助功能（未授权时由 live_test_support 快速失败）"]
    fn injected_drag_yields_selection_gesture() {
        crate::live_test_support::require_accessibility("mouse_drag_gesture");
        let (source, degraded) = MouseSource::spawn();
        let mut source = source.expect("tap must start under accessibility grant");

        let hid = event::new_hid_source().expect("hid event source");
        assert!(
            event::post_mouse_event(&hid, CG_EVENT_MOUSE_MOVED, CGPoint { x: 100.0, y: 100.0 }),
            "move injection"
        );
        let press_at = event::current_position(&hid).expect("current location");
        assert!(
            event::post_mouse_event(&hid, CG_EVENT_LEFT_MOUSE_DOWN, press_at),
            "button press injection"
        );
        for step in 1..=5 {
            assert!(
                event::post_mouse_event(
                    &hid,
                    CG_EVENT_MOUSE_MOVED,
                    CGPoint {
                        x: 100.0 + 60.0 * f64::from(step),
                        y: 100.0 + 40.0 * f64::from(step),
                    },
                ),
                "move injection"
            );
        }
        std::thread::sleep(Duration::from_millis(200));
        let release_at = event::current_position(&hid).expect("current location");
        assert!(
            event::post_mouse_event(&hid, CG_EVENT_LEFT_MOUSE_UP, release_at),
            "button release injection"
        );

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if !source.poll().is_empty() {
                assert!(
                    !degraded.load(std::sync::atomic::Ordering::Relaxed),
                    "listener must not be degraded under a valid grant"
                );
                return;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        panic!("selection gesture not observed within 2s");
    }
}
