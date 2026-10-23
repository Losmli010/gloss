//! 系统字体发现：CoreText 家族定位与字体文件字节解析（ttc 面序号）。

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

/// 依候选顺序查系统 CJK 字体，命中第一个就返回它的文件字节与面序号。
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

/// 依候选顺序定位第一个可用的系统字体家族；全部落空只留 debug 痕
/// （宋楷是排版偏好，缺席不是故障）。
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

/// 装载一个字体文件的字节背衬：mmap 文件背页优先，egui 登记的借用
/// 切片恒指回文件页、字节不进进程私有堆；映射失败（空文件、系统调用
/// 失败）留 warn 后回退 `fs::read` 整包拷贝——降级带痕迹、不阻塞启
/// 动。两条路都失败才报错。
fn load_backing(path: &Path) -> Result<FontBacking, String> {
    let mapped = File::open(path)
        .map_err(|err| err.to_string())
        .and_then(|file| {
            // SAFETY: 映射的是系统字体文件——由系统管理、本进程从不
            // 写入，进程生命期内不被截断；映射长度即打开时的文件长度，
            // 切片读取不会越出映射页。
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

/// 家族集合里选常规面：先比风格修正位（粗斜宽窄，越少越常规），再比
/// 权重离常规（0）的距离——与 font-kit 时代 Weight NORMAL + Style
/// NORMAL 的取向一致。集合缺席或为空按无此族处理。
///
/// 查询走字体集合（只扫**已安装**字体，font-kit 同款形状）：描述符
/// 匹配会命中系统的「可下载字体」目录，查询缺席家族（未随机型的楷
/// 体等）会弹字体下载确认框。
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

/// 目标面在字体文件内的序号：单面文件恒 0；ttc 集合按头部面数逐面
/// 解析 name 表找 postscript 名的命中——序号完全由文件自身回答（与
/// egui 侧解析器同源同约定），找不到即失败降级。不做 CoreText 的
/// 文件枚举：那在无窗口服务的 CI runner 上每次要挂十几分钟（fontd
/// 病态），纯字节解析是微秒级。
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

/// 文件是否为 TrueType 集合（ttc/otc 共用的 `ttcf` 魔数）。
pub(super) fn is_collection(bytes: &[u8]) -> bool {
    bytes.first_chunk::<4>() == Some(b"ttcf")
}

/// ttc 头部的面数（大端 u32，+8）；头部损坏按零面处理。
pub(super) fn num_fonts(bytes: &[u8]) -> u32 {
    u32_at(bytes, 8).unwrap_or(0)
}

/// ttc 头部第 `index` 个面的目录偏移（大端 u32，从 +12 起）。
pub(super) fn face_offset(bytes: &[u8], index: usize) -> Result<usize, String> {
    u32_at(bytes, 12 + 4 * index)
        .map(|raw| raw as usize)
        .ok_or_else(|| "ttc face offset out of range".to_owned())
}

/// 在 `face` 处的 sfnt 表目录里定位 name 表。TTC 的表偏移按规范从
/// 文件头量起（实测 Apple 系统 ttc 即如此），与 egui 侧字形解析器
/// （skrifa）支持的唯一约定一致——面相对等其它约定一概不认，免得
/// 复核通过却登记一张解析不出的面。
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

/// 解析 `face` 目录指向那面的 name 表，取 postscript 名（nameID 6；
/// 优先 Windows 平台 UTF-16BE，退回 Macintosh ASCII）。只为
/// [`face_index`] 的序号定位服务，解析不了返回 `None`。
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
