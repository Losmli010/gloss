//! 全局鼠标监听：左键按下与释放 → 划词手势判定。
//!
//! 手势取“拖拽释放”：按压时长与位移双达标才算选择，分别滤掉瞬时甩动与
//! 普通点击。订阅面只有左键按下与释放，键盘事件不进入本模块。
//!
//! 线程约束：低级鼠标钩子的回调在安装线程取送消息时被系统调用，监听在
//! 独立线程建立并跑消息泵，不占平台事件线程；钩子回调内只做坐标判定并入
//! 队（队列满则丢弃），手势判定在平台事件线程的轮询侧完成。没有停止接口，
//! 监听线程随进程退出消亡；运行中失效（泵退出）经标志异步置位，建立结果
//! 同步返回。钩子无需系统授权。

use crossbeam_channel::Receiver;

const DRAG_THRESHOLD: f64 = 12.0;

const MIN_PRESS_MS: u64 = 150;

const WM_L_DOWN: u32 = 0x0201;

const WM_L_UP: u32 = 0x0202;

#[cfg(test)]
const WM_KEY_DOWN: u32 = 0x0100;

#[cfg(test)]
const WM_MOUSE_MOVE: u32 = 0x0200;

#[cfg(test)]
const WM_R_DOWN: u32 = 0x0204;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeftButton {
    Pressed,
    Released,
}

fn classify(message: u32) -> Option<LeftButton> {
    match message {
        WM_L_DOWN => Some(LeftButton::Pressed),
        WM_L_UP => Some(LeftButton::Released),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ButtonEvent {
    action: LeftButton,
    pos: (f64, f64),
    time: u64,
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
                (held >= MIN_PRESS_MS && dx * dx + dy * dy > DRAG_THRESHOLD * DRAG_THRESHOLD)
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
    use gloss_core::ports::SelectionGestureSource;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum ListenError {
        Hook,
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

    impl SelectionGestureSource for MouseSource {
        fn poll(&mut self) -> Vec<ScreenPoint> {
            self.detector
                .poll(&self.events)
                .into_iter()
                .map(|(x, y)| ScreenPoint::new(x.round() as i32, y.round() as i32))
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
                        "mouse hook creation failed, selection gesture disabled"
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

    mod imp {
        use std::cell::RefCell;

        use crate::ffi::hook::{self, LPARAM, LRESULT, MSG, WPARAM};

        use super::{ButtonEvent, ListenError};
        type Sink = Box<dyn Fn(ButtonEvent) + 'static>;

        thread_local! {
            static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
        }

        pub struct TapHandle {
            _hook: hook::MouseHook,
        }

        pub fn create(sink: impl Fn(ButtonEvent) + 'static) -> Result<TapHandle, ListenError> {
            SINK.with(|cell| *cell.borrow_mut() = Some(Box::new(sink)));
            match hook::MouseHook::install(Some(hook_procedure)) {
                Ok(instance) => Ok(TapHandle { _hook: instance }),
                Err(_) => Err(ListenError::Hook),
            }
        }

        pub fn run(_handle: TapHandle) {
            loop {
                let mut message = MSG::default();
                if let hook::PumpFlow::Exit = hook::pump_once(&mut message) {
                    break;
                }
                hook::dispatch(&message);
            }
        }

        unsafe extern "system" fn hook_procedure(
            code: i32,
            wparam: WPARAM,
            lparam: LPARAM,
        ) -> LRESULT {
            if code < 0 {
                return hook::next_hook(code, wparam, lparam);
            }
            if let Some((message, x, y, time)) = hook::low_level_mouse(wparam, lparam)
                && let Some(action) = super::super::classify(message)
            {
                SINK.with(|cell| {
                    if let Some(sink) = cell.borrow().as_ref() {
                        sink(ButtonEvent {
                            action,
                            pos: (x, y),
                            time,
                        });
                    }
                });
            }
            hook::next_hook(code, wparam, lparam)
        }
    }
}

pub use tap::MouseSource;

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;

    fn pressed(pos: (f64, f64), at_ms: u64) -> ButtonEvent {
        ButtonEvent {
            action: LeftButton::Pressed,
            pos,
            time: at_ms,
        }
    }

    fn released(pos: (f64, f64), at_ms: u64) -> ButtonEvent {
        ButtonEvent {
            action: LeftButton::Released,
            pos,
            time: at_ms,
        }
    }

    const SLOW_PRESS: u64 = 1_000;
    const SLOW_RELEASE: u64 = 1_400;

    #[test]
    fn hook_drag_release_emits_selection() {
        let mut detector = GestureDetector::default();
        assert_eq!(detector.feed(pressed((10.0, 10.0), SLOW_PRESS)), None);
        assert_eq!(
            detector.feed(released((140.0, 30.0), SLOW_RELEASE)),
            Some((140.0, 30.0))
        );
    }

    #[test]
    fn hook_plain_click_and_jitter_do_not_trigger() {
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
    fn hook_flick_shorter_than_the_minimum_press_is_filtered() {
        let mut detector = GestureDetector::default();
        let press = 2_000;
        assert_eq!(detector.feed(pressed((10.0, 10.0), press)), None);
        assert_eq!(
            detector.feed(released((140.0, 30.0), press + 10)),
            None,
            "a 10ms flick with ample displacement must not fire"
        );
    }

    #[test]
    fn hook_press_duration_is_observed_at_the_boundary() {
        let mut detector = GestureDetector::default();
        let press = 3_000;
        assert_eq!(detector.feed(pressed((0.0, 0.0), press)), None);
        assert_eq!(
            detector.feed(released((200.0, 0.0), press + MIN_PRESS_MS - 1)),
            None,
            "one millisecond short of the floor stays filtered"
        );

        assert_eq!(detector.feed(pressed((0.0, 0.0), press)), None);
        assert_eq!(
            detector.feed(released((200.0, 0.0), press + MIN_PRESS_MS)),
            Some((200.0, 0.0)),
            "exactly the floor counts as a deliberate drag"
        );
    }

    #[test]
    fn hook_displacement_comes_from_the_events_themselves() {
        let mut detector = GestureDetector::default();
        assert_eq!(detector.feed(pressed((100.0, 100.0), SLOW_PRESS)), None);
        assert_eq!(
            detector.feed(released((400.0, 100.0), SLOW_RELEASE)),
            Some((400.0, 100.0)),
            "release position is the release event's own position"
        );
    }

    #[test]
    fn hook_state_resets_after_each_gesture() {
        let mut detector = GestureDetector::default();
        assert_eq!(detector.feed(pressed((0.0, 0.0), SLOW_PRESS)), None);
        assert_eq!(
            detector.feed(released((200.0, 0.0), SLOW_RELEASE)),
            Some((200.0, 0.0))
        );
        assert_eq!(detector.feed(pressed((500.0, 500.0), SLOW_RELEASE)), None);
        assert_eq!(
            detector.feed(released((700.0, 500.0), SLOW_RELEASE + 400)),
            Some((700.0, 500.0))
        );
    }

    #[test]
    fn hook_stray_events_are_ignored() {
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
    fn hook_poll_drains_channel_through_state_machine() {
        let (tx, rx) = unbounded::<ButtonEvent>();
        let mut detector = GestureDetector::default();
        tx.send(pressed((0.0, 0.0), SLOW_PRESS)).unwrap();
        tx.send(released((100.0, 0.0), SLOW_RELEASE)).unwrap();
        assert_eq!(detector.poll(&rx), vec![(100.0, 0.0)]);
        assert!(detector.poll(&rx).is_empty(), "drained queue stays empty");
    }

    #[test]
    fn hook_spawn_resolves_synchronously_and_startup_failure_stays_off_the_flag() {
        let (source, degraded) = MouseSource::spawn();
        assert!(
            !degraded.load(std::sync::atomic::Ordering::Relaxed),
            "startup failure must not set the mid-run flag"
        );
        drop(source);
    }

    #[test]
    fn hook_mouse_messages_are_the_only_subscription() {
        assert_eq!(classify(WM_L_DOWN), Some(LeftButton::Pressed));
        assert_eq!(classify(WM_L_UP), Some(LeftButton::Released));

        for message in [WM_KEY_DOWN, WM_KEY_UP, WM_MOUSE_MOVE, WM_R_DOWN] {
            assert_eq!(
                classify(message),
                None,
                "message {message} must never be handled"
            );
        }
        assert_eq!(classify(0), None, "unsubscribed messages fall through");
    }

    const WM_KEY_UP: u32 = 0x0101;
}
