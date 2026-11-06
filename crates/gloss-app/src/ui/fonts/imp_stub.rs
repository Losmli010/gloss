//! 系统字体发现（其余目标占位）：恒无发现，命名字体族兜底到内置字形。
//!
//! 系统（macOS）与系统（Windows）各持真实发现器（imp.rs 与 imp_win.rs）；
//! 本桩服务其余目标：不读系统，父模块的既有降级路径（候选全缺席）照常
//! 成立——界面库缺省字形接管，中文以外文本不受影响。家族字体与字节背衬
//! 在此同名镜像，供父模块的登记流程以同一类型面编译；桩内无构造方，故
//! 放开死码判定。
#![allow(dead_code)]

#[cfg(test)]
pub(super) const DISCOVERY_PRESENT: bool = false;

use std::ops::Deref;

use memmap2::Mmap;

#[derive(Debug)]
pub(super) enum FontBacking {
    Mapped(Mmap),
    Copied(Vec<u8>),
}

impl FontBacking {
    pub(super) fn as_slice(&self) -> &[u8] {
        match self {
            FontBacking::Mapped(mapped) => mapped,
            FontBacking::Copied(bytes) => bytes,
        }
    }
}

impl Deref for FontBacking {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        self.as_slice()
    }
}

pub(super) struct FamilyFont {
    pub(super) bytes: FontBacking,
    pub(super) index: u32,
}

impl FamilyFont {
    #[cfg(test)]
    pub(super) fn from_bytes(bytes: Vec<u8>, index: u32) -> FamilyFont {
        FamilyFont {
            bytes: FontBacking::Copied(bytes),
            index,
        }
    }
}

pub(super) fn load_cjk(_families: &[&str]) -> Option<FamilyFont> {
    None
}

pub(super) fn load_family(_families: &[&str]) -> Option<FamilyFont> {
    None
}
