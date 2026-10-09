//! 系统字体发现与注入：给 egui 补上 CJK 后备字形、经注疏的宋楷字族与
//! 音标的等宽字族。
//!
//! egui 内置字体只覆盖拉丁与常见符号，中文会渲染成豆腐块；这里用 CoreText
//! 从系统里定位一个 CJK 字体，以「最低优先级后备」追加进 egui 的字体族——
//! 拉丁字形仍走内置字体，缺的字形才落到系统字体上。经注疏的宋楷两族（
//! [`FONT_SERIF_NAME`] / [`FONT_KAITI_NAME`]）与等宽族（[`FONT_MONO_NAME`]）
//! 按**命名字体族**注册，不进后备链：只有显式点名的文字（经注疏分区正文、
//! 词卡音标）才用它们，其余文本的字形解析完全不受影响。楷体在系统里缺席时
//! 按 demo 自己的回退链落到宋体（docs/demo/popup-redesign.html 的 --kai 栈
//! 以 Songti SC 收尾）；等宽族按 demo 的 ui-monospace 栈依序探测系统等宽，
//! 族绑定是「系统等宽 + CJK 后备字节」（代码内中文注释仍可读），候选全缺席
//! 时兜底到内置字形。字体字节进程内只向系统读一次文件，浮层与设置窗两个
//! egui 上下文引用同一份；ttc 集合整体登记（egui 按序号取面），不做单面
//! 解包。查找失败只降级不阻塞启动：CJK 后备缺席出 warn，比例/等宽族不接
//! 系统字形（中文暂时不可读）；命名字体族仍恒注册、兜底到内置字形——
//! epaint 对未绑定的字体族直接 panic，族必须永远存在。后续版本可考虑内嵌
//! 开源字体兜底。

use std::borrow::Cow;
use std::sync::{Arc, OnceLock};

use egui::{FontData, FontDefinitions, FontFamily, FontTweak};
use gloss_core::log::{thread, warn};

/// CJK 字体在 egui 字体表里登记的名字。
pub(super) const FONT_NAME: &str = "gloss-cjk";
/// 宋体命名字体族：经/疏正文的排版字体（demo 的 --serif 栈）。
pub(super) const FONT_SERIF_NAME: &str = "gloss-songti";
/// 楷体命名字体族：注正文的排版字体（demo 的 --kai 栈）。
pub(super) const FONT_KAITI_NAME: &str = "gloss-kaiti";
/// 等宽命名字体族：词卡音标的排版字体（demo 的 ui-monospace 栈的落地，
/// 覆盖内置等宽缺的 IPA 音标字形）。
pub(super) const FONT_MONO_NAME: &str = "gloss-mono";

/// 注正文的字体族。
pub(super) fn zhu_family() -> FontFamily {
    FontFamily::Name(FONT_KAITI_NAME.into())
}

/// 经/疏正文的字体族。
pub(super) fn serif_family() -> FontFamily {
    FontFamily::Name(FONT_SERIF_NAME.into())
}

/// 音标的等宽字体族。
pub(super) fn mono_family() -> FontFamily {
    FontFamily::Name(FONT_MONO_NAME.into())
}

/// 一个系统字族的解析结果：整份字体文件的字节与目标面在文件内的序号。
/// 字节是 `fs::read` 的一次拷贝，进程内常驻（缓存于各 `OnceLock`）；
/// ttc 集合整体交给 egui，按序号取面。
struct FamilyFont {
    bytes: Vec<u8>,
    index: u32,
}

/// 把一张系统字面登记进字体表：字节以借用形态挂到 `name` 上。签名要求
/// `'static` 字体——登记的字节必须进程内常驻（生产路径出自 `OnceLock`
/// 缓存），借用的 `Cow` 才不悬垂。
fn insert_face(font: &'static FamilyFont, definitions: &mut FontDefinitions, name: &str) {
    definitions.font_data.insert(
        name.to_owned(),
        Arc::new(FontData {
            font: Cow::Borrowed(&font.bytes),
            index: font.index,
            tweak: FontTweak::default(),
        }),
    );
}

/// 系统 CJK 字体——进程内唯一一份。取用失败同样缓存，不重复查找系统。
static CJK_FONT: OnceLock<Option<FamilyFont>> = OnceLock::new();

/// 宋体/楷体/等宽，各自进程内唯一一份（形态与 [`CJK_FONT`] 相同）。
static SERIF_FONT: OnceLock<Option<FamilyFont>> = OnceLock::new();
static KAITI_FONT: OnceLock<Option<FamilyFont>> = OnceLock::new();
static MONO_FONT: OnceLock<Option<FamilyFont>> = OnceLock::new();

/// 苹方随系统自带，历代候选按可用性排序。
const CJK_FAMILIES: &[&str] = &["PingFang SC", "Hiragino Sans GB", "STHeiti"];

/// 宋体候选（demo --serif 栈的前四项，serif 泛型族不参与精确匹配）。
const SERIF_FAMILIES: &[&str] = &["Songti SC", "STSong", "Noto Serif SC", "SimSun"];

/// 楷体候选（demo --kai 栈的实名项）。
const KAITI_FAMILIES: &[&str] = &["Kaiti SC", "STKaiti", "Kaiti TC", "KaiTi"];

/// 等宽候选（demo ui-monospace 栈的系统落地：macOS 的三代系统等宽在前，
/// 后两项覆盖 Linux/Windows 常见等宽）。
const MONO_FAMILIES: &[&str] = &["SF Mono", "Menlo", "Monaco", "DejaVu Sans Mono", "Consolas"];

/// egui 内置字形的登记名（default_fonts 特性自带）：宋楷两族在系统与
/// CJK 后备双双缺席时的族内兜底，排版降级为默认字形而不是 panic。
pub(super) const BUILTIN_FALLBACK_FONT: &str = "Ubuntu-Light";

/// 带 CJK 后备与宋楷字族的字体定义；bool 表示 CJK 后备是否接上（缺席时
/// 此处出 warn，比例/等宽族中文不可读）。宋楷两族**无条件注册**——
/// epaint 对未绑定的字体族直接 panic——兜底链为 宋/楷系统字体 → 宋体
/// 字节 → CJK 后备字节 → 内置字形，排版逐级降级而不是崩。交给哪个上下文
/// 由 [`super::context`] 的统一装入点决定。
pub(in crate::ui) fn definitions() -> (FontDefinitions, bool) {
    let mut definitions = FontDefinitions::default();
    let cjk = cjk_font();
    if let Some(cjk) = cjk {
        insert_face(cjk, &mut definitions, FONT_NAME);
        append_fallback(&mut definitions);
    } else {
        warn!(
            thread = thread::FONTS,
            "no system CJK font found, CJK text renders without fallback glyphs"
        );
    }

    let serif = SERIF_FONT.get_or_init(|| imp::load_family(SERIF_FAMILIES));
    let kaiti = KAITI_FONT.get_or_init(|| imp::load_family(KAITI_FAMILIES));
    let serif_font = serif.as_ref().or(cjk);
    let kaiti_font = kaiti.as_ref().or(serif_font);
    register_named_or_builtin(&mut definitions, FONT_SERIF_NAME, serif_font);
    register_named_or_builtin(&mut definitions, FONT_KAITI_NAME, kaiti_font);
    register_mono_family(&mut definitions, mono_font(), cjk);
    (definitions, cjk.is_some())
}

/// 仅内置字形的字体定义：命名字体族恒绑定（绑到内置字形，epaint 对未
/// 绑定族直接 panic），系统字体一个不装。启动首帧的快路径——系统字体
/// 装载是秒级的，由 `context::apply_system_fonts` 延迟补装。
pub(in crate::ui) fn builtin_definitions() -> FontDefinitions {
    let mut definitions = FontDefinitions::default();
    for name in [FONT_SERIF_NAME, FONT_KAITI_NAME, FONT_MONO_NAME] {
        definitions.families.insert(
            FontFamily::Name(name.into()),
            vec![BUILTIN_FALLBACK_FONT.to_owned()],
        );
    }
    definitions
}

/// 系统字体是否已装载过（字节级缓存就位）。统一装入点据此选择完整定义
/// 与内置快路径：就位前任何上下文施加都走快路径，系统字体只由延迟装载
/// 一次性补齐。
pub(in crate::ui) fn system_fonts_ready() -> bool {
    CJK_FONT.get().is_some()
        && SERIF_FONT.get().is_some()
        && KAITI_FONT.get().is_some()
        && MONO_FONT.get().is_some()
}

/// 系统 CJK 字体；首次调用向系统取，之后命中缓存。
fn cjk_font() -> Option<&'static FamilyFont> {
    CJK_FONT.get_or_init(imp::load_cjk).as_ref()
}

/// 系统等宽字体；首次调用向系统取，之后命中缓存。
fn mono_font() -> Option<&'static FamilyFont> {
    MONO_FONT
        .get_or_init(|| imp::load_family(MONO_FAMILIES))
        .as_ref()
}

/// 把 [`FONT_NAME`] 以后备（最低优先级）追加进比例与等宽两个字体族：
/// 内置字体先挑，挑不到的字形才轮到系统字体。字节已由 [`cjk_font`]
/// 登记进字体表，这里只补族链。调用前提：`definitions` 里 [`FONT_NAME`]
/// 已注册（[`definitions`] 的装入顺序保证）。
fn append_fallback(definitions: &mut FontDefinitions) {
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        definitions
            .families
            .entry(family)
            .or_default()
            .push(FONT_NAME.to_owned());
    }
}

/// 把一个系统字面注册成命名字体族；缺失时把族绑到内置字形上——族必须
/// 恒存在（epaint 对未绑定的族直接 panic），字形逐级降级。
fn register_named_or_builtin(
    definitions: &mut FontDefinitions,
    name: &'static str,
    font: Option<&'static FamilyFont>,
) {
    match font {
        Some(font) => {
            insert_face(font, definitions, name);
            definitions
                .families
                .insert(FontFamily::Name(name.into()), vec![name.to_owned()]);
        }
        None => {
            definitions.families.insert(
                FontFamily::Name(name.into()),
                vec![BUILTIN_FALLBACK_FONT.to_owned()],
            );
        }
    }
}

/// 等宽命名字体族：族绑定 = 系统等宽 + CJK 后备字节（等宽系统字体不收
/// CJK 字形，链条尾部接上才有代码内中文注释）；系统等宽缺席时绑内置
/// 字形，不从 CJK 降级——比例字形的等宽族会让代码失去对齐，宁可用内置
/// 等宽。`cjk` 为 `Some` 时 [`FONT_NAME`] 必已注册（`definitions` 的装入
/// 顺序保证：后备追加先于本调用）。
fn register_mono_family(
    definitions: &mut FontDefinitions,
    mono: Option<&'static FamilyFont>,
    cjk: Option<&'static FamilyFont>,
) {
    let Some(mono) = mono else {
        definitions.families.insert(
            FontFamily::Name(FONT_MONO_NAME.into()),
            vec![BUILTIN_FALLBACK_FONT.to_owned()],
        );
        return;
    };
    insert_face(mono, definitions, FONT_MONO_NAME);
    let mut chain = vec![FONT_MONO_NAME.to_owned()];
    if cjk.is_some() {
        chain.push(FONT_NAME.to_owned());
    }
    definitions
        .families
        .insert(FontFamily::Name(FONT_MONO_NAME.into()), chain);
}

mod imp {
    use core_foundation::array::CFArray;
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;
    use core_foundation::url::{CFURL, kCFURLPOSIXPathStyle};
    use core_text::font_collection;
    use core_text::font_descriptor::{
        CTFontDescriptor, TraitAccessors, kCTFontBoldTrait, kCTFontCondensedTrait,
        kCTFontExpandedTrait, kCTFontItalicTrait,
    };
    use core_text::font_manager::CTFontManagerCreateFontDescriptorsFromURL;
    use gloss_core::log::{debug, info, thread, warn};

    use super::{CJK_FAMILIES, FONT_NAME, FamilyFont};

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
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(err) => {
                    warn!(
                        thread = thread::FONTS,
                        family,
                        error = %err,
                        path = %path.display(),
                        "failed to read matched font file"
                    );
                    continue;
                }
            };
            let index = match face_index(&bytes, &path, &postscript) {
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

    /// 家族集合里选常规面：先比风格修正位（粗斜宽窄，越少越常规），再比
    /// 权重离常规（0）的距离——与 font-kit 时代 Weight NORMAL + Style
    /// NORMAL 的取向一致。集合缺席或为空按无此族处理。
    fn select_regular_face(family: &str) -> Option<CTFontDescriptor> {
        const STYLE_PENALTY_MASK: u32 =
            kCTFontBoldTrait | kCTFontItalicTrait | kCTFontExpandedTrait | kCTFontCondensedTrait;
        let collection = font_collection::create_for_family(family)?;
        let descriptors = collection.get_descriptors()?;
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

    /// 目标面在字体文件内的序号：单面文件恒 0；ttc 集合按 CoreText 的
    /// 文件枚举找 postscript 名的位次，并用文件自身的 name 表复核该序号
    /// 确实交出同一个名字——枚举位次与文件序号的对齐是经验事实，复核失
    /// 败就放弃该候选，不冒险登记错面。
    fn face_index(bytes: &[u8], path: &std::path::Path, postscript: &str) -> Result<u32, String> {
        if !is_collection(bytes) {
            return Ok(0);
        }
        let descriptors = file_face_descriptors(path)?;
        let count = descriptors.len();
        let mut position = None;
        for i in 0..count {
            if let Some(descriptor) = descriptors.get(i)
                && descriptor.font_name() == postscript
            {
                position = Some(i as usize);
                break;
            }
        }
        let position = position
            .ok_or_else(|| format!("face {postscript} not enumerated in {}", path.display()))?;
        let face = face_offset(bytes, position)?;
        match face_postscript_name(bytes, face) {
            Some(name) if name == postscript => Ok(position as u32),
            found => Err(format!(
                "face {postscript} at directory {position} of {} has name {found:?}",
                path.display()
            )),
        }
    }

    /// 文件是否为 TrueType 集合（ttc/otc 共用的 `ttcf` 魔数）。
    pub(super) fn is_collection(bytes: &[u8]) -> bool {
        bytes.first_chunk::<4>() == Some(b"ttcf")
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
    /// [`face_index`] 的序号复核服务，解析不了返回 `None`。
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

    /// 按文件枚举字体面的描述符数组（次序即文件内面序号，与本仓
    /// PingFang/Songti/Menlo 实测一致）。
    fn file_face_descriptors(path: &std::path::Path) -> Result<CFArray<CTFontDescriptor>, String> {
        let url = CFURL::from_file_system_path(
            CFString::new(&path.to_string_lossy()),
            kCFURLPOSIXPathStyle,
            false,
        );
        // SAFETY: 枚举失败时 CoreText 返回 NULL，成功时返回带 +1 引用计数
        // 的 CFArrayRef；NULL 分支提前报错，非 NULL 交给 CFArray 接管释放。
        let raw = unsafe { CTFontManagerCreateFontDescriptorsFromURL(url.as_concrete_TypeRef()) };
        if raw.is_null() {
            return Err(format!(
                "coretext enumeration failed for {}",
                path.display()
            ));
        }
        // SAFETY: raw 是上一步刚创建的 +1 数组引用，此后无人再持它。
        Ok(unsafe { CFArray::<CTFontDescriptor>::wrap_under_create_rule(raw) })
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;

    const SAMPLE_FONT: &[u8] = b"sample font bytes";

    fn sample_font() -> &'static FamilyFont {
        static FONT: OnceLock<FamilyFont> = OnceLock::new();
        FONT.get_or_init(|| FamilyFont {
            bytes: SAMPLE_FONT.to_vec(),
            index: 0,
        })
    }

    fn registered_bytes(definitions: &FontDefinitions, name: &str) -> &'static [u8] {
        match &definitions.font_data[name].font {
            Cow::Borrowed(bytes) => bytes,
            Cow::Owned(_) => panic!("font bytes registered as owned"),
        }
    }

    #[test]
    fn cjk_fallback_appends_after_builtin_fonts() {
        let mut definitions = FontDefinitions::default();
        insert_face(sample_font(), &mut definitions, FONT_NAME);
        let builtin_len = definitions.families[&FontFamily::Proportional].len();
        append_fallback(&mut definitions);

        let proportional = &definitions.families[&FontFamily::Proportional];
        let monospace = &definitions.families[&FontFamily::Monospace];
        assert_eq!(proportional.len(), builtin_len + 1);
        assert_eq!(proportional.last(), Some(&FONT_NAME.to_owned()));
        assert_eq!(monospace.last(), Some(&FONT_NAME.to_owned()));
        assert!(definitions.font_data.contains_key(FONT_NAME));
    }

    #[test]
    fn named_family_without_bytes_binds_the_builtin_glyphs() {
        let mut definitions = FontDefinitions::default();
        register_named_or_builtin(&mut definitions, FONT_SERIF_NAME, None);

        assert!(
            !definitions.font_data.contains_key(FONT_SERIF_NAME),
            "no bytes must not invent font data"
        );
        assert_eq!(
            definitions.families[&FontFamily::Name(FONT_SERIF_NAME.into())],
            vec![BUILTIN_FALLBACK_FONT.to_owned()],
            "the family must still be bound, or epaint panics on first use"
        );
    }

    #[test]
    fn cjk_fallback_shares_bytes_across_contexts() {
        let mut first = FontDefinitions::default();
        let mut second = FontDefinitions::default();
        insert_face(sample_font(), &mut first, FONT_NAME);
        insert_face(sample_font(), &mut second, FONT_NAME);
        append_fallback(&mut first);
        append_fallback(&mut second);

        assert!(std::ptr::eq(
            registered_bytes(&first, FONT_NAME),
            sample_font().bytes.as_slice()
        ));
        assert!(std::ptr::eq(
            registered_bytes(&first, FONT_NAME),
            registered_bytes(&second, FONT_NAME)
        ));
    }

    #[test]
    fn named_family_registers_bytes_verbatim() {
        let mut definitions = FontDefinitions::default();
        register_named_or_builtin(&mut definitions, FONT_SERIF_NAME, Some(sample_font()));

        assert_eq!(
            definitions.families[&FontFamily::Name(FONT_SERIF_NAME.into())],
            vec![FONT_SERIF_NAME.to_owned()]
        );
        assert_eq!(registered_bytes(&definitions, FONT_SERIF_NAME), SAMPLE_FONT);
    }

    #[test]
    fn definitions_always_bind_the_named_typography_families() {
        let (definitions, _cjk_fallback) = definitions();

        for name in [FONT_SERIF_NAME, FONT_KAITI_NAME, FONT_MONO_NAME] {
            assert!(
                definitions.font_data.contains_key(name)
                    || definitions.families[&FontFamily::Name(name.into())]
                        == vec![BUILTIN_FALLBACK_FONT.to_owned()],
                "epaint panics on unbound families: {name} must always be bound"
            );
        }
    }

    #[test]
    fn zhu_family_is_always_named() {
        assert_eq!(zhu_family(), FontFamily::Name(FONT_KAITI_NAME.into()));
    }

    #[test]
    fn mono_family_is_always_named() {
        assert_eq!(mono_family(), FontFamily::Name(FONT_MONO_NAME.into()));
    }

    #[test]
    fn mono_family_chain_appends_the_cjk_fallback() {
        let mut definitions = FontDefinitions::default();
        insert_face(sample_font(), &mut definitions, FONT_NAME);
        append_fallback(&mut definitions);
        register_mono_family(&mut definitions, Some(sample_font()), Some(sample_font()));

        assert_eq!(
            definitions.families[&FontFamily::Name(FONT_MONO_NAME.into())],
            vec![FONT_MONO_NAME.to_owned(), FONT_NAME.to_owned()],
            "CJK glyphs must resolve after the system mono font"
        );
        assert_eq!(registered_bytes(&definitions, FONT_MONO_NAME), SAMPLE_FONT);
    }

    #[test]
    fn mono_family_without_a_system_mono_binds_the_builtin_glyphs() {
        let mut definitions = FontDefinitions::default();
        insert_face(sample_font(), &mut definitions, FONT_NAME);
        register_mono_family(&mut definitions, None, Some(sample_font()));

        assert!(
            !definitions.font_data.contains_key(FONT_MONO_NAME),
            "no system mono must not invent font data"
        );
        assert_eq!(
            definitions.families[&FontFamily::Name(FONT_MONO_NAME.into())],
            vec![BUILTIN_FALLBACK_FONT.to_owned()],
            "the family must still be bound, and must not degrade to the proportional CJK font"
        );
    }

    #[test]
    fn system_cjk_font_is_discoverable() {
        assert!(cjk_font().is_some());
    }

    #[test]
    fn system_monospace_font_is_discoverable() {
        assert!(mono_font().is_some());
    }

    #[test]
    fn system_cjk_font_is_loaded_once() {
        let first = cjk_font().expect("host CJK font");
        let second = cjk_font().expect("host CJK font");
        assert!(!first.bytes.is_empty());
        assert!(std::ptr::eq(first, second));

        let mut one = FontDefinitions::default();
        let mut two = FontDefinitions::default();
        insert_face(first, &mut one, FONT_NAME);
        insert_face(second, &mut two, FONT_NAME);
        assert!(std::ptr::eq(
            registered_bytes(&one, FONT_NAME),
            registered_bytes(&two, FONT_NAME)
        ));
    }

    #[test]
    fn single_face_files_register_face_zero() {
        let font = mono_font().expect("host monospace font");
        if !imp::is_collection(&font.bytes) {
            assert_eq!(font.index, 0);
        }
    }

    #[test]
    fn face_postscript_name_parses_a_synthetic_collection() {
        let ttc = synthetic_ttc("TestFont");
        assert!(imp::is_collection(&ttc));
        let face = imp::face_offset(&ttc, 0).expect("face offset");
        assert_eq!(
            face, 16,
            "one face table directory follows the 16-byte ttc header"
        );
        assert_eq!(
            imp::face_postscript_name(&ttc, face).as_deref(),
            Some("TestFont")
        );
    }

    #[test]
    fn face_postscript_name_rejects_a_truncated_collection() {
        let ttc = synthetic_ttc("TestFont");
        assert_eq!(imp::face_postscript_name(&ttc[..8], 8), None);
        assert!(imp::face_offset(&ttc[..8], 0).is_err());
    }

    #[test]
    fn face_postscript_name_rejects_an_out_of_range_table_offset() {
        let mut ttc = synthetic_ttc("TestFont");
        let field = 16 + 12 + 8;
        ttc[field..field + 4].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(imp::face_postscript_name(&ttc, 16), None);
    }

    fn synthetic_ttc(postscript: &str) -> Vec<u8> {
        const FACE_DIR: u32 = 16;
        const NAME_TABLE: u32 = FACE_DIR + 12 + 16;
        let mut ttc = Vec::new();
        ttc.extend_from_slice(b"ttcf");
        ttc.extend_from_slice(&1u32.to_be_bytes());
        ttc.extend_from_slice(&1u32.to_be_bytes());
        ttc.extend_from_slice(&FACE_DIR.to_be_bytes());

        ttc.extend_from_slice(&[0, 1, 0, 0]);
        ttc.extend_from_slice(&1u16.to_be_bytes());
        ttc.extend_from_slice(&[0; 6]);
        ttc.extend_from_slice(b"name");
        ttc.extend_from_slice(&0u32.to_be_bytes());
        ttc.extend_from_slice(&NAME_TABLE.to_be_bytes());
        ttc.extend_from_slice(&34u32.to_be_bytes());

        ttc.extend_from_slice(&0u16.to_be_bytes());
        ttc.extend_from_slice(&1u16.to_be_bytes());
        ttc.extend_from_slice(&18u16.to_be_bytes());
        ttc.extend_from_slice(&3u16.to_be_bytes());
        ttc.extend_from_slice(&1u16.to_be_bytes());
        ttc.extend_from_slice(&0x409u16.to_be_bytes());
        ttc.extend_from_slice(&6u16.to_be_bytes());
        ttc.extend_from_slice(&(postscript.len() as u16 * 2).to_be_bytes());
        ttc.extend_from_slice(&0u16.to_be_bytes());
        for unit in postscript.encode_utf16() {
            ttc.extend_from_slice(&unit.to_be_bytes());
        }
        ttc
    }

    #[test]
    fn builtin_definitions_binds_named_families_without_system_fonts() {
        let definitions = builtin_definitions();
        for name in [FONT_SERIF_NAME, FONT_KAITI_NAME, FONT_MONO_NAME] {
            let bound = definitions
                .families
                .get(&FontFamily::Name(name.into()))
                .expect("named family must stay bound");
            assert_eq!(bound, &[BUILTIN_FALLBACK_FONT.to_owned()]);
        }
        assert!(!definitions.font_data.contains_key(FONT_NAME));
    }
}
