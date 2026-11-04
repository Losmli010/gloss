//! 应用菜单（系统（Windows）占位）：托盘装配的桩。
//!
//! 系统（macOS）的坞菜单在系统（Windows）对应托盘图标加托盘菜单
//! （tray-icon），属行为差异项，按移植方案后续阶段补齐；本桩报告未装配
//! ——壳侧照常按“无菜单栏运行”降级，设置入口仍可经浮层动作打开。

use gloss_core::model::Locale;

pub fn install(_locale: Locale, _on_open_settings: impl Fn() + Send + 'static) -> bool {
    false
}
