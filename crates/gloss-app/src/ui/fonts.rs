//! 系统字体发现与注入：给 egui 补上 CJK 后备字形。
//!
//! egui 内置字体只覆盖拉丁与常见符号，中文会渲染成豆腐块；这里用 font-kit 从
//! 系统里定位一个 CJK 字体，以「最低优先级后备」追加进 egui 的字体族——拉丁
//! 字形仍走内置字体，缺的字形才落到系统字体上。字体字节进程内只向系统取一份，
//! 浮层与设置窗两个 egui 上下文引用同一份。查找失败只降级告警，不阻塞启动：
//! 浮层照常工作，中文暂时不可读，后续版本可考虑内嵌开源字体兜底。

use std::sync::{Arc, OnceLock};

use egui::{FontData, FontDefinitions, FontFamily};
use gloss_core::log::{info, thread, warn};

/// CJK 字体在 egui 字体表里登记的名字。
const FONT_NAME: &str = "gloss-cjk";

/// 系统 CJK 字体的字节——进程内唯一一份。取用失败同样缓存，不重复查找系统。
static CJK_BYTES: OnceLock<Option<Vec<u8>>> = OnceLock::new();

/// 把系统中文字体接进 egui 的后备链。失败只记日志，返回 `false` 表示未接入。
pub fn install(ctx: &egui::Context) {
    let mut definitions = FontDefinitions::default();
    if apply(&mut definitions, cjk_bytes()) {
        ctx.set_fonts(definitions);
    }
}

/// 系统 CJK 字体字节；首次调用向系统取，之后命中缓存。
fn cjk_bytes() -> Option<&'static [u8]> {
    CJK_BYTES.get_or_init(imp::load_cjk_bytes).as_deref()
}

/// 把字体字节写进字体定义；返回是否接入。
fn apply(definitions: &mut FontDefinitions, bytes: Option<&'static [u8]>) -> bool {
    match bytes {
        Some(bytes) => {
            append_fallback(definitions, bytes);
            info!(thread = thread::UI, "CJK fallback font installed into egui");
            true
        }
        None => {
            warn!(
                thread = thread::UI,
                "no system CJK font found; Chinese text will render as boxes"
            );
            false
        }
    }
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

/// 苹方随系统自带，历代候选按可用性排序。
const CJK_FAMILIES: &[&str] = &["PingFang SC", "Hiragino Sans GB", "STHeiti"];

mod imp {
    use std::sync::Arc;

    use super::{CJK_FAMILIES, FONT_NAME};
    use font_kit::family_name::FamilyName;
    use font_kit::properties::{Properties, Style, Weight};
    use font_kit::source::SystemSource;
    use gloss_core::log::{debug, info, thread, warn};

    /// 依候选顺序查系统 CJK 字体，命中第一个就返回它的字节。
    ///
    /// 匹配走 CSS Fonts L3 的 `select_best_match`；CoreText 侧按家族名解析出面，
    /// 交出的数据已被 font-kit 就地解包成单面 sfnt，egui 侧 face index 恒为 0。
    pub(super) fn load_cjk_bytes() -> Option<Vec<u8>> {
        let source = SystemSource::new();
        let mut properties = Properties::new();
        properties.weight(Weight::NORMAL).style(Style::Normal);
        for family in CJK_FAMILIES {
            let handle = match source
                .select_best_match(&[FamilyName::Title((*family).to_owned())], &properties)
            {
                Ok(handle) => handle,
                Err(err) => {
                    debug!(thread = thread::UI, family, error = %err, "CJK font candidate not matched");
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
                            "failed to load CJK font candidate"
                        );
                        continue;
                    }
                };
                match font.copy_font_data() {
                    Some(bytes) => bytes,
                    None => {
                        warn!(
                            thread = thread::UI,
                            family, "CJK font data unavailable from system loader"
                        );
                        continue;
                    }
                }
            };
            // 走到这里 font-kit 的字体已析构，Arc 只剩这一个持有者：
            // try_unwrap 直接取走 Vec，不复制整包
            let len = bytes.len();
            let vec = Arc::try_unwrap(bytes).unwrap_or_else(|arc| {
                debug!(
                    thread = thread::UI,
                    bytes = len,
                    "CJK font bytes copied out"
                );
                (*arc).clone()
            });
            info!(
                thread = thread::UI,
                family,
                bytes = len,
                "located system CJK font"
            );
            return Some(vec);
        }
        warn!(
            thread = thread::UI,
            font = FONT_NAME,
            "exhausted CJK font candidates"
        );
        None
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;

    const SAMPLE_FONT: &[u8] = b"sample font bytes";

    fn registered_bytes(definitions: &FontDefinitions) -> &'static [u8] {
        match &definitions.font_data[FONT_NAME].font {
            Cow::Borrowed(bytes) => bytes,
            Cow::Owned(_) => panic!("font bytes registered as owned"),
        }
    }

    #[test]
    fn cjk_fallback_appends_after_builtin_fonts() {
        let mut definitions = FontDefinitions::default();
        let builtin_len = definitions.families[&FontFamily::Proportional].len();
        assert!(apply(&mut definitions, Some(SAMPLE_FONT)));

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

        assert!(!apply(&mut definitions, None));
        assert!(!definitions.font_data.contains_key(FONT_NAME));
        assert_eq!(
            definitions.families[&FontFamily::Proportional].len(),
            proportional_len
        );
        assert_eq!(
            definitions.families[&FontFamily::Monospace].len(),
            monospace_len
        );
    }

    #[test]
    fn cjk_fallback_shares_bytes_across_contexts() {
        let mut first = FontDefinitions::default();
        let mut second = FontDefinitions::default();
        assert!(apply(&mut first, Some(SAMPLE_FONT)));
        assert!(apply(&mut second, Some(SAMPLE_FONT)));

        assert!(std::ptr::eq(registered_bytes(&first), SAMPLE_FONT));
        assert!(std::ptr::eq(
            registered_bytes(&first),
            registered_bytes(&second)
        ));
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
        assert!(apply(&mut one, Some(first)));
        assert!(apply(&mut two, Some(second)));
        assert!(std::ptr::eq(registered_bytes(&one), registered_bytes(&two)));
    }
}
