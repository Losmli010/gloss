//! 系统字体发现（其余目标占位）：恒无发现，命名字体族兜底到内置字形。
//!
//! 系统（macOS）与系统（Windows）各持真实发现器（imp.rs 与 imp_win.rs）；
//! 本桩服务其余目标：不读系统，父模块的既有降级路径（候选全缺席）照常
//! 成立——界面库缺省字形接管，中文以外文本不受影响。家族字体与字节背衬
//! 在共享背衬模块（backing），桩只交回“无发现”。

#[cfg(test)]
pub(super) const DISCOVERY_PRESENT: bool = false;

use super::backing::FamilyFont;

pub(super) fn load_cjk(_families: &[&str]) -> Option<FamilyFont> {
    None
}

pub(super) fn load_family(_families: &[&str]) -> Option<FamilyFont> {
    None
}
