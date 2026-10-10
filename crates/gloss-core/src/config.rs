//! 配置模型：`Config` 结构与出厂默认。
//!
//! 密钥红线：配置里只存 keychain 条目标识，密钥本体永不进
//! `Config`——运行时整份快照可被任意线程读取，不能携带凭据。

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::model::{Lang, Locale};

/// 出厂默认模型 id：`Config::model` 的出厂值——单一模型承接全部任务
/// （分类与执行同一模型），文本与图像模态不分（引擎按不透明 id 透传）。
/// 设置页可改；App 在触发时冻结进任务选项。
pub const DEFAULT_MODEL: &str = "deepseek-flash";

/// 出厂默认 OpenAI 兼容端点：DeepSeek。客户端按
/// `{base_url}/chat/completions` 拼接，设置页可改。
pub const DEFAULT_BASE_URL: &str = "https://api.deepseek.com/v1";

/// 界面主题：跟随系统 / 固定浅色 / 固定深色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Theme {
    /// 跟随系统外观。
    #[default]
    System,
    /// 固定浅色。
    Light,
    /// 固定深色。
    Dark,
}

/// 界面语言：出厂跟随系统。落定成 [`Locale`]，供 prompt 模板（触发时解析、
/// 随任务冻结）与界面文案（渲染帧逐帧取表）共用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Language {
    /// 跟随系统语言。
    #[default]
    System,
    /// 简体中文。
    Zh,
    /// English。
    En,
}

impl Language {
    /// 本语言落定成的 [`Locale`]：显式选择直接用；`System` 按调用方给的
    /// **系统语言**落定——进程内系统语言只会读一次（改系统语言要重启），
    /// 因此这是纯映射，不做任何探测。prompt 模板与界面文案共用这一份结果。
    pub fn resolve(self, system: Locale) -> Locale {
        match self {
            Language::System => system,
            Language::Zh => Locale::Zh,
            Language::En => Locale::En,
        }
    }
}

/// 缓存有效期的合法上限（秒）：30 天。0 = 永不失效（合法，进程内缓存
/// 随退出清空）；上限只为拦手滑输入的天文数字。
pub const CACHE_TTL_MAX_SECS: u64 = 30 * 24 * 60 * 60;

/// Base URL 的结构性问题类别（[`validate_base_url`] 的失败面）。文案由
/// 展示层映射；`Display` 是引擎侧的英文消息，与既有引擎检查逐字一致。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaseUrlError {
    /// trim 后为空。
    Empty,
    /// 缺 scheme/host 等结构性残缺，或含空白与控制字符。
    Invalid,
    /// scheme 不是 https。
    NotHttps,
    /// authority 段内嵌账号密码（会被 reqwest 抽成 Basic 认证）。
    EmbeddedCredentials,
    /// 带 query 或 fragment（路径拼接会落错位置）。
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

/// Base URL 的结构性校验（纯逻辑）：设置页逐字段校验与引擎请求前检查
/// 共用这一份规则，避免两套口径漂移。规则与既有引擎检查一致：
/// 非空、https、无内嵌凭据、无 query/fragment、结构可解析；尾斜杠宽容。
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
    // scheme 以外的部分即 authority + path；query/fragment 全局拒绝。
    let Some((scheme, rest)) = url.split_once("://") else {
        return Err(BaseUrlError::Invalid);
    };
    if !scheme.is_ascii() || scheme.is_empty() {
        return Err(BaseUrlError::Invalid);
    }
    // scheme 大小写不敏感（url crate 会归一化为小写，`HTTPS://` 一直合法）。
    if !scheme.eq_ignore_ascii_case("https") {
        return Err(BaseUrlError::NotHttps);
    }
    if rest.contains('?') || rest.contains('#') {
        return Err(BaseUrlError::QueryOrFragment);
    }
    // authority 到第一个 `/` 为止；`@` 出现在其中即内嵌凭据（须在
    // host/port 拆分前检查——userinfo 里可能含冒号）。
    let authority = rest.split('/').next().unwrap_or_default();
    if authority.contains('@') {
        return Err(BaseUrlError::EmbeddedCredentials);
    }
    // host 与端口拆分：IPv6 字面量（[...]）自带冒号，需先剥方括号。
    let (host, port) = if let Some(v6_part) = authority.strip_prefix('[') {
        match v6_part.split_once(']') {
            Some((v6, "")) => (format!("[{v6}]"), None),
            Some((v6, port)) => {
                let Some(port) = port.strip_prefix(':') else {
                    return Err(BaseUrlError::Invalid);
                };
                (format!("[{v6}]"), Some(port.to_owned()))
            }
            None => return Err(BaseUrlError::Invalid),
        }
    } else {
        match authority.rsplit_once(':') {
            Some((host, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
                (host.to_owned(), Some(port.to_owned()))
            }
            Some(_) => return Err(BaseUrlError::Invalid),
            None => (authority.to_owned(), None),
        }
    };
    if host.is_empty() {
        return Err(BaseUrlError::Invalid);
    }
    if let Some(port) = port
        && port.parse::<u32>().map(|p| p > 65535).unwrap_or(true)
    {
        return Err(BaseUrlError::Invalid);
    }
    Ok(())
}

/// 一个 provider 的密钥条目：`keychain_id` 是密钥在系统 keychain 里的
/// 条目标识，读取走 `ConfigStore::secret`，配置文件中永不出现密钥本体。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderKey {
    /// provider 名称，如 "deepseek"。
    pub provider: String,
    /// keychain 条目标识。
    pub keychain_id: String,
}

/// 出厂 provider 条目：密钥本体永远只在 keychain，这里只给条目标识。
fn factory_provider() -> ProviderKey {
    ProviderKey {
        provider: "deepseek".into(),
        keychain_id: "gloss/deepseek".into(),
    }
}

/// 出厂 provider 条目的静态承载：`resolved_provider` 要返回引用，兜底时给它。
static FACTORY_PROVIDER: OnceLock<ProviderKey> = OnceLock::new();

/// 出厂默认 provider 表（目前只有一条）。
fn default_provider_keys() -> Vec<ProviderKey> {
    vec![factory_provider()]
}

/// 应用配置：持久化为 TOML（`FileConfigStore`），运行时以整份快照在
/// 各线程间共享（共享形态不携带密钥，见模块文档红线）。反序列化带 `#[serde(default)]`：
/// 手改配置缺字段时按出厂默认补齐，不允许半份配置带病运行；退役字段
/// （历史版本写下的多余键）静默忽略，测试有钉子。
///
/// 落点（改动本节时同步更新）：`target_lang` / `model` / `language` 已接线
/// （触发时由状态机按快照冻结进任务选项）；`base_url` / `provider_keys`
/// 已接线（引擎每请求解析端点、按条目直查 keychain）；`theme` 已接线
/// （主题施加到两个 egui 上下文）；`cache_ttl_secs` 归 gloss-app 的缓存
/// 构造接线。都不在触发时冻结的只有 `theme`（渲染帧读取）。
///
/// 敏感信息防护不在此列：它不是配置项，判据内建在 `gloss_core::guard`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// OpenAI 兼容端点的基地址，客户端按 `{base_url}/chat/completions`
    /// 拼接（尾斜杠会被去掉）。MVP 单端点，多 provider 路由出现时再把它
    /// 移进 provider 条目；空串视为未配置（引擎明确报错，不猜端点）。
    pub base_url: String,
    /// 各 provider 的 keychain 条目标识；密钥本体只在 keychain。
    pub provider_keys: Vec<ProviderKey>,
    /// 全部任务共用的模型 id：分类与执行同一模型，触发时随任务选项冻结。
    pub model: String,
    /// 翻译类任务的默认目标语言。
    pub target_lang: Lang,
    /// 缓存条目存活时长（秒）；0 = 永不失效，上限 [`CACHE_TTL_MAX_SECS`]。
    pub cache_ttl_secs: u64,
    /// 界面主题。
    pub theme: Theme,
    /// 界面语言。
    pub language: Language,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            provider_keys: default_provider_keys(),
            model: DEFAULT_MODEL.to_owned(),
            target_lang: Lang::Zh,
            // 与 gloss-app::cache 的出厂 TTL（1 小时）一致（同源校验测试
            // 在 app 侧，随 DEFAULT_TTL 钉住）。
            cache_ttl_secs: 60 * 60,
            theme: Theme::System,
            language: Language::System,
        }
    }
}

impl Config {
    /// 本任务实际使用的 provider 条目：MVP 单端点，取最后一条（与
    /// `keychain_id_for` 的「后条覆盖前条」一致）；`provider_keys` 为空时返回
    /// `None`（纯查表）。
    pub fn active_provider(&self) -> Option<&ProviderKey> {
        self.provider_keys.last()
    }

    /// 引擎实际使用的 provider 条目：空 `provider_keys` 时回退出厂条目（只带
    /// keychain 条目标识，不带密钥），因此**恒有值**——真正的失败面是「条目指
    /// 向的密钥没设」（`EngineAuth`，UI 引导去设置页）。
    ///
    /// 兜底不只是「方便」：旧版本落盘的出厂值是空数组，而 `#[serde(default)]`
    /// 只补缺失字段——那批机器上落盘的 `provider_keys = []` 会一直留着，设置页
    /// 落地前又没有改它的 UI。
    pub fn resolved_provider(&self) -> &ProviderKey {
        self.active_provider()
            .unwrap_or_else(|| FACTORY_PROVIDER.get_or_init(factory_provider))
    }

    /// 查某 provider 的 keychain 条目标识（同 provider 多条时后条覆盖前条）。
    pub fn keychain_id_for(&self, provider: &str) -> Option<&str> {
        self.provider_keys
            .iter()
            .rev()
            .find(|entry| entry.provider == provider)
            .map(|entry| entry.keychain_id.as_str())
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
    }

    #[test]
    fn base_url_validation_rejects_structural_problems() {
        assert_eq!(validate_base_url(""), Err(BaseUrlError::Empty));
        assert_eq!(validate_base_url("   "), Err(BaseUrlError::Empty));
        assert_eq!(
            validate_base_url("ftp://api.example.com/v1"),
            Err(BaseUrlError::NotHttps)
        );
        assert_eq!(
            validate_base_url("api.example.com/v1"),
            Err(BaseUrlError::Invalid)
        );
        assert_eq!(validate_base_url("https:///v1"), Err(BaseUrlError::Invalid));
        assert_eq!(
            validate_base_url("https://api.example .com/v1"),
            Err(BaseUrlError::Invalid),
            "trim 只作用于首尾，中部空白仍拒绝"
        );
        assert_eq!(
            validate_base_url("https://user:pass@api.example.com"),
            Err(BaseUrlError::EmbeddedCredentials)
        );
        assert_eq!(
            validate_base_url("https://api.example.com/v1?x=1"),
            Err(BaseUrlError::QueryOrFragment)
        );
        assert_eq!(
            validate_base_url("https://api.example.com/v1?x=1"),
            Err(BaseUrlError::QueryOrFragment)
        );
        assert_eq!(
            validate_base_url("https://:8080/v1"),
            Err(BaseUrlError::Invalid),
            "缺 host 只带端口仍拒绝"
        );
        assert_eq!(
            validate_base_url("https://api.example.com:99999"),
            Err(BaseUrlError::Invalid),
            "端口超出 16 位范围拒绝"
        );
        assert_eq!(
            validate_base_url("https://api.example.com:abc"),
            Err(BaseUrlError::Invalid),
            "非数字端口拒绝"
        );
        assert_eq!(
            validate_base_url("https://[::1/v1"),
            Err(BaseUrlError::Invalid),
            "IPv6 括号不闭合拒绝"
        );
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
            target_lang: Lang::Other("ko".into()),
            cache_ttl_secs: 120,
            theme: Theme::Dark,
            language: Language::En,
        };
        let json = serde_json::to_string(&config).expect("config should serialize");
        let back: Config = serde_json::from_str(&json).expect("config should deserialize");
        assert_eq!(back, config);
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

        assert_eq!(config.keychain_id_for("deepseek"), Some("gloss/deepseek-2"));
        assert_eq!(config.keychain_id_for("openai"), Some("gloss/openai"));
        assert_eq!(config.keychain_id_for("mistral"), None);
        assert_eq!(
            config.active_provider().map(|p| &p.provider),
            Some(&"openai".to_owned())
        );
    }
}
