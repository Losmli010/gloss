//! 组件对象模型（COM）：套间初始化与自动化对象创建。
//!
//! 用户界面自动化（UIA）要求单线程套间（STA）且调用线程固定：套间在平台
//! 事件线程的启动序里初始化（线程约束见事件线程模块头），自动化接口对象
//! 只在该线程创建与使用；套间初始化的成功与否由守卫承载，线程退出时按序
//! 卸载。初始化失败只关掉用户界面自动化（UIA）这一路，不阻断事件线程。

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};
use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};

use gloss_core::log::{debug, thread};

#[derive(Debug)]
pub(crate) struct ComApartment {
    owns_init: bool,
}

impl ComApartment {
    pub(crate) fn init_sta() -> Option<Self> {
        // SAFETY: the calling thread is fresh off the thread builder and has no
        // prior COM apartment; the call either registers it as STA or reports
        // the existing mode, and no pointers are passed or returned.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if hr.is_err() {
            debug!(
                thread = thread::EVENT,
                "COM STA init failed, UIA channel disabled"
            );
            return None;
        }
        Some(Self { owns_init: true })
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.owns_init {
            // SAFETY: exactly balances the successful `CoInitializeEx` from
            // `init_sta` on this same thread, as the uninitialize contract
            // requires.
            unsafe { CoUninitialize() };
        }
    }
}

pub(crate) fn create_automation() -> windows::core::Result<IUIAutomation> {
    // SAFETY: the class id is the system UIA automation class and the call
    // creates an in-proc instance with no outer aggregate; the returned
    // interface reference is handed to the caller.
    unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
}
