//! 应用图标适配器（系统（Windows）占位），实现图标端口：装配即报告未支持。
//!
//! 任务栏图标来自打包资源，无程序坞（Dock）式的运行时装配面；真实现
//! 按移植方案落为无操作或任务栏覆盖，本桩先行占位。

use gloss_core::model::GlossError;
use gloss_core::ports::AppIcon;

#[derive(Debug, Default, Clone, Copy)]
pub struct MacAppIcon;

impl MacAppIcon {
    pub fn new() -> Self {
        Self
    }
}

impl AppIcon for MacAppIcon {
    fn install(&self, _png: &[u8]) -> Result<(), GlossError> {
        Err(GlossError::PlatformUnsupported)
    }
}
