//! 应用图标适配器（Windows）：任务栏图标来自打包资源，端口实现为无操作。
//!
//! 系统（Windows）无程序坞（Dock）式的运行时图标装配面：任务栏与开始
//! 菜单的图标在打包期由可执行文件资源承担（图标文件随分发打包落地），
//! 运行期没有要补装的东西。端口实现保持恒成功的无操作，调用方的启动序
//! 不因平台差异走失败分支。

use gloss_core::model::GlossError;
use gloss_core::ports::AppIcon;

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemAppIcon;

impl SystemAppIcon {
    pub fn new() -> Self {
        Self
    }
}

impl AppIcon for SystemAppIcon {
    fn install(&self, _png: &[u8]) -> Result<(), GlossError> {
        Ok(())
    }
}
