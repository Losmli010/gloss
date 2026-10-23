//! 系统字体发现与注入：给 egui 补上 CJK 后备字形、经注疏的宋楷字族与
//! 音标的等宽字族。
//!
//! egui 内置字体只覆盖拉丁与常见符号，中文会渲染成豆腐块；这里用 CoreText
//! 从系统里定位一个 CJK 字体，以「最低优先级后备」追加进 egui 的字体族——
//! 拉丁字形仍走内置字体，缺的字形才落到系统字体上。经注疏的宋楷两族（
//! [`FONT_SERIF_NAME`] / [`FONT_KAITI_NAME`]）与等宽族（[`FONT_MONO_NAME`]）
//! 按**命名字体族**注册，不进后备链：只有显式点名的文字（经注疏分区正文、
//! 词卡音标）才用它们，其余文本的字形解析完全不受影响。楷体在系统里缺席时
//! 按设计定稿的回退链落到宋体（--kai 栈
//! 以 `Songti SC` 收尾）；等宽族按 demo 的 ui-monospace 栈依序探测系统等宽，
//! 族绑定是「系统等宽 + CJK 后备字节」（代码内中文注释仍可读），候选全缺席
//! 时兜底到内置字形。字体字节进程内只向系统读一次文件（mmap 文件背页，
//! 映射失败回退整包拷贝），浮层与设置窗两个
//! egui 上下文引用同一份；ttc 集合整体登记（egui 按序号取面），不做单面
//! 解包。查找失败只降级不阻塞启动：CJK 后备缺席出 warn，比例/等宽族不接
//! 系统字形（中文不可读）；命名字体族仍恒注册、兜底到内置字形——
//! epaint 对未绑定的字体族直接 panic，族必须永远存在。

use std::borrow::Cow;
use std::ops::Deref;
use std::sync::{Arc, OnceLock};

use egui::{FontData, FontDefinitions, FontFamily, FontTweak};
use gloss_core::log::{thread, warn};
use memmap2::Mmap;

pub(super) const FONT_NAME: &str = "gloss-cjk";
pub(super) const FONT_SERIF_NAME: &str = "gloss-songti";
pub(super) const FONT_KAITI_NAME: &str = "gloss-kaiti";
pub(super) const FONT_MONO_NAME: &str = "gloss-mono";

pub(super) fn zhu_family() -> FontFamily {
    FontFamily::Name(FONT_KAITI_NAME.into())
}

pub(super) fn serif_family() -> FontFamily {
    FontFamily::Name(FONT_SERIF_NAME.into())
}

pub(super) fn mono_family() -> FontFamily {
    FontFamily::Name(FONT_MONO_NAME.into())
}

enum FontBacking {
    Mapped(Mmap),
    Copied(Vec<u8>),
}

impl FontBacking {
    fn as_slice(&self) -> &[u8] {
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

struct FamilyFont {
    bytes: FontBacking,
    index: u32,
}

impl FamilyFont {
    #[cfg(test)]
    fn from_bytes(bytes: Vec<u8>, index: u32) -> FamilyFont {
        FamilyFont {
            bytes: FontBacking::Copied(bytes),
            index,
        }
    }
}

fn insert_face(font: &'static FamilyFont, definitions: &mut FontDefinitions, name: &str) {
    definitions.font_data.insert(
        name.to_owned(),
        Arc::new(FontData {
            font: Cow::Borrowed(font.bytes.as_slice()),
            index: font.index,
            tweak: FontTweak::default(),
        }),
    );
}

static CJK_FONT: OnceLock<Option<FamilyFont>> = OnceLock::new();

static SERIF_FONT: OnceLock<Option<FamilyFont>> = OnceLock::new();
static KAITI_FONT: OnceLock<Option<FamilyFont>> = OnceLock::new();
static MONO_FONT: OnceLock<Option<FamilyFont>> = OnceLock::new();

const CJK_FAMILIES: &[&str] = &["PingFang SC", "Hiragino Sans GB", "STHeiti"];

const SERIF_FAMILIES: &[&str] = &["Songti SC", "STSong", "Noto Serif SC", "SimSun"];

const KAITI_FAMILIES: &[&str] = &["Kaiti SC", "STKaiti", "Kaiti TC", "KaiTi"];

const MONO_FAMILIES: &[&str] = &["SF Mono", "Menlo", "Monaco", "DejaVu Sans Mono", "Consolas"];

pub(super) const BUILTIN_FALLBACK_FONT: &str = "Ubuntu-Light";

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

pub(in crate::ui) fn system_fonts_ready() -> bool {
    CJK_FONT.get().is_some()
        && SERIF_FONT.get().is_some()
        && KAITI_FONT.get().is_some()
        && MONO_FONT.get().is_some()
}

fn cjk_font() -> Option<&'static FamilyFont> {
    CJK_FONT.get_or_init(imp::load_cjk).as_ref()
}

fn mono_font() -> Option<&'static FamilyFont> {
    MONO_FONT
        .get_or_init(|| imp::load_family(MONO_FAMILIES))
        .as_ref()
}

fn append_fallback(definitions: &mut FontDefinitions) {
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        definitions
            .families
            .entry(family)
            .or_default()
            .push(FONT_NAME.to_owned());
    }
}

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

mod imp;

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;

    const SAMPLE_FONT: &[u8] = b"sample font bytes";

    fn sample_font() -> &'static FamilyFont {
        static FONT: OnceLock<FamilyFont> = OnceLock::new();
        FONT.get_or_init(|| FamilyFont::from_bytes(SAMPLE_FONT.to_vec(), 0))
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
