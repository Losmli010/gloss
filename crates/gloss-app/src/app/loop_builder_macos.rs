//! 事件循环装配的系统差异项（系统（macOS））：关掉窗口库（winit）缺省应用菜单。
//!
//! 窗口库在系统（macOS）默认挂一份英文缺省菜单，与自建应用菜单并存；
//! 装配期显式关闭，菜单面完全交给菜单模块。其余目标无缺省菜单，装配器
//! 原样返回（同名模块）。

use winit::event_loop::EventLoopBuilder;

use super::events::UserEvent;

pub(crate) fn suppress_default_menu(builder: &mut EventLoopBuilder<UserEvent>) {
    use winit::platform::macos::EventLoopBuilderExtMacOS;

    builder.with_default_menu(false);
}
