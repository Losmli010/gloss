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

/// 一个 locale 的全部界面文案。
///
/// 字段名（= 模块名）与 TOML 节名分处两侧，靠 `rename` 对齐：节名一律
/// `gloss_<模块>`，于是词条的全限定名是 `gloss_<模块>.<词条>`——扁平且全局唯一，
/// 便于整表导入外部翻译平台。改节名要连同改这里。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Text {
    #[serde(rename = "gloss_app")]
    pub(crate) app: AppText,
    #[serde(rename = "gloss_kinds")]
    pub(crate) kinds: KindText,
    #[serde(rename = "gloss_langs")]
    pub(crate) langs: LangText,
    #[serde(rename = "gloss_popup")]
    pub(crate) popup: PopupText,
    #[serde(rename = "gloss_errors")]
    pub(crate) errors: ErrorText,
    #[serde(rename = "gloss_settings")]
    pub(crate) settings: SettingsText,
    #[serde(rename = "gloss_ui_language")]
    pub(crate) ui_language: UiLanguageText,
    #[serde(rename = "gloss_theme")]
    pub(crate) theme: ThemeText,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AppText {
    pub(crate) settings_title: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct KindText {
    pub(crate) translate_word: String,
    pub(crate) translate_sentence: String,
    pub(crate) explain_code: String,
    pub(crate) image_ocr: String,
    pub(crate) image_explain: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LangText {
    pub(crate) zh: String,
    pub(crate) en: String,
    pub(crate) ja: String,
    pub(crate) ko: String,
    pub(crate) fr: String,
    pub(crate) other: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PopupText {
    pub(crate) brand: String,
    pub(crate) selfcheck: String,
    pub(crate) failed: String,
    pub(crate) retry: String,
    pub(crate) open_settings: String,
    pub(crate) close_label: String,
    pub(crate) settings_label: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ErrorText {
    pub(crate) selection_unavailable: String,
    pub(crate) accessibility_denied: String,
    pub(crate) screen_capture_denied: String,
    pub(crate) region_too_large: String,
    pub(crate) unsupported_modality: String,
    pub(crate) engine_network: String,
    pub(crate) engine_auth: String,
    pub(crate) engine_rate_limited: String,
    pub(crate) engine_response: String,
    pub(crate) config: String,
    pub(crate) acquire_channel: String,
    pub(crate) inference_channel: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SettingsText {
    pub(crate) section_model: String,
    pub(crate) section_task: String,
    pub(crate) section_hotkey: String,
    pub(crate) section_general: String,
    pub(crate) save: String,
    pub(crate) cancel: String,
    pub(crate) invalid_summary: String,
    pub(crate) base_url_hint: String,
    pub(crate) key_keep_hint: String,
    pub(crate) key_clear_hint: String,
    pub(crate) clear_key: String,
    pub(crate) undo_clear_key: String,
    pub(crate) default_kind: String,
    pub(crate) default_kind_hint: String,
    pub(crate) target_lang: String,
    pub(crate) kind_switch: String,
    pub(crate) kind_switch_hint: String,
    pub(crate) default_model: String,
    pub(crate) default_model_hint: String,
    pub(crate) vision_model_hint: String,
    pub(crate) switch_label: String,
    pub(crate) hotkey_hint: String,
    pub(crate) ui_language: String,
    pub(crate) ui_theme: String,
    pub(crate) cache_ttl: String,
    pub(crate) cache_ttl_suffix: String,
    pub(crate) cache_ttl_hint: String,
    pub(crate) notice_key_update_failed: String,
    pub(crate) notice_save_failed: String,
    pub(crate) notice_key_updated_save_failed: String,
    pub(crate) error: SettingsErrorText,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SettingsErrorText {
    pub(crate) duplicate_hotkey: String,
    pub(crate) empty_trigger: String,
    pub(crate) invalid_trigger: String,
    pub(crate) newline_in_model: String,
    pub(crate) base_url_empty: String,
    pub(crate) base_url_invalid: String,
    pub(crate) base_url_credentials: String,
    pub(crate) base_url_query: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiLanguageText {
    pub(crate) system: String,
    pub(crate) zh: String,
    pub(crate) en: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ThemeText {
    pub(crate) system: String,
    pub(crate) light: String,
    pub(crate) dark: String,
}

impl ErrorText {
    /// 失败卡文案：按 [`GlossError`] 变体映射，不按英文 `Display` 反查
    /// ——后者是日志用的诊断文本，措辞与它无关（改 `Display` 不该改界面）。
    pub(crate) fn for_error(&self, error: &GlossError) -> String {
        match error {
            GlossError::SelectionUnavailable => self.selection_unavailable.clone(),
            GlossError::AccessibilityDenied => self.accessibility_denied.clone(),
            GlossError::ScreenCaptureDenied => self.screen_capture_denied.clone(),
            GlossError::RegionTooLarge => self.region_too_large.clone(),
            GlossError::UnsupportedModality => self.unsupported_modality.clone(),
            GlossError::EngineNetwork => self.engine_network.clone(),
            GlossError::EngineAuth => self.engine_auth.clone(),
            GlossError::EngineRateLimited => self.engine_rate_limited.clone(),
            GlossError::EngineResponse(detail) => {
                fill(&self.engine_response, &[("detail", detail)])
            }
            GlossError::Config(detail) => fill(&self.config, &[("detail", detail)]),
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
}

impl Text {
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
        let zh = keys_of(ZH);
        let en = keys_of(EN);
        assert!(zh.contains(&"gloss_settings.error.duplicate_hotkey".to_owned()));
        assert!(zh.len() > 60, "key walk must cover the whole table");
        assert_eq!(zh, en, "zh.toml and en.toml must declare the same keys");
    }

    #[test]
    fn catalogs_parse_into_typed_fields() {
        assert_eq!(Text::get(Locale::Zh).settings.save, "保存");
        assert_eq!(Text::get(Locale::En).settings.save, "Save");
        assert_ne!(
            Text::get(Locale::Zh).popup.failed,
            Text::get(Locale::En).popup.failed
        );
    }

    #[test]
    fn error_text_maps_every_variant_per_locale() {
        let zh = &Text::get(Locale::Zh).errors;
        let en = &Text::get(Locale::En).errors;
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
    fn error_detail_prefers_the_variant_diagnostic() {
        let zh = &Text::get(Locale::Zh).errors;
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

    fn keys_of(raw: &str) -> Vec<String> {
        let value: toml::Value = toml::from_str(raw).expect("catalog must parse");
        let mut keys = Vec::new();
        collect_keys(&value, "", &mut keys);
        keys.sort_unstable();
        keys
    }

    fn collect_keys(value: &toml::Value, prefix: &str, out: &mut Vec<String>) {
        let toml::Value::Table(table) = value else {
            return;
        };
        for (key, child) in table {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            out.push(path.clone());
            collect_keys(child, &path, out);
        }
    }
}
