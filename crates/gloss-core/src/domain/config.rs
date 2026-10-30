//! 配置模型：配置结构与出厂默认。
//!
//! 目标语言的退役值（历史版本写入的日语、韩语、法语与自定义名）在反序
//! 列化时有痕降级到缺省目标语言——沿用部分解析先例，单字段回落不破整份
//! 配置。
//!
//! 密钥红线：配置里只存钥匙串条目标识，密钥本体永不进
//! 配置——运行时整份快照可被任意线程读取，不能携带凭据。

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::log::{thread, warn};
use crate::model::{Lang, Locale};

pub const DEFAULT_MODEL: &str = "deepseek-flash";

pub const DEFAULT_BASE_URL: &str = "https://api.deepseek.com/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Language {
    #[default]
    System,
    Zh,
    En,
}

impl Language {
    pub fn resolve(self, system: Locale) -> Locale {
        match self {
            Language::System => system,
            Language::Zh => Locale::Zh,
            Language::En => Locale::En,
        }
    }
}

pub const CACHE_TTL_MAX_SECS: u64 = 30 * 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaseUrlError {
    Empty,
    Invalid,
    NotHttps,
    EmbeddedCredentials,
    QueryOrFragment,
}

impl std::fmt::Display for BaseUrlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BaseUrlError::Empty => write!(f, "no provider endpoint configured"),
            BaseUrlError::Invalid => write!(f, "invalid provider endpoint"),
            BaseUrlError::NotHttps => write!(f, "provider endpoint must use https"),
            BaseUrlError::EmbeddedCredentials => {
                write!(f, "provider endpoint must not embed credentials")
            }
            BaseUrlError::QueryOrFragment => {
                write!(f, "provider endpoint must not carry query or fragment")
            }
        }
    }
}

pub fn validate_base_url(raw: &str) -> Result<(), BaseUrlError> {
    let url = raw.trim();
    if url.is_empty() {
        return Err(BaseUrlError::Empty);
    }
    if url
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || c == '<' || c == '>')
    {
        return Err(BaseUrlError::Invalid);
    }
    let Some((scheme, rest)) = url.split_once("://") else {
        return Err(BaseUrlError::Invalid);
    };
    if !scheme.is_ascii() || scheme.is_empty() {
        return Err(BaseUrlError::Invalid);
    }
    if !scheme.eq_ignore_ascii_case("https") {
        return Err(BaseUrlError::NotHttps);
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() {
        return Err(BaseUrlError::Invalid);
    }
    if authority.contains('@') {
        return Err(BaseUrlError::EmbeddedCredentials);
    }
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return Err(BaseUrlError::Invalid);
    };
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(BaseUrlError::QueryOrFragment);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderKey {
    pub provider: String,
    pub keychain_id: String,
}

fn factory_provider() -> ProviderKey {
    ProviderKey {
        provider: "deepseek".into(),
        keychain_id: "gloss/deepseek".into(),
    }
}

static FACTORY_PROVIDER: OnceLock<ProviderKey> = OnceLock::new();

fn default_provider_keys() -> Vec<ProviderKey> {
    vec![factory_provider()]
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub base_url: String,
    pub provider_keys: Vec<ProviderKey>,
    pub model: String,
    #[serde(deserialize_with = "deserialize_target_lang")]
    pub target_lang: Lang,
    pub cache_ttl_secs: u64,
    pub theme: Theme,
    pub language: Language,
    pub watch_clipboard_images: bool,
}

fn deserialize_target_lang<'de, D>(deserializer: D) -> Result<Lang, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Lang::deserialize(deserializer).or_else(|_| {
        warn!(
            thread = thread::UI,
            "config target_lang names a retired or unsupported language, falling back \
             to the default target language"
        );
        Ok(Lang::default())
    })
}

impl Default for Config {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            provider_keys: default_provider_keys(),
            model: DEFAULT_MODEL.to_owned(),
            target_lang: Lang::Zh,
            cache_ttl_secs: 60 * 60,
            theme: Theme::System,
            language: Language::System,
            watch_clipboard_images: true,
        }
    }
}

impl Config {
    pub fn active_provider(&self) -> Option<&ProviderKey> {
        self.provider_keys.last()
    }

    pub fn resolved_provider(&self) -> &ProviderKey {
        self.active_provider()
            .unwrap_or_else(|| FACTORY_PROVIDER.get_or_init(factory_provider))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_defaults_match_spec() {
        let config = Config::default();
        assert_eq!(config.target_lang, Lang::Zh);
        assert_eq!(config.model, DEFAULT_MODEL);
        assert_eq!(config.cache_ttl_secs, 60 * 60);
        assert_eq!(config.theme, Theme::System);
        assert_eq!(config.language, Language::System);
        assert_eq!(config.base_url, DEFAULT_BASE_URL);
        assert!(
            config.watch_clipboard_images,
            "clipboard image watch must default to on"
        );

        let provider = config.active_provider().expect("factory provider");
        assert_eq!(provider.provider, "deepseek");
        assert_eq!(provider.keychain_id, "gloss/deepseek");
    }

    #[test]
    fn missing_fields_fall_back_to_defaults_on_deserialize() {
        let json = serde_json::json!({
            "base_url": DEFAULT_BASE_URL,
            "provider_keys": [],
            "target_lang": "Zh",
            "cache_ttl_secs": 0,
            "theme": "System",
        });
        let config: Config =
            serde_json::from_value(json).expect("missing language must fall back to default");
        assert_eq!(config.language, Language::System);
        assert_eq!(config.model, DEFAULT_MODEL);
        assert!(
            config.watch_clipboard_images,
            "a config file without the watch key must fall back to on"
        );
    }

    #[test]
    fn base_url_validation_rejects_structural_problems() {
        for (url, expected) in [
            ("", BaseUrlError::Empty),
            ("   ", BaseUrlError::Empty),
            ("ftp://api.example.com/v1", BaseUrlError::NotHttps),
            ("api.example.com/v1", BaseUrlError::Invalid),
            ("https:///v1", BaseUrlError::Invalid),
            ("https://api.example .com/v1", BaseUrlError::Invalid),
            (
                "https://user:pass@api.example.com",
                BaseUrlError::EmbeddedCredentials,
            ),
            (
                "https://@api.example.com/v1",
                BaseUrlError::EmbeddedCredentials,
            ),
            ("http:///v1", BaseUrlError::NotHttps),
            (
                "https://api.example.com?token=a@b",
                BaseUrlError::QueryOrFragment,
            ),
            (
                "https://api.example.com/v1?x=1",
                BaseUrlError::QueryOrFragment,
            ),
            ("https://:8080/v1", BaseUrlError::Invalid),
            ("https://api.example.com:99999", BaseUrlError::Invalid),
            ("https://api.example.com:abc", BaseUrlError::Invalid),
            ("https://[::1/v1", BaseUrlError::Invalid),
        ] {
            assert_eq!(validate_base_url(url), Err(expected), "{url}");
        }
    }

    #[test]
    fn base_url_validation_accepts_legal_addresses() {
        assert_eq!(validate_base_url("https://api.deepseek.com/v1"), Ok(()));
        assert_eq!(validate_base_url("  https://api.deepseek.com  "), Ok(()));
        assert_eq!(validate_base_url("https://api.deepseek.com"), Ok(()));
        assert_eq!(validate_base_url("https://127.0.0.1:8080/v1"), Ok(()));
        assert_eq!(
            validate_base_url("HTTPS://Api.Example.com/v1"),
            Ok(()),
            "scheme 大小写不敏感（url crate 归一化语义）"
        );
        assert_eq!(validate_base_url("https://[::1]:8080/v1"), Ok(()));
    }

    #[test]
    fn language_resolves_to_locale() {
        assert_eq!(
            Language::System.resolve(Locale::En),
            Locale::En,
            "follow-the-system must take the detected system language"
        );
        assert_eq!(Language::System.resolve(Locale::Zh), Locale::Zh);
        assert_eq!(
            Language::Zh.resolve(Locale::En),
            Locale::Zh,
            "an explicit choice ignores the system language"
        );
        assert_eq!(Language::En.resolve(Locale::Zh), Locale::En);
    }

    #[test]
    fn config_round_trips_through_serde() {
        let config = Config {
            base_url: "https://example.test/v1".into(),
            provider_keys: vec![ProviderKey {
                provider: "deepseek".into(),
                keychain_id: "gloss/deepseek".into(),
            }],
            model: "custom-model".into(),
            target_lang: Lang::En,
            cache_ttl_secs: 120,
            theme: Theme::Dark,
            language: Language::En,
            watch_clipboard_images: true,
        };
        let json = serde_json::to_string(&config).expect("config should serialize");
        let back: Config = serde_json::from_str(&json).expect("config should deserialize");
        assert_eq!(back, config);
    }

    #[test]
    fn retired_target_lang_values_degrade_to_the_default_with_a_trace() {
        let _serial = crate::log::test_support::lock_dispatchers();
        for value in [
            r#""Ja""#,
            r#""Ko""#,
            r#""Fr""#,
            r#"{ Other = "ko" }"#,
            r#""es""#,
            "7",
        ] {
            let logs = crate::log::capture(|| {
                let config: Config = toml::from_str(&format!("target_lang = {value}"))
                    .expect("a retired target language must degrade, not fail the load");
                assert_eq!(config.target_lang, Lang::Zh, "{value}");
                assert_eq!(
                    config.cache_ttl_secs,
                    60 * 60,
                    "{value}: the other fields keep their defaults"
                );
            });
            assert!(
                logs.contains("falling back"),
                "{value}: the degradation must leave a warn trace: {logs}"
            );
        }
    }

    #[test]
    fn supported_target_lang_values_still_parse() {
        assert_eq!(
            toml::from_str::<Config>("target_lang = \"Zh\"")
                .expect("zh parses")
                .target_lang,
            Lang::Zh
        );
        assert_eq!(
            toml::from_str::<Config>("target_lang = \"En\"")
                .expect("en parses")
                .target_lang,
            Lang::En
        );
    }

    #[test]
    fn retired_fields_are_ignored_on_load() {
        let config: Config = serde_json::from_str(
            r#"{"theme": "Light", "guard_enabled": false, "guard_blocked_apps": ["com.example.vault"], "hotkey_bindings": [{"trigger": "Cmd+Shift+D"}], "model_by_kind": [{"kind": "TranslateWord", "model": "old-model"}], "default_text_kind": "ExplainCode", "enabled_kinds": ["TranslateWord"]}"#,
        )
        .expect("a config written by an older version must still load");
        assert_eq!(config.theme, Theme::Light, "known fields keep their values");
        assert_eq!(config.target_lang, Lang::Zh, "missing fields still default");
        assert_eq!(
            config.model, DEFAULT_MODEL,
            "retired per-kind model tables must not leak into the single model"
        );
    }

    #[test]
    fn partial_document_fills_factory_defaults() {
        let config: Config =
            serde_json::from_str(r#"{"theme": "Light"}"#).expect("partial config should parse");
        assert_eq!(config.theme, Theme::Light, "present field must be kept");
        assert_eq!(config.target_lang, Lang::Zh, "missing field must default");
        assert_eq!(config.cache_ttl_secs, 60 * 60);
        assert_eq!(config.base_url, DEFAULT_BASE_URL);
        assert_eq!(config.resolved_provider().provider, "deepseek");
        assert_eq!(config.model, DEFAULT_MODEL);
    }

    #[test]
    fn lookups_prefer_later_entries() {
        let config = Config {
            provider_keys: vec![
                ProviderKey {
                    provider: "deepseek".into(),
                    keychain_id: "gloss/deepseek".into(),
                },
                ProviderKey {
                    provider: "deepseek".into(),
                    keychain_id: "gloss/deepseek-2".into(),
                },
                ProviderKey {
                    provider: "openai".into(),
                    keychain_id: "gloss/openai".into(),
                },
            ],
            ..Default::default()
        };

        assert_eq!(
            config.active_provider().map(|p| &p.provider),
            Some(&"openai".to_owned())
        );
    }
}
