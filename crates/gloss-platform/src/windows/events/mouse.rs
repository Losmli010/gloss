//! 全局鼠标监听（系统（Windows）绑定）：安装只按消息号收左键按下与释放的
//! 低级鼠标钩子，钩子回调把消息原样翻译进共享手势事件并经线程局部槽入队。
//! 手势判定与监听线程脚手架住平台中立共享层（common），本模块只提供事件
//! 创建与宿主运行两个闭包：创建相在当前线程安装钩子，宿主相阻塞跑消息泵。
//!
//! 线程约束：低级鼠标钩子的回调在安装线程取送消息时被系统调用，监听在
//! 独立线程建立并跑消息泵，不占平台事件线程；钩子回调内只做事件搬运（队列
//! 满则由共享汇点丢弃），手势判定在平台事件线程的轮询侧完成。没有停止接口，
//! 监听线程随进程退出消亡；运行中失效（泵退出）经标志异步置位，建立结果
//! 同步返回。钩子无需系统授权。

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::common::gesture::LeftButton;

pub use crate::common::gesture::MouseSource;

const WM_L_DOWN: u32 = 0x0201;

const WM_L_UP: u32 = 0x0202;

#[cfg(test)]
const WM_KEY_DOWN: u32 = 0x0100;

#[cfg(test)]
const WM_MOUSE_MOVE: u32 = 0x0200;

#[cfg(test)]
const WM_R_DOWN: u32 = 0x0204;

fn classify(message: u32) -> Option<LeftButton> {
    match message {
        WM_L_DOWN => Some(LeftButton::Pressed),
        WM_L_UP => Some(LeftButton::Released),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ListenError {
    Hook,
}

impl MouseSource {
    pub fn spawn() -> (Option<Self>, Arc<AtomicBool>) {
        Self::spawn_listening(imp::create, imp::run)
    }
}

mod imp {
    use std::cell::RefCell;
    use std::time::Duration;

    use crate::common::gesture::{ButtonEvent, Sink};
    use crate::ffi::hook::{self, LPARAM, LRESULT, MSG, WPARAM};

    use super::ListenError;

    thread_local! {
        static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
    }

    pub struct TapHandle {
        _hook: hook::MouseHook,
    }

    pub(super) fn create(sink: Sink) -> Result<TapHandle, ListenError> {
        SINK.with(|cell| *cell.borrow_mut() = Some(sink));
        match hook::MouseHook::install(Some(hook_procedure)) {
            Ok(instance) => Ok(TapHandle { _hook: instance }),
            Err(_) => Err(ListenError::Hook),
        }
    }

    pub(super) fn run(_handle: TapHandle) {
        loop {
            let mut message = MSG::default();
            if let hook::PumpFlow::Exit = hook::pump_once(&mut message) {
                break;
            }
            hook::dispatch(&message);
        }
    }

    unsafe extern "system" fn hook_procedure(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code < 0 {
            return hook::next_hook(code, wparam, lparam);
        }
        if let Some((message, x, y, time)) = hook::low_level_mouse(wparam, lparam)
            && let Some(action) = super::classify(message)
        {
            SINK.with(|cell| {
                if let Some(sink) = cell.borrow().as_ref() {
                    sink(ButtonEvent {
                        action,
                        pos: (x, y),
                        time: Duration::from_millis(time),
                    });
                }
            });
        }
        hook::next_hook(code, wparam, lparam)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
