//! 事件循环装配的系统差异项（其余目标占位）：无缺省应用菜单可关。
//!
//! 窗口库（winit）只在系统（macOS）挂缺省应用菜单，其余目标没有这一
//! 步；装配器原样返回，与系统（macOS）实现的同名模块同签名。

use winit::event_loop::EventLoopBuilder;

use super::events::UserEvent;

pub(crate) fn suppress_default_menu(_builder: &mut EventLoopBuilder<UserEvent>) {}
