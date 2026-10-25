//! 系统字体发现：字体家族定位（CoreText）与字体文件字节解析，含字体集合（ttc）的面序号。

use std::fs::File;
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

use super::{CJK_FAMILIES, FONT_NAME, FamilyFont, FontBacking};

pub(super) fn load_cjk() -> Option<FamilyFont> {
    load_family(CJK_FAMILIES).inspect(|font| {
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

pub(super) fn is_collection(bytes: &[u8]) -> bool {
    bytes.first_chunk::<4>() == Some(b"ttcf")
}

pub(super) fn num_fonts(bytes: &[u8]) -> u32 {
    u32_at(bytes, 8).unwrap_or(0)
}

pub(super) fn face_offset(bytes: &[u8], index: usize) -> Result<usize, String> {
    u32_at(bytes, 12 + 4 * index)
        .map(|raw| raw as usize)
        .ok_or_else(|| "ttc face offset out of range".to_owned())
}

fn locate_name_table(bytes: &[u8], face: usize) -> Option<usize> {
    let num_tables = u16_at(bytes, face + 4)?;
    for i in 0..num_tables as usize {
        let record = face + 12 + 16 * i;
        if bytes.get(record..record + 4)? != b"name" {
            continue;
        }
        let at = u32_at(bytes, record + 8)? as usize;
        return (matches!(u16_at(bytes, at), Some(0 | 1))
            && u16_at(bytes, at + 2).unwrap_or(0) > 0)
            .then_some(at);
    }
    None
}

pub(super) fn face_postscript_name(bytes: &[u8], face: usize) -> Option<String> {
    let name_at = locate_name_table(bytes, face)?;
    let count = u16_at(bytes, name_at + 2)? as usize;
    let strings_at = name_at + u16_at(bytes, name_at + 4)? as usize;
    let mut ascii = None;
    for i in 0..count {
        let entry = name_at + 6 + 12 * i;
        let platform = u16_at(bytes, entry)?;
        let id = u16_at(bytes, entry + 6)?;
        if id != 6 {
            continue;
        }
        let length = u16_at(bytes, entry + 8)? as usize;
        let at = strings_at + u16_at(bytes, entry + 10)? as usize;
        let raw = bytes.get(at..at + length)?;
        if platform == 3 {
            let units: Vec<u16> = raw
                .chunks_exact(2)
                .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                .collect();
            return String::from_utf16(&units).ok();
        }
        if platform == 1 && ascii.is_none() && raw.is_ascii() {
            ascii = Some(raw.iter().map(|&byte| byte as char).collect::<String>());
        }
    }
    ascii
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}
