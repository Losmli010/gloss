//! 低级鼠标钩子与消息泵：钩子安装拆卸、消息取送与线程节拍定时器。
//!
//! 低级钩子的回调在安装线程取送消息时被系统调用：安装线程必须持续运行
//! 消息泵，否则回调停摆、钩子被系统摘除。线程归属与线程名约束见鼠标手势
//! 源模块头；本模块只给原语，泵的循环结构与消息分派决策归宿主模块。

pub(crate) use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
pub(crate) use windows::Win32::UI::WindowsAndMessaging::{MSG, WM_TIMER};

use gloss_core::log::{debug, thread};

use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HHOOK, HOOKPROC, KillTimer, MSLLHOOKSTRUCT,
    SetTimer, SetWindowsHookExW, UnhookWindowsHookEx, WH_MOUSE_LL,
};

#[derive(Debug)]
pub(crate) struct MouseHook(HHOOK);

impl MouseHook {
    pub(crate) fn install(procedure: HOOKPROC) -> windows::core::Result<Self> {
        // SAFETY: a null module-name query returns this process's own image
        // handle, which stays valid for the process lifetime, as the hook's
        // code module requires.
        let module = unsafe { GetModuleHandleW(None) }?;
        // SAFETY: `procedure` is a valid hook callback that the caller keeps
        // alive for the hook's lifetime; a zero thread id installs a global
        // low-level hook, and the returned handle is unhooked exactly once in
        // `Drop`.
        let hook = unsafe { SetWindowsHookExW(WH_MOUSE_LL, procedure, Some(module.into()), 0) }?;
        Ok(Self(hook))
    }
}

impl Drop for MouseHook {
    fn drop(&mut self) {
        // SAFETY: `self.0` is the handle returned by `SetWindowsHookExW` and
        // no other code unhooks it.
        if let Err(err) = unsafe { UnhookWindowsHookEx(self.0) } {
            debug!(thread = thread::MOUSE_TAP, error = %err, "unhook mouse failed");
        }
    }
}

pub(crate) fn next_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: pure forwarding of the hook callback's own arguments per the
    // low-level hook contract; no handle is consumed.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

pub(crate) fn low_level_mouse(wparam: WPARAM, lparam: LPARAM) -> Option<(u32, f64, f64, u64)> {
    // SAFETY: for a mouse hook the `lparam` always points to an
    // `MSLLHOOKSTRUCT` that is valid for the duration of the callback; we
    // only read its fields.
    let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
    Some((
        wparam.0 as u32,
        f64::from(info.pt.x),
        f64::from(info.pt.y),
        u64::from(info.time),
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PumpFlow {
    Continue,
    Exit,
}

pub(crate) fn pump_once(msg: &mut MSG) -> PumpFlow {
    // SAFETY: `msg` is a valid out buffer for exactly one message; a null
    // window handle fetches from this thread's own queue. The contract says
    // the return value is zero on WM_QUIT and -1 on error; both end the pump,
    // which also keeps -1 from being mistaken for "got a message".
    let code = unsafe { GetMessageW(msg, None, 0, 0) }.0;
    if code == 0 || code == -1 {
        PumpFlow::Exit
    } else {
        PumpFlow::Continue
    }
}

pub(crate) fn dispatch(msg: &MSG) {
    // SAFETY: `msg` is a message just fetched by `GetMessageW` on this thread,
    // dispatched per the standard pump order for this thread's windows.
    unsafe { DispatchMessageW(msg) };
}

#[derive(Debug)]
pub(crate) struct TickTimer {
    id: usize,
}

impl TickTimer {
    pub(crate) fn install(id: usize, period_ms: u32) -> Option<Self> {
        // SAFETY: a null window handle creates a thread-queue timer that posts
        // to the calling thread; a null callback delivers WM_TIMER instead, and
        // the returned id is zero only on failure.
        let timer = unsafe { SetTimer(None, id, period_ms, None) };
        (timer != 0).then_some(Self { id: timer })
    }

    pub(crate) fn id(&self) -> usize {
        self.id
    }
}

impl Drop for TickTimer {
    fn drop(&mut self) {
        // SAFETY: `self.id` is the timer id returned by `SetTimer` on this
        // thread and the timer is killed exactly once, on the same thread.
        if let Err(err) = unsafe { KillTimer(None, self.id) } {
            debug!(thread = thread::EVENT, error = %err, "kill tick timer failed");
        }
    }
}
