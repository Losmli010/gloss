//! 事件权限的预检与授权引导（Windows）：系统（Windows）无系统级授权门。
//!
//! 选区主路走用户界面自动化（UIA），不索取任何系统授权：预检恒放行、
//! 引导为无操作、授权监视恒不触发——移植方案能力映射的定案。

use gloss_core::ports::PermissionProbe;

pub struct SystemPermissionProbe;

impl PermissionProbe for SystemPermissionProbe {
    fn preflight(&self) -> bool {
        true
    }

    fn request(&self) -> bool {
        true
    }

    fn watch(&self, _grant_pending: bool) -> Box<dyn FnMut() -> bool + Send> {
        Box::new(|| false)
    }
}
