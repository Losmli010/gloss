//! 桌面入口装配（系统（Windows））：保证桌面快捷方式指向当前可执行文件。
//!
//! 便携压缩包即安装形态，可执行文件落到哪里，桌面入口就应在哪里：每次
//! 启动幂等重建一份快捷方式（外壳链接），目标与图标都取当前可执行文件
//! ——应用原位更新后路径不变，链接自然保持有效；可执行文件被挪动或更新
//! 换位后，重建让入口始终跟随实际位置，省去旧链接的读回比对。可执行文件
//! 本身已在桌面上时不建自指链接。链接名取打包名（Gloss），失败只向调用方
//! 报错（降级路径），不阻断启动。
//!
//! 线程约束：在界面主线程的启动装配段调用，组件对象模型（COM）套间由
//! 外部函数接口（FFI）封装按需补初始化（见外壳链接模块头）。

use std::env;

use gloss_core::model::GlossError;
use gloss_core::ports::DesktopShortcut;

use crate::windows::ffi;

const LINK_FILE_NAME: &str = "Gloss.lnk";

pub struct SystemDesktopShortcut;

impl DesktopShortcut for SystemDesktopShortcut {
    fn ensure(&self) -> Result<(), GlossError> {
        let exe = env::current_exe()
            .map_err(|err| GlossError::Config(format!("locate running executable: {err}")))?;
        let desktop = ffi::shell_link::desktop_dir()?;
        if let Some(parent) = exe.parent() {
            let same = parent
                .to_string_lossy()
                .eq_ignore_ascii_case(&desktop.to_string_lossy());
            if same {
                return Ok(());
            }
        }
        ffi::shell_link::write_shortcut(&desktop.join(LINK_FILE_NAME), &exe)
    }
}
