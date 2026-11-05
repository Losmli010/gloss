//! 前台窗口进程查询：敏感应用闸门的前台应用探针。
//!
//! 前台窗口取不到（安全桌面（UAC）、锁屏、会话切换）时返回空——闸门按
//! “无事实”放行，探针读不到不该升级成拦截。查询只取进程映像路径与进程
//! 标识，不读窗口标题、不碰用户内容。

use windows::Win32::Foundation::CloseHandle;

use gloss_core::log::{debug, thread};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
use windows::core::PWSTR;

const IMAGE_PATH_CHARS: usize = 1024;

pub(crate) fn foreground_image_name() -> Option<(String, u32)> {
    // SAFETY: a pure query with no preconditions; an invalid return means no
    // foreground window right now.
    let window = unsafe { GetForegroundWindow() };
    if window.is_invalid() {
        return None;
    }
    let mut pid = 0u32;
    // SAFETY: `window` is valid as returned above and the out parameter points
    // to the stack variable just declared; the thread id return is not needed.
    unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
    if pid == 0 {
        return None;
    }
    // SAFETY: query-limited rights only, so the call needs no elevation; the
    // returned handle is closed on every path below.
    let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) })
    else {
        return None;
    };
    let mut buffer = [0u16; IMAGE_PATH_CHARS];
    let mut len = buffer.len() as u32;
    // SAFETY: the handle is valid and read-only in intent; the buffer and its
    // length are consistent, and the size is updated in place on success.
    let queried = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        )
    };
    // SAFETY: `process` is the +1 handle returned by `OpenProcess` above and
    // this is the sole release point, before any early return.
    if let Err(err) = unsafe { CloseHandle(process) } {
        debug!(thread = thread::EVENT, error = %err, "close process handle failed");
    }
    queried.ok()?;
    if len == 0 {
        return None;
    }
    Some((String::from_utf16_lossy(&buffer[..len as usize]), pid))
}
