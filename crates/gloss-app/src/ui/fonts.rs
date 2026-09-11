//! 系统字体发现与注入：给 egui 补上 CJK 后备字形（M1-T5）。
//!
//! egui 内置字体只覆盖拉丁与常见符号，中文会渲染成豆腐块；这里用 font-kit 从
//! 系统里定位一个 CJK 字体，以「最低优先级后备」追加进 egui 的字体族——拉丁
//! 字形仍走内置字体，缺的字形才落到系统字体上。查找失败只降级告警，不阻塞
//! 启动：浮层照常工作，中文暂时不可读，后续版本可考虑内嵌开源字体兜底。

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};
use gloss_core::log::{info, thread, warn};

/// CJK 字体在 egui 字体表里登记的名字。
const FONT_NAME: &str = "gloss-cjk";

/// 把系统中文字体接进 egui 的后备链。失败只记日志，返回 `false` 表示未接入。
pub fn install(ctx: &egui::Context) {
    let mut definitions = FontDefinitions::default();
    if apply(&mut definitions) {
        ctx.set_fonts(definitions);
    }
}

/// 定位系统 CJK 字体并写进字体定义；返回是否成功。
fn apply(definitions: &mut FontDefinitions) -> bool {
    match imp::find_cjk() {
        Some(data) => {
            append_fallback(definitions, data);
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
/// 内置字体先挑，挑不到的字形才轮到系统字体。
fn append_fallback(definitions: &mut FontDefinitions, data: FontData) {
    definitions
        .font_data
        .insert(FONT_NAME.to_owned(), Arc::new(data));
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        definitions
            .families
            .entry(family)
            .or_default()
            .push(FONT_NAME.to_owned());
    }
}

/// macOS：苹方随系统自带，历代候选按可用性排序。
#[cfg(target_os = "macos")]
const CJK_FAMILIES: &[&str] = &["PingFang SC", "Hiragino Sans GB", "STHeiti"];

/// Windows：微软雅黑 Vista 起全 SKU 内置，黑体/宋体兜底。
#[cfg(target_os = "windows")]
const CJK_FAMILIES: &[&str] = &["Microsoft YaHei", "微软雅黑", "SimHei", "SimSun"];

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod imp {
    use super::{CJK_FAMILIES, FONT_NAME};
    use egui::FontData;
    use font_kit::source::SystemSource;
    use gloss_core::log::{debug, info, thread, warn};

    /// 依候选顺序查系统 CJK 字体，命中第一个就返回。
    ///
    /// font-kit 的 CoreText/DirectWrite 后端在 `load()` 时会把 .ttc 集合拆成
    /// 单个字体面，`copy_font_data()` 给出的就是拆好的数据，因此 egui 侧的
    /// face index 恒为 0。
    pub(super) fn find_cjk() -> Option<FontData> {
        let source = SystemSource::new();
        for family in CJK_FAMILIES {
            let family_handle = match source.select_family_by_name(family) {
                Ok(handle) if !handle.is_empty() => handle,
                _ => {
                    debug!(thread = thread::UI, family, "CJK font family not found");
                    continue;
                }
            };
            let font = match family_handle.fonts()[0].clone().load() {
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
                Some(bytes) => {
                    info!(
                        thread = thread::UI,
                        family,
                        bytes = bytes.len(),
                        "located system CJK font"
                    );
                    return Some(FontData {
                        font: bytes.to_vec().into(),
                        index: 0,
                        tweak: Default::default(),
                    });
                }
                None => warn!(
                    thread = thread::UI,
                    family, "CJK font data unavailable from system loader"
                ),
            }
        }
        warn!(
            thread = thread::UI,
            font = FONT_NAME,
            "exhausted CJK font candidates"
        );
        None
    }
}

/// Linux 仅用于 CI：不编译 font-kit，恒返回「未找到」。
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod imp {
    use egui::FontData;

    pub(super) fn find_cjk() -> Option<FontData> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::FontTweak;

    /// 后备字体必须排在内置字体之后（拉丁度量不受影响），且两个字体族都接上。
    #[test]
    fn cjk_fallback_appends_after_builtin_fonts() {
        let mut definitions = FontDefinitions::default();
        let builtin_len = definitions.families[&FontFamily::Proportional].len();
        append_fallback(
            &mut definitions,
            FontData {
                font: Vec::new().into(),
                index: 0,
                tweak: FontTweak::default(),
            },
        );

        let proportional = &definitions.families[&FontFamily::Proportional];
        let monospace = &definitions.families[&FontFamily::Monospace];
        assert_eq!(proportional.len(), builtin_len + 1);
        assert_eq!(proportional.last(), Some(&FONT_NAME.to_owned()));
        assert_eq!(monospace.last(), Some(&FONT_NAME.to_owned()));
        assert!(definitions.font_data.contains_key(FONT_NAME));
    }

    /// 真实系统上必须能找到 CJK 字体（CI 的 macos/windows runner 自带系统字体）。
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn system_cjk_font_is_discoverable() {
        assert!(imp::find_cjk().is_some());
    }

    /// 非 macOS/Windows 平台（CI 的 Linux runner）找不到是预期行为。
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    #[test]
    fn no_cjk_font_on_ci_linux_means_no_install() {
        let mut definitions = FontDefinitions::default();
        assert!(!apply(&mut definitions));
        assert!(!definitions.font_data.contains_key(FONT_NAME));
    }
}
