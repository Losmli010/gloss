//! 家族字体与字节背衬：两侧系统发现器共用的类型面与文件装载。
//!
//! 字节背衬以内存映射优先、整包拷贝兜底——界面库（egui）登记的借用
//! 切片恒指回文件页，字节不进进程私有堆；映射失败留痕后回退整包拷贝。
//! 两种形态只由发现器构造，装载失败交回调用方降级，本模块不决定降级
//! 走向。

use std::fs::File;
use std::ops::Deref;
use std::path::Path;

use gloss_core::log::{thread, warn};
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

pub(super) fn load_backing(path: &Path) -> Result<FontBacking, String> {
    let mapped = File::open(path)
        .map_err(|err| err.to_string())
        .and_then(|file| {
            // SAFETY: The file being mapped is a system font file: it is managed
            // by the system, never written to by this process, and never
            // truncated during the lifetime of the process; the mapping length
            // equals the file length at open time, so slice reads cannot run
            // past the mapped pages.
            unsafe { Mmap::map(&file) }.map_err(|err| err.to_string())
        });
    match mapped {
        Ok(backing) => return Ok(FontBacking::Mapped(backing)),
        Err(err) => warn!(
            thread = thread::FONTS,
            path = %path.display(),
            error = err,
            "font file mmap failed, falling back to full copy"
        ),
    }
    std::fs::read(path)
        .map(FontBacking::Copied)
        .map_err(|err| err.to_string())
}
