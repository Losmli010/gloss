//! 全局鼠标监听（系统（macOS）绑定）：自建只订阅左键按下与释放的事件
//! 监听器（tap），回调把事件原样翻译进共享手势事件——键盘事件绝不进入
//! 订阅面，订阅规则以订阅清单单源。手势判定与监听线程脚手架住平台中立
//! 共享层（common），本模块只提供事件创建与宿主运行两个闭包：创建相在
//! 当前线程建立监听器（tap）并挂上运行循环（RunLoop），宿主相阻塞运行到
//! 监听失效。
//!
//! 线程约束：事件监听在当前线程建立并阻塞运行，回调跑在它自己的运行循环
//! 上，不能放进平台事件线程。需要辅助功能授权，建立结果同步返回，运行中
//! 失效才经标志异步置位。

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::common::gesture::LeftButton;

pub use crate::common::gesture::MouseSource;

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

#[derive(Debug)]
enum ListenError {
    EventTap,
    RunLoopSource,
}

impl MouseSource {
    pub fn spawn() -> (Option<Self>, Arc<AtomicBool>) {
        Self::spawn_listening(imp::create, imp::run)
    }
}

mod imp {
    use std::ffi::c_void;
    use std::time::Duration;

    use crate::common::gesture::{ButtonEvent, Sink};
    use crate::ffi::cf::{
        CFMachPortCreateRunLoopSource, CFRelease, CFRunLoopAddSource, CFRunLoopGetCurrent,
        CFRunLoopRun, kCFAllocatorDefault, kCFRunLoopCommonModes,
    };
    use crate::ffi::eventtap::{
        CGEventGetLocation, CGEventGetTimestamp, CGEventRef, CGEventTapCreate, CGEventTapEnable,
        CGEventTapProxy, CGPoint, K_CG_EVENT_TAP_OPTION_LISTEN_ONLY, K_CG_HEAD_INSERT_EVENT_TAP,
        K_CG_HID_EVENT_TAP,
    };

    use super::{ListenError, SUBSCRIBED_EVENT_TYPES, classify};

    pub(super) fn subscribed_mask() -> u64 {
        SUBSCRIBED_EVENT_TYPES
            .iter()
            .fold(0, |mask, (raw, _)| mask | (1_u64 << raw))
    }

    pub struct TapHandle;

    pub(super) fn create(sink: Sink) -> Result<TapHandle, ListenError> {
        let ctx: *mut Box<dyn Fn(ButtonEvent)> = Box::into_raw(Box::new(sink));

        // SAFETY: all arguments are constants or valid pointers within this
        // process; the `raw_callback` signature matches `CGEventTapCallback`;
        // `ctx` was just obtained from `Box::into_raw` and lives as long as
        // the tap.
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
            // SAFETY: `ctx` comes from `Box::into_raw` above and is still the
            // only reference at this point.
            drop(unsafe { Box::from_raw(ctx) });
            return Err(ListenError::EventTap);
        }

        // SAFETY: `tap` is non-null (checked above); a NULL allocator means
        // the default allocator; the call returns a +1 reference, which is
        // handed to the run loop below and then given back.
        let source = unsafe { CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0) };
        if source.is_null() {
            // SAFETY: `tap` is the +1 reference returned by `CGEventTapCreate`
            // above, owned by us to release; `ctx` is still the only reference
            // obtained from `Box::into_raw`.
            unsafe {
                CFRelease(tap.cast::<c_void>());
                drop(Box::from_raw(ctx));
            }
            return Err(ListenError::RunLoopSource);
        }
        // SAFETY: this fetches the current thread's run loop without
        // transferring ownership; `source` is valid, and after it is added
        // the run loop holds its own copy, so our +1 reference is released
        // right after.
        unsafe {
            let run_loop = CFRunLoopGetCurrent();
            CFRunLoopAddSource(run_loop, source, kCFRunLoopCommonModes);
            CFRelease(source.cast::<c_void>());
            CGEventTapEnable(tap, true);
        }
        Ok(TapHandle)
    }

    pub(super) fn run(_handle: TapHandle) {
        // SAFETY: `create` has already attached the source to the current
        // thread's run loop; this only enters the blocking run. The argument
        // carries only a two-phase ordering constraint and is spent once the
        // run begins.
        unsafe { CFRunLoopRun() };
    }

    unsafe extern "C" fn raw_callback(
        _proxy: CGEventTapProxy,
        raw_type: u32,
        event: CGEventRef,
        user_info: *mut c_void,
    ) -> CGEventRef {
        if let Some(action) = classify(raw_type) {
            // SAFETY: `user_info` is the `Box<dyn Fn(ButtonEvent)>` pointer
            // passed in by `create` via `Box::into_raw`, intentionally leaked
            // until process exit; the C side never releases it.
            let sink = unsafe { &*(user_info as *const Box<dyn Fn(ButtonEvent)>) };
            // SAFETY: `event` is an event reference lent to us by the system
            // for the duration of the callback; we only read the location and
            // timestamp, without taking ownership.
            let CGPoint { x, y } = unsafe { CGEventGetLocation(event) };
            // SAFETY: same as above, `event` is valid for the duration of the
            // callback; reading the timestamp is a pure read.
            let time = unsafe { CGEventGetTimestamp(event) };
            sink(ButtonEvent {
                action,
                pos: (x, y),
                time: Duration::from_nanos(time),
            });
        }
        event
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let mask = imp::subscribed_mask();
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

    #[test]
    fn spawn_resolves_synchronously_and_startup_failure_stays_off_the_flag() {
        let (source, degraded) = MouseSource::spawn();
        assert!(
            !degraded.load(std::sync::atomic::Ordering::Relaxed),
            "startup failure must not set the mid-run flag"
        );
        drop(source);
    }
}

#[cfg(test)]
mod injected_gesture_live_tests {
    use std::time::{Duration, Instant};

    use gloss_core::ports::SelectionGestureSource;

    use super::MouseSource;
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
