//! 桌面入口装配（系统（macOS））：装包即集成，无桌面快捷方式概念。
//!
//! 应用包拖入应用程序文件夹即是安装与入口，程序坞（Dock）图标由图标
//! 装配承担，访达（Finder）桌面不放启动链接——移植方案能力映射的定案，
//! 本实现恒为无操作，恒成功。

use gloss_core::model::GlossError;
use gloss_core::ports::DesktopShortcut;

pub struct SystemDesktopShortcut;

impl DesktopShortcut for SystemDesktopShortcut {
    fn ensure(&self) -> Result<(), GlossError> {
        Ok(())
    }
}
