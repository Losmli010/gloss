//! 系统字体发现（macOS）：字体家族定位与字体文件字节解析。
//!
//! 经系统文本栈（CoreText）按家族名取常规面描述符，文件路径落地后
//! 一次性读进界面库（egui），面序号按声明序号的字体名（PostScript）回答——
//! 集合文件的面序解析原语在字体集合解析模块（collection）。家族字体与
//! 字节背衬的类型定义在本模块：字节背衬以内存映射优先、整包拷贝兜底，
//! 两种形态只由本模块的发现器构造。

#[cfg(test)]
pub(super) const DISCOVERY_PRESENT: bool = true;

use std::fs::File;
use std::ops::Deref;
use std::path::Path;

use core_foundation::array::CFArray;
use core_foundation::base::TCFType;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use core_text::font_collection;
use core_text::font_descriptor::{
    CTFontDescriptor, TraitAccessors, kCTFontBoldTrait, kCTFontCondensedTrait,
    kCTFontExpandedTrait, kCTFontItalicTrait, new_from_attributes,
};
use gloss_core::log::{debug, info, thread, warn};
use memmap2::Mmap;

use super::FONT_NAME;
use super::collection::{face_offset, face_postscript_name, is_collection, u32_at};

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

pub(super) fn load_cjk(families: &[&str]) -> Option<FamilyFont> {
    load_family(families).inspect(|font| {
        info!(
            thread = thread::FONTS,
            font = FONT_NAME,
            bytes = font.bytes.len(),
            face = font.index,
            "located system CJK font"
        );
    })
}

pub(super) fn load_family(families: &[&str]) -> Option<FamilyFont> {
    for family in families {
        let Some(descriptor) = select_regular_face(family) else {
            debug!(thread = thread::FONTS, family, "font family not matched");
            continue;
        };
        let Some(path) = descriptor.font_path() else {
            debug!(
                thread = thread::FONTS,
                family, "matched face has no file path"
            );
            continue;
        };
        let postscript = descriptor.font_name();
        let bytes = match load_backing(&path) {
            Ok(bytes) => bytes,
            Err(err) => {
                warn!(
                    thread = thread::FONTS,
                    family,
                    error = err,
                    path = %path.display(),
                    "failed to load matched font file"
                );
                continue;
            }
        };
        let index = match face_index(&bytes, &postscript) {
            Ok(index) => index,
            Err(err) => {
                warn!(
                    thread = thread::FONTS,
                    family,
                    error = err,
                    path = %path.display(),
                    "failed to locate matched face in file"
                );
                continue;
            }
        };
        return Some(FamilyFont { bytes, index });
    }
    debug!(
        thread = thread::FONTS,
        ?families,
        "exhausted font family candidates"
    );
    None
}

fn load_backing(path: &Path) -> Result<FontBacking, String> {
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

fn select_regular_face(family: &str) -> Option<CTFontDescriptor> {
    const STYLE_PENALTY_MASK: u32 =
        kCTFontBoldTrait | kCTFontItalicTrait | kCTFontExpandedTrait | kCTFontCondensedTrait;
    let attributes = CFDictionary::from_CFType_pairs(&[(
        CFString::new("NSFontFamilyAttribute"),
        CFString::new(family).as_CFType(),
    )]);
    let specified = CFArray::from_CFTypes(&[new_from_attributes(&attributes)]);
    let descriptors = font_collection::new_from_descriptors(&specified).get_descriptors()?;
    let mut best: Option<((u32, i64), CTFontDescriptor)> = None;
    for i in 0..descriptors.len() {
        let Some(descriptor) = descriptors.get(i) else {
            continue;
        };
        let traits = descriptor.traits();
        let style_flags = traits.symbolic_traits() & STYLE_PENALTY_MASK;
        let weight_drift = (traits.normalized_weight().abs() * 100.0) as i64;
        let rank = (style_flags.count_ones(), weight_drift);
        if best
            .as_ref()
            .is_none_or(|(existing_rank, _)| rank < *existing_rank)
        {
            best = Some((rank, (*descriptor).clone()));
        }
    }
    best.map(|(_, descriptor)| descriptor)
}

fn face_index(bytes: &[u8], postscript: &str) -> Result<u32, String> {
    const MAX_SCANNED_FACES: u32 = 1024;
    if !is_collection(bytes) {
        return Ok(0);
    }
    let faces = num_fonts(bytes).min(MAX_SCANNED_FACES) as usize;
    for index in 0..faces {
        let face = face_offset(bytes, index)?;
        if face_postscript_name(bytes, face).as_deref() == Some(postscript) {
            return Ok(index as u32);
        }
    }
    Err(format!(
        "face {postscript} not found in {faces} collection faces"
    ))
}

fn num_fonts(bytes: &[u8]) -> u32 {
    u32_at(bytes, 8).unwrap_or(0)
}
