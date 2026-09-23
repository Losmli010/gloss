//! 界面文案表：locale → 类型化文案，编译期嵌入的文件资源。
//!
//! 与 core 的 prompt 模板同构（文件资源 + `include_str!`），分工不同：
//! prompt 面向模型（长文 markdown），这里面向用户（短标签键值表）。
//!
//! 键完整性由类型与 serde 共同保证：字段与两份文件一一对应，
//! `deny_unknown_fields` 拦多键、字段无缺省拦少键——「拼错键静默回退」在
//! 类型层不存在。解析在首次取用时做一次并常驻，失败回落空表并记 error：
//! 文件是编译期常量，解析测试保证那条分支不可达。
//!
//! 语言由调用方给（`Config::language` 落定成的 `Locale`），本模块不做探测。

use std::sync::OnceLock;

use gloss_core::log::error;
use gloss_core::model::{GlossError, Locale};
use serde::Deserialize;

/// 中文文案（出厂 locale）。
const ZH: &str = include_str!("../i18n/zh.toml");
/// 英文文案。
const EN: &str = include_str!("../i18n/en.toml");

/// 一个 locale 的全部界面文案：一张扁平表，字段名就是 TOML 里的键。
///
/// 键一律是全限定名 `gloss_<模块>_<词条>`（下划线连接，见 `i18n/*.toml`）；
/// 字段与文件逐字同名，因此既没有 rename 层也没有嵌套结构体——文件与代码读到的
/// 是同一个名字。改词条名要连着改 TOML 里的那一行，名字对不上即解析失败。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Text {
    pub(crate) gloss_app_settings_title: String,

    pub(crate) gloss_kinds_translate_word: String,
    pub(crate) gloss_kinds_translate_sentence: String,
    pub(crate) gloss_kinds_explain_code: String,
    pub(crate) gloss_kinds_image_ocr: String,
    pub(crate) gloss_kinds_image_explain: String,

    pub(crate) gloss_langs_zh: String,
    pub(crate) gloss_langs_en: String,
    pub(crate) gloss_langs_ja: String,
    pub(crate) gloss_langs_ko: String,
    pub(crate) gloss_langs_fr: String,
    pub(crate) gloss_langs_other: String,

    pub(crate) gloss_popup_brand: String,
    pub(crate) gloss_popup_selfcheck: String,
    pub(crate) gloss_popup_failed: String,
    pub(crate) gloss_popup_retry: String,
    pub(crate) gloss_popup_open_settings: String,
    pub(crate) gloss_popup_close_label: String,
    pub(crate) gloss_popup_settings_label: String,

    pub(crate) gloss_errors_selection_unavailable: String,
    pub(crate) gloss_errors_accessibility_denied: String,
    pub(crate) gloss_errors_screen_capture_denied: String,
    pub(crate) gloss_errors_region_too_large: String,
    pub(crate) gloss_errors_unsupported_modality: String,
    pub(crate) gloss_errors_engine_network: String,
    pub(crate) gloss_errors_engine_auth: String,
    pub(crate) gloss_errors_engine_rate_limited: String,
    pub(crate) gloss_errors_engine_response: String,
    pub(crate) gloss_errors_config: String,
    pub(crate) gloss_errors_acquire_channel: String,
    pub(crate) gloss_errors_inference_channel: String,

    pub(crate) gloss_settings_section_model: String,
    pub(crate) gloss_settings_section_task: String,
    pub(crate) gloss_settings_section_hotkey: String,
    pub(crate) gloss_settings_section_general: String,

    pub(crate) gloss_settings_save: String,
    pub(crate) gloss_settings_cancel: String,
    pub(crate) gloss_settings_invalid_summary: String,
    pub(crate) gloss_settings_base_url_hint: String,
    pub(crate) gloss_settings_key_keep_hint: String,
    pub(crate) gloss_settings_key_clear_hint: String,
    pub(crate) gloss_settings_clear_key: String,
    pub(crate) gloss_settings_undo_clear_key: String,
    pub(crate) gloss_settings_default_kind: String,
    pub(crate) gloss_settings_default_kind_hint: String,
    pub(crate) gloss_settings_target_lang: String,
    pub(crate) gloss_settings_kind_switch: String,
    pub(crate) gloss_settings_kind_switch_hint: String,
    pub(crate) gloss_settings_default_model: String,
    pub(crate) gloss_settings_default_model_hint: String,
    pub(crate) gloss_settings_vision_model_hint: String,
    pub(crate) gloss_settings_switch_label: String,
    pub(crate) gloss_settings_hotkey_hint: String,
    pub(crate) gloss_settings_ui_language: String,
    pub(crate) gloss_settings_ui_theme: String,
    pub(crate) gloss_settings_cache_ttl: String,
    pub(crate) gloss_settings_cache_ttl_suffix: String,
    pub(crate) gloss_settings_cache_ttl_hint: String,
    pub(crate) gloss_settings_notice_key_update_failed: String,
    pub(crate) gloss_settings_notice_save_failed: String,
    pub(crate) gloss_settings_notice_key_updated_save_failed: String,
    pub(crate) gloss_settings_error_default_kind_disabled: String,
    pub(crate) gloss_settings_error_duplicate_hotkey: String,
    pub(crate) gloss_settings_error_empty_trigger: String,
    pub(crate) gloss_settings_error_invalid_trigger: String,
    pub(crate) gloss_settings_error_newline_in_model: String,
    pub(crate) gloss_settings_error_base_url_empty: String,
    pub(crate) gloss_settings_error_base_url_invalid: String,
    pub(crate) gloss_settings_error_base_url_credentials: String,
    pub(crate) gloss_settings_error_base_url_query: String,

    pub(crate) gloss_ui_language_system: String,
    pub(crate) gloss_ui_language_zh: String,
    pub(crate) gloss_ui_language_en: String,

    pub(crate) gloss_theme_system: String,
    pub(crate) gloss_theme_light: String,
    pub(crate) gloss_theme_dark: String,
}

impl Text {
    /// 失败卡文案：按 [`GlossError`] 变体映射，不按英文 `Display` 反查
    /// ——后者是日志用的诊断文本，措辞与它无关（改 `Display` 不该改界面）。
    pub(crate) fn for_error(&self, error: &GlossError) -> String {
        match error {
            GlossError::SelectionUnavailable => self.gloss_errors_selection_unavailable.clone(),
            GlossError::AccessibilityDenied => self.gloss_errors_accessibility_denied.clone(),
            GlossError::ScreenCaptureDenied => self.gloss_errors_screen_capture_denied.clone(),
            GlossError::RegionTooLarge => self.gloss_errors_region_too_large.clone(),
            GlossError::UnsupportedModality => self.gloss_errors_unsupported_modality.clone(),
            GlossError::EngineNetwork => self.gloss_errors_engine_network.clone(),
            GlossError::EngineAuth => self.gloss_errors_engine_auth.clone(),
            GlossError::EngineRateLimited => self.gloss_errors_engine_rate_limited.clone(),
            GlossError::EngineResponse(detail) => {
                fill(&self.gloss_errors_engine_response, &[("detail", detail)])
            }
            GlossError::Config(detail) => fill(&self.gloss_errors_config, &[("detail", detail)]),
        }
    }

    /// 错因细节：变体自带诊断文本的取它，其余整句照搬。
    ///
    /// 供**复合**提示用（如设置页的「保存失败：{{detail}}」）：复合提示的
    /// 前缀已经交代了场合，再拼一遍本地化整句会读成
    /// 「保存失败：配置有误：…」——落盘失败与配置本身非法是两回事。
    pub(crate) fn for_error_detail(&self, error: &GlossError) -> String {
        match error {
            GlossError::EngineResponse(detail) | GlossError::Config(detail) => detail.clone(),
            other => self.for_error(other),
        }
    }

    /// 取某 locale 的文案表。首次调用时解析两份文件并常驻，之后零锁读取。
    pub(crate) fn get(locale: Locale) -> &'static Self {
        static CATALOGS: OnceLock<[Text; 2]> = OnceLock::new();
        let catalogs = CATALOGS.get_or_init(|| [parse(ZH, "zh"), parse(EN, "en")]);
        match locale {
            Locale::Zh => &catalogs[0],
            Locale::En => &catalogs[1],
        }
    }
}

/// 解析一份文案文件；失败回落空表并记 error（有痕迹的降级）。
fn parse(raw: &str, locale: &str) -> Text {
    toml::from_str(raw).unwrap_or_else(|err| {
        error!(
            locale = %locale,
            error = %err,
            "ui text catalog failed to parse, falling back to an empty table"
        );
        Text::default()
    })
}

/// 填充 `{{name}}` 占位符：表里出现几次就替换几次。
pub(crate) fn fill(template: &str, args: &[(&str, &str)]) -> String {
    let mut out = template.to_owned();
    for (name, value) in args {
        // format! 里 `{{` 转义成 `{`，四个花括号合起来就是占位符的 `{{`。
        out = out.replace(&format!("{{{{{name}}}}}"), value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_locale_files_declare_the_same_keys() {
        let zh = leaves_of(ZH);
        let en = leaves_of(EN);
        assert_eq!(
            zh.len(),
            76,
            "the entry count is pinned so a walker that stops recursing cannot pass"
        );
        assert_eq!(
            zh.iter().map(|(key, _)| key).collect::<Vec<_>>(),
            en.iter().map(|(key, _)| key).collect::<Vec<_>>(),
            "zh.toml and en.toml must declare the same keys"
        );
    }

    #[test]
    fn entries_are_written_fully_qualified() {
        for (name, raw) in [("zh.toml", ZH), ("en.toml", EN)] {
            for line in raw.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let Some((key, _)) = line.split_once(" = ") else {
                    panic!("{name}: not a key/value line: {line}");
                };
                assert!(
                    key.starts_with("gloss_") && !key.contains('.'),
                    "{name}: a key must be gloss_<module>_<entry>, underscore-joined: {key}"
                );
            }
        }
    }

    #[test]
    fn every_entry_is_translated_in_the_english_catalog() {
        let zh = leaves_of(ZH);
        let en = leaves_of(EN);
        assert_eq!(zh.len(), en.len());
        for ((key, zh_value), (_, en_value)) in zh.iter().zip(&en) {
            if key == "gloss_ui_language_en" {
                assert_eq!(
                    en_value, "English",
                    "a language's own name is not translated"
                );
                continue;
            }
            assert_ne!(
                zh_value, en_value,
                "{key} is copied from zh.toml instead of translated"
            );
        }
    }

    #[test]
    fn placeholders_match_across_locales() {
        let zh = leaves_of(ZH);
        let en = leaves_of(EN);
        assert_eq!(zh.len(), en.len());
        let mut templated = Vec::new();
        for ((key, zh_value), (_, en_value)) in zh.iter().zip(&en) {
            assert_eq!(
                placeholders(zh_value),
                placeholders(en_value),
                "{key} declares different placeholders per locale"
            );
            if !placeholders(zh_value).is_empty() {
                templated.push(key.as_str());
            }
        }
        assert_eq!(
            templated,
            [
                "gloss_errors_config",
                "gloss_errors_engine_response",
                "gloss_settings_error_duplicate_hotkey",
                "gloss_settings_error_invalid_trigger",
                "gloss_settings_invalid_summary",
                "gloss_settings_notice_key_update_failed",
                "gloss_settings_notice_key_updated_save_failed",
                "gloss_settings_notice_save_failed",
                "gloss_settings_switch_label",
            ],
            "every templated entry must be walked"
        );
    }

    #[test]
    fn catalogs_parse_into_typed_fields() {
        assert_eq!(Text::get(Locale::Zh).gloss_settings_save, "保存");
        assert_eq!(Text::get(Locale::En).gloss_settings_save, "Save");
        assert_ne!(
            Text::get(Locale::Zh).gloss_popup_failed,
            Text::get(Locale::En).gloss_popup_failed
        );
    }

    #[test]
    fn error_text_maps_every_variant_per_locale() {
        let zh = Text::get(Locale::Zh);
        let en = Text::get(Locale::En);
        for error in [
            GlossError::SelectionUnavailable,
            GlossError::AccessibilityDenied,
            GlossError::ScreenCaptureDenied,
            GlossError::RegionTooLarge,
            GlossError::UnsupportedModality,
            GlossError::EngineNetwork,
            GlossError::EngineAuth,
            GlossError::EngineRateLimited,
        ] {
            let (zh_text, en_text) = (zh.for_error(&error), en.for_error(&error));
            assert!(!zh_text.is_empty() && !en_text.is_empty(), "{error:?}");
            assert_ne!(zh_text, en_text, "{error:?} must be translated, not copied");
        }
        assert_eq!(
            zh.for_error(&GlossError::EngineResponse("HTTP 400".into())),
            "服务返回异常：HTTP 400"
        );
        assert_eq!(
            en.for_error(&GlossError::Config("bad port".into())),
            "Invalid configuration: bad port"
        );
    }

    #[test]
    fn each_error_variant_maps_to_its_own_entry() {
        let errors = Text::get(Locale::Zh);
        for (error, expected) in [
            (
                GlossError::SelectionUnavailable,
                &errors.gloss_errors_selection_unavailable,
            ),
            (
                GlossError::AccessibilityDenied,
                &errors.gloss_errors_accessibility_denied,
            ),
            (
                GlossError::ScreenCaptureDenied,
                &errors.gloss_errors_screen_capture_denied,
            ),
            (
                GlossError::RegionTooLarge,
                &errors.gloss_errors_region_too_large,
            ),
            (
                GlossError::UnsupportedModality,
                &errors.gloss_errors_unsupported_modality,
            ),
            (
                GlossError::EngineNetwork,
                &errors.gloss_errors_engine_network,
            ),
            (GlossError::EngineAuth, &errors.gloss_errors_engine_auth),
            (
                GlossError::EngineRateLimited,
                &errors.gloss_errors_engine_rate_limited,
            ),
        ] {
            assert_eq!(
                &errors.for_error(&error),
                expected,
                "{error:?} must use its own entry"
            );
            assert_eq!(
                &errors.for_error_detail(&error),
                expected,
                "{error:?} has no diagnostic of its own"
            );
        }
        let response = GlossError::EngineResponse("HTTP 400".into());
        assert_eq!(
            errors.for_error(&response),
            fill(
                &errors.gloss_errors_engine_response,
                &[("detail", "HTTP 400")]
            )
        );
        let config = GlossError::Config("bad port".into());
        assert_eq!(
            errors.for_error(&config),
            fill(&errors.gloss_errors_config, &[("detail", "bad port")])
        );
    }

    #[test]
    fn error_detail_prefers_the_variant_diagnostic() {
        let zh = Text::get(Locale::Zh);
        assert_eq!(
            zh.for_error_detail(&GlossError::Config("disk on fire".into())),
            "disk on fire",
            "a compound notice must not restate the standalone sentence"
        );
        assert_eq!(
            zh.for_error_detail(&GlossError::EngineNetwork),
            zh.for_error(&GlossError::EngineNetwork),
            "a variant without its own detail falls back to the sentence"
        );
    }

    #[test]
    fn fill_replaces_every_named_placeholder() {
        assert_eq!(
            fill("有 {{count}} 处，共 {{count}} 次", &[("count", "2")]),
            "有 2 处，共 2 次"
        );
        assert_eq!(fill("没有占位符", &[("count", "2")]), "没有占位符");
        assert_eq!(
            fill("未知 {{other}}", &[("count", "2")]),
            "未知 {{other}}",
            "a placeholder without an argument stays verbatim"
        );
    }

    fn leaves_of(raw: &str) -> Vec<(String, String)> {
        let value: toml::Value = toml::from_str(raw).expect("catalog must parse");
        let mut leaves = Vec::new();
        collect_leaves(&value, "", &mut leaves);
        leaves.sort_by(|a, b| a.0.cmp(&b.0));
        leaves
    }

    fn collect_leaves(value: &toml::Value, prefix: &str, out: &mut Vec<(String, String)>) {
        let toml::Value::Table(table) = value else {
            out.push((
                prefix.to_owned(),
                value.as_str().expect("every entry is a string").to_owned(),
            ));
            return;
        };
        for (key, child) in table {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            collect_leaves(child, &path, out);
        }
    }

    fn placeholders(template: &str) -> Vec<String> {
        let mut names = Vec::new();
        let mut rest = template;
        while let Some(start) = rest.find("{{") {
            let Some(end) = rest[start..].find("}}") else {
                break;
            };
            names.push(rest[start + 2..start + end].to_owned());
            rest = &rest[start + end + 2..];
        }
        names.sort();
        names.dedup();
        names
    }
}
