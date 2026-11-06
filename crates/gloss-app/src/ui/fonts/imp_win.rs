//! 系统字体发现（Windows）：字体栈（DirectWrite）枚举优先，字体目录固定候选探测退化。
//!
//! 经字体栈（DirectWrite）在系统字体集合里按家族名定位常规面，面序号直接取回；
//! 文件引用经本地字体文件加载器还原成路径后，字节一次性读进界面库（egui）。
//! 枚举不可用（字体服务异常等）时，中日韩后备退化为探测字体目录的固定候选
//! 文件，面序号按候选文件内的已知布局取值。家族字体与字节背衬的类型定义在
//! 本模块：字节背衬以内存映射优先、整包拷贝兜底，两种形态只由本模块的发现器
//! 构造。查找失败只降级不阻塞启动：枚举与候选文件全落空时交回“无发现”，
//! 命名字体族由父模块的既有降级路径兜到内置字形，与平台发现器族的约定一致。

#[cfg(test)]
pub(super) const DISCOVERY_PRESENT: bool = true;

use std::fs::File;
use std::ops::Deref;
use std::path::{Path, PathBuf};

use gloss_core::log::{debug, info, thread, warn};
use memmap2::Mmap;
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_NORMAL, DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection,
    IDWriteFontFace, IDWriteLocalFontFileLoader,
};
use windows::core::{BOOL, HSTRING, Interface};

use super::FONT_NAME;

const FONTS_DIR: &str = "C:\\Windows\\Fonts";

const CJK_FALLBACK_FILES: &[(&str, u32)] = &[("msyh.ttc", 0), ("simsun.ttc", 0)];

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
    let located = match load_family(families) {
        Some(font) => Some(font),
        None => {
            debug!(
                thread = thread::FONTS,
                "DirectWrite enumeration exhausted, probing fixed font files"
            );
            probe_cjk_files()
        }
    };
    located.inspect(|font| {
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
    let collection = system_collection()?;
    for family in families {
        match family_face(&collection, family) {
            Ok(Some(font)) => return Some(font),
            Ok(None) => debug!(thread = thread::FONTS, family, "font family not matched"),
            Err(err) => warn!(
                thread = thread::FONTS,
                family,
                error = err,
                "matched font family failed to load"
            ),
        }
    }
    debug!(
        thread = thread::FONTS,
        ?families,
        "exhausted font family candidates"
    );
    None
}

fn probe_cjk_files() -> Option<FamilyFont> {
    for (file, index) in CJK_FALLBACK_FILES {
        let path = Path::new(FONTS_DIR).join(file);
        match load_backing(&path) {
            Ok(bytes) => {
                return Some(FamilyFont {
                    bytes,
                    index: *index,
                });
            }
            Err(err) => debug!(
                thread = thread::FONTS,
                file,
                error = err,
                "fixed font candidate unavailable"
            ),
        }
    }
    None
}

fn system_collection() -> Option<IDWriteFontCollection> {
    let factory = match create_factory() {
        Ok(factory) => factory,
        Err(err) => {
            warn!(
                thread = thread::FONTS,
                error = %err,
                "DirectWrite factory unavailable"
            );
            return None;
        }
    };
    let mut collection: Option<IDWriteFontCollection> = None;
    // SAFETY: The call only reads the factory interface and writes an owned
    // collection reference into the out-parameter prepared here; no other
    // pointers are passed or retained.
    if let Err(err) = unsafe { factory.GetSystemFontCollection(&mut collection, false) } {
        warn!(
            thread = thread::FONTS,
            error = %err,
            "system font collection unavailable"
        );
        return None;
    }
    collection
}

fn create_factory() -> windows::core::Result<IDWriteFactory> {
    // SAFETY: The shared DirectWrite factory is a process-wide singleton and
    // the call hands an owned reference of it to the caller; the call creates
    // no COM apartment and takes no pointers beyond the type selector.
    unsafe { DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED) }
}

fn family_face(
    collection: &IDWriteFontCollection,
    family: &str,
) -> Result<Option<FamilyFont>, String> {
    let name = HSTRING::from(family);
    let mut index = 0u32;
    let mut exists = BOOL::default();
    // SAFETY: The name string stays alive for the duration of the call, and
    // the out-parameters are plain integers prepared here; the call writes no
    // pointers.
    unsafe { collection.FindFamilyName(&name, &mut index, &mut exists) }
        .map_err(|err| err.to_string())?;
    if !exists.as_bool() {
        return Ok(None);
    }
    // SAFETY: The index came from the preceding `FindFamilyName` hit, which is
    // within the collection's family count by contract.
    let family = unsafe { collection.GetFontFamily(index) }.map_err(|err| err.to_string())?;
    // SAFETY: The call only reads the family interface and returns an owned
    // reference to the requested regular face within it.
    let font = unsafe {
        family.GetFirstMatchingFont(
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
        )
    }
    .map_err(|err| err.to_string())?;
    // SAFETY: The call only reads the font interface and returns an owned face
    // reference to the caller.
    let face = unsafe { font.CreateFontFace() }.map_err(|err| err.to_string())?;
    // SAFETY: The call only reads the face interface and reports the face's
    // index within its underlying file.
    let index = unsafe { face.GetIndex() };
    let path = face_file_path(&face)?;
    let bytes = load_backing(&path)?;
    Ok(Some(FamilyFont { bytes, index }))
}

fn face_file_path(face: &IDWriteFontFace) -> Result<PathBuf, String> {
    let mut count = 0u32;
    // SAFETY: With no output buffer the call only reports the file count into
    // the out-parameter prepared here.
    unsafe { face.GetFiles(&mut count, None) }.map_err(|err| err.to_string())?;
    if count == 0 {
        return Err("font face has no file references".to_owned());
    }
    let mut files = vec![None; count as usize];
    // SAFETY: The buffer holds exactly the slot count reported by the
    // preceding call, and the callee fills each slot with an owned file
    // reference handed to this function.
    unsafe { face.GetFiles(&mut count, Some(files.as_mut_ptr())) }
        .map_err(|err| err.to_string())?;
    let Some(file) = files[0].take() else {
        return Err("font file slot was not filled".to_owned());
    };
    let mut key: *mut core::ffi::c_void = std::ptr::null_mut();
    let mut key_size = 0u32;
    // SAFETY: Both out-parameters are prepared here; the key is owned by the
    // font file object, which outlives every use of the key in this scope.
    unsafe { file.GetReferenceKey(&mut key, &mut key_size) }.map_err(|err| err.to_string())?;
    // SAFETY: The call only reads the file interface and returns an owned
    // reference to the loader that backs this file's key.
    let loader = unsafe { file.GetLoader() }.map_err(|err| err.to_string())?;
    let local: IDWriteLocalFontFileLoader = loader.cast().map_err(|err| err.to_string())?;
    // SAFETY: The key pointer with its size came from `GetReferenceKey` on the
    // same file object, and the call only reads that key.
    let length =
        unsafe { local.GetFilePathLengthFromKey(key, key_size) }.map_err(|err| err.to_string())?;
    let mut path = vec![0u16; length as usize + 1];
    // SAFETY: The buffer holds the length reported by the preceding length
    // query plus room for the terminator, and the key is the same reference
    // this scope obtained from the file object.
    unsafe { local.GetFilePathFromKey(key, key_size, &mut path) }.map_err(|err| err.to_string())?;
    path.truncate(length as usize);
    Ok(PathBuf::from(String::from_utf16_lossy(&path)))
}

fn load_backing(path: &Path) -> Result<FontBacking, String> {
    let mapped = File::open(path)
        .map_err(|err| err.to_string())
        .and_then(|file| {
            // SAFETY: The file being mapped is a system font file: it is
            // managed by the system, never written to by this process, and
            // never truncated during the lifetime of the process; the mapping
            // length equals the file length at open time, so slice reads
            // cannot run past the mapped pages.
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
