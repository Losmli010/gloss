//! 系统字体发现与注入：给 egui 补上 CJK 后备字形与经注疏的宋楷字族。
//!
//! egui 内置字体只覆盖拉丁与常见符号，中文会渲染成豆腐块；这里用 font-kit 从
//! 系统里定位一个 CJK 字体，以「最低优先级后备」追加进 egui 的字体族——拉丁
//! 字形仍走内置字体，缺的字形才落到系统字体上。经注疏的宋楷两族（
//! [`FONT_SERIF_NAME`] / [`FONT_KAITI_NAME`]）按家族名注册成**命名字体族**，
//! 不进后备链：只有显式点名的文字（经注疏分区正文）才用它，其余文本的
//! 字形解析完全不受影响。楷体在系统里缺席时按 demo 自己的回退链落到宋体
//! （docs/demo/popup-redesign.html 的 --kai 栈以 Songti SC 收尾）。
//! 字体字节进程内只向系统取一份，浮层与设置窗两个 egui 上下文引用同一份。
//! 查找失败只降级告警，不阻塞启动：CJK 后备缺席时比例/等宽族不接系统字形
//! （中文暂时不可读），宋楷命名字体族仍恒注册、兜底到内置字形——epaint 对
//! 未绑定的字体族直接 panic，族必须永远存在。后续版本可考虑内嵌开源字体
//! 兜底。

use std::sync::{Arc, OnceLock};

use egui::{FontData, FontDefinitions, FontFamily};
/// CJK 字体在 egui 字体表里登记的名字。
pub(super) const FONT_NAME: &str = "gloss-cjk";
/// 宋体命名字体族：经/疏正文的排版字体（demo 的 --serif 栈）。
pub const FONT_SERIF_NAME: &str = "gloss-songti";
/// 楷体命名字体族：注正文的排版字体（demo 的 --kai 栈）。
pub const FONT_KAITI_NAME: &str = "gloss-kaiti";

/// 注正文的字体族。
pub fn zhu_family() -> FontFamily {
    FontFamily::Name(FONT_KAITI_NAME.into())
}

/// 经/疏正文的字体族。
pub fn serif_family() -> FontFamily {
    FontFamily::Name(FONT_SERIF_NAME.into())
}

/// 系统 CJK 字体的字节——进程内唯一一份。取用失败同样缓存，不重复查找系统。
static CJK_BYTES: OnceLock<Option<Vec<u8>>> = OnceLock::new();

/// 宋体/楷体的字节，各自进程内唯一一份（形态与 [`CJK_BYTES`] 相同）。
static SERIF_BYTES: OnceLock<Option<Vec<u8>>> = OnceLock::new();
static KAITI_BYTES: OnceLock<Option<Vec<u8>>> = OnceLock::new();

/// 苹方随系统自带，历代候选按可用性排序。
const CJK_FAMILIES: &[&str] = &["PingFang SC", "Hiragino Sans GB", "STHeiti"];

/// 宋体候选（demo --serif 栈的前四项，serif 泛型族不参与精确匹配）。
const SERIF_FAMILIES: &[&str] = &["Songti SC", "STSong", "Noto Serif SC", "SimSun"];

/// 楷体候选（demo --kai 栈的实名项）。
const KAITI_FAMILIES: &[&str] = &["Kaiti SC", "STKaiti", "Kaiti TC", "KaiTi"];

/// egui 内置字形的登记名（default_fonts 特性自带）：宋楷两族在系统与
/// CJK 后备双双缺席时的族内兜底，排版降级为默认字形而不是 panic。
const BUILTIN_FALLBACK_FONT: &str = "Ubuntu-Light";

/// 带 CJK 后备与宋楷字族的字体定义；bool 表示 CJK 后备是否接上（加载处
/// 已告警，比例/等宽族在未接上时中文不可读）。宋楷两族**无条件注册**——
/// epaint 对未绑定的字体族直接 panic——兜底链为 宋/楷系统字体 → 宋体
/// 字节 → CJK 后备字节 → 内置字形，排版逐级降级而不是崩。交给哪个上下文
/// 由 [`super::context`] 的统一装入点决定。
pub(in crate::ui) fn definitions() -> (FontDefinitions, bool) {
    let mut definitions = FontDefinitions::default();
    let cjk = cjk_bytes();
    if let Some(cjk) = cjk {
        append_fallback(&mut definitions, cjk);
    }

    let serif = SERIF_BYTES.get_or_init(|| imp::load_family_bytes(SERIF_FAMILIES));
    let kaiti = KAITI_BYTES
        .get_or_init(|| imp::load_family_bytes(KAITI_FAMILIES).or_else(|| serif.clone()));
    let serif_bytes = serif.as_deref().or(cjk);
    let kaiti_bytes = kaiti.as_deref().or(serif_bytes);
    register_named_or_builtin(&mut definitions, FONT_SERIF_NAME, serif_bytes);
    register_named_or_builtin(&mut definitions, FONT_KAITI_NAME, kaiti_bytes);
    (definitions, cjk.is_some())
}

/// 系统 CJK 字体字节；首次调用向系统取，之后命中缓存。
fn cjk_bytes() -> Option<&'static [u8]> {
    CJK_BYTES.get_or_init(imp::load_cjk_bytes).as_deref()
}

/// 把一个字体以后备（最低优先级）追加进比例与等宽两个字体族：
/// 内置字体先挑，挑不到的字形才轮到系统字体。字节以借用形态登记。
fn append_fallback(definitions: &mut FontDefinitions, bytes: &'static [u8]) {
    definitions
        .font_data
        .insert(FONT_NAME.to_owned(), Arc::new(FontData::from_static(bytes)));
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        definitions
            .families
            .entry(family)
            .or_default()
            .push(FONT_NAME.to_owned());
    }
}

/// 把一批字体字节注册成命名字体族；字节缺失时把族绑到内置字形上——
/// 族必须恒存在（epaint 对未绑定的族直接 panic），字形逐级降级。
fn register_named_or_builtin(
    definitions: &mut FontDefinitions,
    name: &'static str,
    bytes: Option<&'static [u8]>,
) {
    match bytes {
        Some(bytes) => {
            definitions
                .font_data
                .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
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

mod imp {
    use std::sync::Arc;

    use font_kit::family_name::FamilyName;
    use font_kit::properties::{Properties, Style, Weight};
    use font_kit::source::SystemSource;
    use gloss_core::log::{debug, info, thread, warn};

    use super::{CJK_FAMILIES, FONT_NAME};

    /// 依候选顺序查系统 CJK 字体，命中第一个就返回它的字节。
    ///
    /// 匹配走 CSS Fonts L3 的 `select_best_match`；CoreText 侧按家族名解析出面，
    /// 交出的数据已被 font-kit 就地解包成单面 sfnt，egui 侧 face index 恒为 0。
    pub(super) fn load_cjk_bytes() -> Option<Vec<u8>> {
        load_family_bytes(CJK_FAMILIES).inspect(|bytes| {
            info!(
                thread = thread::UI,
                font = FONT_NAME,
                bytes = bytes.len(),
                "located system CJK font"
            );
        })
    }

    /// 依候选顺序定位第一个可用的系统字体家族，返回它的字节；全部落空
    /// 只留 debug 痕（宋楷是排版偏好，缺席不是故障）。
    pub(super) fn load_family_bytes(families: &[&str]) -> Option<Vec<u8>> {
        let source = SystemSource::new();
        let mut properties = Properties::new();
        properties.weight(Weight::NORMAL).style(Style::Normal);
        for family in families {
            let handle = match source
                .select_best_match(&[FamilyName::Title((*family).to_owned())], &properties)
            {
                Ok(handle) => handle,
                Err(err) => {
                    debug!(thread = thread::UI, family, error = %err, "font family not matched");
                    continue;
                }
            };
            let bytes = {
                let font = match handle.load() {
                    Ok(font) => font,
                    Err(err) => {
                        warn!(
                            thread = thread::UI,
                            family,
                            error = %err,
                            "failed to load font family candidate"
                        );
                        continue;
                    }
                };
                match font.copy_font_data() {
                    Some(bytes) => bytes,
                    None => {
                        warn!(
                            thread = thread::UI,
                            family, "font data unavailable from system loader"
                        );
                        continue;
                    }
                }
            };
            // 走到这里 font-kit 的字体已析构，Arc 只剩这一个持有者：
            // try_unwrap 直接取走 Vec，不复制整包
            let len = bytes.len();
            let vec = Arc::try_unwrap(bytes).unwrap_or_else(|arc| {
                debug!(thread = thread::UI, bytes = len, "font bytes copied out");
                (*arc).clone()
            });
            return Some(vec);
        }
        debug!(
            thread = thread::UI,
            ?families,
            "exhausted font family candidates"
        );
        None
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;

    const SAMPLE_FONT: &[u8] = b"sample font bytes";

    fn registered_bytes(definitions: &FontDefinitions, name: &str) -> &'static [u8] {
        match &definitions.font_data[name].font {
            Cow::Borrowed(bytes) => bytes,
            Cow::Owned(_) => panic!("font bytes registered as owned"),
        }
    }

    #[test]
    fn cjk_fallback_appends_after_builtin_fonts() {
        let mut definitions = FontDefinitions::default();
        let builtin_len = definitions.families[&FontFamily::Proportional].len();
        append_fallback(&mut definitions, SAMPLE_FONT);

        let proportional = &definitions.families[&FontFamily::Proportional];
        let monospace = &definitions.families[&FontFamily::Monospace];
        assert_eq!(proportional.len(), builtin_len + 1);
        assert_eq!(proportional.last(), Some(&FONT_NAME.to_owned()));
        assert_eq!(monospace.last(), Some(&FONT_NAME.to_owned()));
        assert!(definitions.font_data.contains_key(FONT_NAME));
    }

    #[test]
    fn cjk_fallback_without_system_font_installs_nothing() {
        let mut definitions = FontDefinitions::default();
        let proportional_len = definitions.families[&FontFamily::Proportional].len();
        let monospace_len = definitions.families[&FontFamily::Monospace].len();

        append_fallback(&mut definitions, SAMPLE_FONT);
        assert!(definitions.font_data.contains_key(FONT_NAME));
        assert_eq!(
            definitions.families[&FontFamily::Proportional].len(),
            proportional_len + 1
        );
        assert_eq!(
            definitions.families[&FontFamily::Monospace].len(),
            monospace_len + 1
        );
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
        append_fallback(&mut first, SAMPLE_FONT);
        append_fallback(&mut second, SAMPLE_FONT);

        assert!(std::ptr::eq(
            registered_bytes(&first, FONT_NAME),
            SAMPLE_FONT
        ));
        assert!(std::ptr::eq(
            registered_bytes(&first, FONT_NAME),
            registered_bytes(&second, FONT_NAME)
        ));
    }

    #[test]
    fn named_family_registers_bytes_verbatim() {
        let mut definitions = FontDefinitions::default();
        register_named_or_builtin(&mut definitions, FONT_SERIF_NAME, Some(SAMPLE_FONT));

        assert_eq!(
            definitions.families[&FontFamily::Name(FONT_SERIF_NAME.into())],
            vec![FONT_SERIF_NAME.to_owned()]
        );
        assert_eq!(registered_bytes(&definitions, FONT_SERIF_NAME), SAMPLE_FONT);
    }

    #[test]
    fn definitions_always_bind_the_named_typography_families() {
        let (definitions, _cjk_fallback) = definitions();

        for name in [FONT_SERIF_NAME, FONT_KAITI_NAME] {
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
    fn system_cjk_font_is_discoverable() {
        assert!(cjk_bytes().is_some());
    }

    #[test]
    fn system_cjk_font_is_loaded_once() {
        let first = cjk_bytes().expect("host CJK font");
        let second = cjk_bytes().expect("host CJK font");
        assert!(!first.is_empty());
        assert!(std::ptr::eq(first, second));

        let mut one = FontDefinitions::default();
        let mut two = FontDefinitions::default();
        append_fallback(&mut one, first);
        append_fallback(&mut two, second);
        assert!(std::ptr::eq(
            registered_bytes(&one, FONT_NAME),
            registered_bytes(&two, FONT_NAME)
        ));
    }
}
