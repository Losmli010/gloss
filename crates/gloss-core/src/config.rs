//! 配置模型：`Config` 结构、出厂默认与查找助手。
//!
//! 密钥红线：配置里只存 keychain 条目标识，密钥本体永不进
//! `Config`——运行时整份快照可被任意线程读取，不能携带凭据。

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::model::Lang;
use crate::task::{HotkeyBinding, InputSource, TaskKind};

/// 全部任务类型：`enabled_kinds` 的出厂值与设置页的任务开关列表共用，
/// 两处来自同一常量，新增 kind 时不会漏掉一边。
pub const ALL_KINDS: [TaskKind; 5] = [
    TaskKind::TranslateWord,
    TaskKind::TranslateSentence,
    TaskKind::ExplainCode,
    TaskKind::ImageOcr,
    TaskKind::ImageExplain,
];

/// 出厂默认文本模型 id：既是 `model_by_kind` 的出厂值，也是 `model_by_kind`
/// 缺项时的兜底——两处共用同一处字面量（各写一份时改一边不会有人红）。
pub const DEFAULT_TEXT_MODEL: &str = "deepseek-chat";

/// 出厂默认 OpenAI 兼容端点：DeepSeek。客户端按
/// `{base_url}/chat/completions` 拼接，设置页可改。
pub const DEFAULT_BASE_URL: &str = "https://api.deepseek.com/v1";

/// 出厂默认热键表：与 `gloss-platform::events::hotkey` 的写死默认一致。
/// 有意避开系统截图（Cmd+Shift+3/4/5）等系统级组合。
fn default_hotkey_bindings() -> Vec<HotkeyBinding> {
    fn selection(trigger: &str, kind: TaskKind) -> HotkeyBinding {
        HotkeyBinding {
            trigger: trigger.to_owned(),
            kind,
            source: InputSource::Selection,
        }
    }
    vec![
        selection("Cmd+Shift+D", TaskKind::TranslateWord),
        selection("Cmd+Shift+F", TaskKind::TranslateSentence),
        selection("Cmd+Shift+E", TaskKind::ExplainCode),
    ]
}

/// 界面主题。
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

/// 界面语言（与 [`Theme`] 同为壳侧展示开关：不随任务冻结）。
/// 出厂跟随系统；消费归 prompt locale（R1）与 UI 文案翻译（R7）。
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

/// 任务类型 → 默认模型 id 的绑定：统一 LLM 客户端下，模态能力差异是
/// 配置问题——文本任务配文本模型、图像任务配视觉模型。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelBinding {
    /// 任务类型。
    pub kind: TaskKind,
    /// 该任务默认使用的模型 id。
    pub model: String,
}

/// 出厂默认模型表：文本类任务先给出可用默认（用户装完只差一把钥匙），
/// 图像类留空——视觉模型随图像任务接入，未配置时由引擎报能力不匹配。
fn default_model_bindings() -> Vec<ModelBinding> {
    [
        TaskKind::TranslateWord,
        TaskKind::TranslateSentence,
        TaskKind::ExplainCode,
    ]
    .into_iter()
    .map(|kind| ModelBinding {
        kind,
        model: DEFAULT_TEXT_MODEL.to_owned(),
    })
    .collect()
}

/// 应用配置：持久化为 TOML（`FileConfigStore`），运行时以整份快照在
/// 各线程间共享（共享形态不携带密钥，见模块文档红线）。反序列化带 `#[serde(default)]`：
/// 手改配置缺字段时按出厂默认补齐，不允许半份配置带病运行。
///
/// 落点（改动本节时同步更新）：`target_lang` / `model_by_kind` /
/// `default_text_kind` 已接线（触发时解析进任务）；`base_url` /
/// `provider_keys` 已接线（引擎每请求解析端点、按条目直查 keychain）；
/// `enabled_kinds` 已接线（触发时过滤）；`hotkey_bindings` /
/// `theme` 已接线（保存后重注册热键；主题施加到两个 egui 上下文）——
/// 都**不**在触发时冻结；`cache_ttl_secs` 归缓存构造接线；
/// `language` 已持久化，消费归 prompt locale（R1）与 UI 文案翻译（R7）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// OpenAI 兼容端点的基地址，客户端按 `{base_url}/chat/completions`
    /// 拼接（尾斜杠会被去掉）。MVP 单端点，多 provider 路由出现时再把它
    /// 移进 provider 条目；空串视为未配置（引擎明确报错，不猜端点）。
    pub base_url: String,
    /// 各 provider 的 keychain 条目标识；密钥本体只在 keychain。
    pub provider_keys: Vec<ProviderKey>,
    /// 每个任务类型的默认模型 id。
    pub model_by_kind: Vec<ModelBinding>,
    /// 翻译类任务的默认目标语言。
    pub target_lang: Lang,
    /// 全局热键 → 任务类型 + 输入源。
    pub hotkey_bindings: Vec<HotkeyBinding>,
    /// 划词手势的默认任务类型。
    pub default_text_kind: TaskKind,
    /// 启用的任务类型：被停用的 kind 对一切触发路径无响应（设置页任务
    /// 开关）。缺字段（老配置）按全启用补齐。
    pub enabled_kinds: Vec<TaskKind>,
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
            model_by_kind: default_model_bindings(),
            target_lang: Lang::Zh,
            hotkey_bindings: default_hotkey_bindings(),
            default_text_kind: TaskKind::TranslateWord,
            enabled_kinds: ALL_KINDS.to_vec(),
            // 与 gloss-core::cache 的出厂 TTL（1 小时）一致。
            cache_ttl_secs: 60 * 60,
            theme: Theme::System,
            language: Language::System,
        }
    }
}

impl Config {
    /// 某任务类型是否启用：停用的 kind 对一切触发路径无响应（设置页任务
    /// 开关）。空表视为「全部停用」——显式清空是用户的明确意图，不猜。
    pub fn is_kind_enabled(&self, kind: TaskKind) -> bool {
        self.enabled_kinds.contains(&kind)
    }

    /// 设置某任务类型的开关（设置页任务开关的落点）：启用为追加、停用为
    /// 移除，保证表内不出现重复条目。
    pub fn set_kind_enabled(&mut self, kind: TaskKind, enabled: bool) {
        if enabled {
            if !self.enabled_kinds.contains(&kind) {
                self.enabled_kinds.push(kind);
            }
        } else {
            self.enabled_kinds.retain(|&candidate| candidate != kind);
        }
    }

    /// 设置某任务类型的默认模型 id（设置页模型表的落点）：替换该 kind 的
    /// 全部既有条目（查找助手按「后条覆盖前条」语义，整表重建前不留旧条
    /// 目）；空串/纯空白视为未配置，移除条目。
    pub fn set_model_for_kind(&mut self, kind: TaskKind, model: &str) {
        self.model_by_kind.retain(|binding| binding.kind != kind);
        let trimmed = model.trim();
        if !trimmed.is_empty() {
            self.model_by_kind.push(ModelBinding {
                kind,
                model: trimmed.to_owned(),
            });
        }
    }

    /// 划词手势（文本取材）实际使用的任务类型：`default_text_kind` 若被手改
    /// 成图像 kind，回退出厂默认 `TranslateWord`。
    ///
    /// 划词路径只取得到文本，图像 kind 到引擎必被模态校验拒（`TaskFailed`），
    /// 用户看到的会是与病因无关的提示——配置格式合法不代表组合可用，这里
    /// 按取材源收口。图像取材的框选路径落地时由它消费
    /// `hotkey_bindings` 里的 `InputSource::Region` 绑定，不受本方法影响。
    pub fn selection_task_kind(&self) -> TaskKind {
        if self.default_text_kind.accepts_text() {
            self.default_text_kind
        } else {
            TaskKind::TranslateWord
        }
    }

    /// 查某任务类型的默认模型 id。同 kind 多条时**后条覆盖前条**（设置页
    /// 保存按追加语义更新）；未配置返回 `None`，兜底策略由调用方决定。
    pub fn model_for_kind(&self, kind: TaskKind) -> Option<&str> {
        self.model_by_kind
            .iter()
            .rev()
            .find(|binding| binding.kind == kind)
            .map(|binding| binding.model.as_str())
    }

    /// 本任务实际使用的模型 id：`model_by_kind` 配了就用它，否则文本类任务
    /// 退回出厂默认 [`DEFAULT_TEXT_MODEL`]；图像类未配置时返回 `None`——
    /// 视觉模型随图像任务接入，不猜一个文本模型去接图像任务。
    pub fn resolved_model(&self, kind: TaskKind) -> Option<&str> {
        self.model_for_kind(kind)
            .or_else(|| kind.accepts_text().then_some(DEFAULT_TEXT_MODEL))
    }

    /// 本任务使用的 provider 条目：MVP 单端点，取最后一条（与
    /// `keychain_id_for` 的「后条覆盖前条」一致）；`provider_keys` 为空时返回
    /// `None`（纯查表，与 `model_for_kind` 对称）。
    pub fn active_provider(&self) -> Option<&ProviderKey> {
        self.provider_keys.last()
    }

    /// 引擎实际使用的 provider 条目：空 `provider_keys` 时回退出厂条目（只带
    /// keychain 条目标识，不带密钥），因此**恒有值**——真正的失败面是「条目指
    /// 向的密钥没设」（`EngineAuth`，UI 引导去设置页）。
    ///
    /// 兜底不只是「方便」：旧版本落盘的出厂值是空数组，而 `#[serde(default)]`
    /// 只补缺失字段——那批机器上落盘的 `provider_keys = []` 会一直留着，设置页
    /// 落地前又没有改它的 UI；`resolved_model` 对同一类问题已有对称兜底。
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
    use std::time::Duration;

    use super::*;

    #[test]
    fn factory_defaults_match_spec() {
        let config = Config::default();
        assert_eq!(config.target_lang, Lang::Zh);
        assert_eq!(config.default_text_kind, TaskKind::TranslateWord);
        assert_eq!(config.cache_ttl_secs, 60 * 60);
        assert_eq!(config.theme, Theme::System);
        assert_eq!(config.language, Language::System);
        assert_eq!(config.base_url, DEFAULT_BASE_URL);

        assert_eq!(
            config.resolved_model(TaskKind::TranslateWord),
            Some(DEFAULT_TEXT_MODEL)
        );
        assert_eq!(
            config.resolved_model(TaskKind::TranslateSentence),
            Some(DEFAULT_TEXT_MODEL)
        );
        assert_eq!(
            config.resolved_model(TaskKind::ExplainCode),
            Some(DEFAULT_TEXT_MODEL)
        );
        assert_eq!(config.resolved_model(TaskKind::ImageOcr), None);

        let provider = config.active_provider().expect("factory provider");
        assert_eq!(provider.provider, "deepseek");
        assert_eq!(provider.keychain_id, "gloss/deepseek");

        let triggers: Vec<_> = config
            .hotkey_bindings
            .iter()
            .map(|b| (b.trigger.as_str(), b.kind, b.source))
            .collect();
        assert_eq!(
            triggers,
            vec![
                (
                    "Cmd+Shift+D",
                    TaskKind::TranslateWord,
                    InputSource::Selection
                ),
                (
                    "Cmd+Shift+F",
                    TaskKind::TranslateSentence,
                    InputSource::Selection
                ),
                ("Cmd+Shift+E", TaskKind::ExplainCode, InputSource::Selection),
            ]
        );
    }

    #[test]
    fn missing_fields_fall_back_to_defaults_on_deserialize() {
        let json = serde_json::json!({
            "base_url": DEFAULT_BASE_URL,
            "provider_keys": [],
            "model_by_kind": [],
            "target_lang": "Zh",
            "hotkey_bindings": [],
            "default_text_kind": "TranslateWord",
            "enabled_kinds": [],
            "cache_ttl_secs": 0,
            "theme": "System",
        });
        let config: Config =
            serde_json::from_value(json).expect("missing language must fall back to default");
        assert_eq!(config.language, Language::System);
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
    fn config_round_trips_through_serde() {
        let config = Config {
            base_url: "https://example.test/v1".into(),
            provider_keys: vec![ProviderKey {
                provider: "deepseek".into(),
                keychain_id: "gloss/deepseek".into(),
            }],
            model_by_kind: vec![
                ModelBinding {
                    kind: TaskKind::TranslateWord,
                    model: "deepseek-chat".into(),
                },
                ModelBinding {
                    kind: TaskKind::ImageOcr,
                    model: "vision-model".into(),
                },
            ],
            target_lang: Lang::Other("ko".into()),
            hotkey_bindings: vec![HotkeyBinding {
                trigger: "Cmd+Shift+R".into(),
                kind: TaskKind::ImageExplain,
                source: InputSource::Region,
            }],
            default_text_kind: TaskKind::ExplainCode,
            enabled_kinds: vec![TaskKind::TranslateWord, TaskKind::ExplainCode],
            cache_ttl_secs: 120,
            theme: Theme::Dark,
            language: Language::En,
        };
        let json = serde_json::to_string(&config).expect("config should serialize");
        let back: Config = serde_json::from_str(&json).expect("config should deserialize");
        assert_eq!(back, config);
    }

    #[test]
    fn partial_document_fills_factory_defaults() {
        let config: Config =
            serde_json::from_str(r#"{"theme": "Light"}"#).expect("partial config should parse");
        assert_eq!(config.theme, Theme::Light, "present field must be kept");
        assert_eq!(config.target_lang, Lang::Zh, "missing field must default");
        assert_eq!(config.cache_ttl_secs, 60 * 60);
        assert_eq!(config.hotkey_bindings.len(), 3);
        assert_eq!(config.base_url, DEFAULT_BASE_URL);
        assert_eq!(config.resolved_provider().provider, "deepseek");
        assert_eq!(
            config.resolved_model(TaskKind::TranslateWord),
            Some(DEFAULT_TEXT_MODEL)
        );
        assert!(ALL_KINDS.iter().all(|&kind| config.is_kind_enabled(kind)));
    }

    #[test]
    fn explicit_empty_enabled_kinds_disables_everything() {
        let config: Config =
            serde_json::from_str(r#"{"enabled_kinds": []}"#).expect("explicit empty should parse");
        assert!(
            ALL_KINDS.iter().all(|&kind| !config.is_kind_enabled(kind)),
            "explicit empty table must disable every kind"
        );
    }

    #[test]
    fn missing_fields_default_while_explicit_empty_stays_empty() {
        let cleared: Config = serde_json::from_str(r#"{"provider_keys": [], "model_by_kind": []}"#)
            .expect("explicit empty config should parse");
        assert!(
            cleared.active_provider().is_none(),
            "explicit empty must stay empty for pure lookups"
        );
        assert_eq!(
            cleared.resolved_provider().keychain_id,
            "gloss/deepseek",
            "resolved lookup must fall back to the factory entry"
        );
        assert_eq!(
            cleared.resolved_model(TaskKind::TranslateWord),
            Some(DEFAULT_TEXT_MODEL)
        );
        assert_eq!(
            cleared.resolved_model(TaskKind::ImageOcr),
            None,
            "image kinds must not borrow the text model"
        );
    }

    #[test]
    fn selection_kind_falls_back_for_image_kinds() {
        let misconfigured = Config {
            default_text_kind: TaskKind::ImageOcr,
            ..Default::default()
        };
        assert_eq!(
            misconfigured.selection_task_kind(),
            TaskKind::TranslateWord,
            "image kind cannot be served by the selection gesture"
        );

        let text_config = Config {
            default_text_kind: TaskKind::ExplainCode,
            ..Default::default()
        };
        assert_eq!(text_config.selection_task_kind(), TaskKind::ExplainCode);
    }

    #[test]
    fn default_cache_ttl_matches_cache_implementation() {
        assert_eq!(
            Duration::from_secs(Config::default().cache_ttl_secs),
            crate::cache::DEFAULT_TTL
        );
    }

    #[test]
    fn edit_helpers_keep_tables_canonical() {
        let mut config = Config::default();
        assert!(config.is_kind_enabled(TaskKind::TranslateWord));

        config.set_kind_enabled(TaskKind::TranslateWord, true);
        assert_eq!(
            config
                .enabled_kinds
                .iter()
                .filter(|&&k| k == TaskKind::TranslateWord)
                .count(),
            1,
            "enabling an enabled kind must not duplicate the entry"
        );

        config.set_kind_enabled(TaskKind::TranslateWord, false);
        assert!(!config.is_kind_enabled(TaskKind::TranslateWord));
        config.set_kind_enabled(TaskKind::TranslateWord, false);
        assert_eq!(
            config
                .enabled_kinds
                .iter()
                .filter(|&&k| k == TaskKind::TranslateWord)
                .count(),
            0,
            "disabling twice stays empty"
        );

        config.set_model_for_kind(TaskKind::ImageOcr, "  vision-x  ");
        assert_eq!(config.model_for_kind(TaskKind::ImageOcr), Some("vision-x"));
        config.set_model_for_kind(TaskKind::ImageOcr, "vision-y");
        assert_eq!(
            config.model_for_kind(TaskKind::ImageOcr),
            Some("vision-y"),
            "re-setting must replace, not append"
        );
        config.set_model_for_kind(TaskKind::ImageOcr, "   ");
        assert_eq!(
            config.model_for_kind(TaskKind::ImageOcr),
            None,
            "blank model id means unconfigured"
        );
    }

    #[test]
    fn lookups_prefer_later_entries() {
        let config = Config {
            model_by_kind: vec![
                ModelBinding {
                    kind: TaskKind::TranslateWord,
                    model: "old-model".into(),
                },
                ModelBinding {
                    kind: TaskKind::TranslateWord,
                    model: "new-model".into(),
                },
                ModelBinding {
                    kind: TaskKind::ExplainCode,
                    model: "code-model".into(),
                },
            ],
            provider_keys: vec![ProviderKey {
                provider: "deepseek".into(),
                keychain_id: "gloss/deepseek".into(),
            }],
            ..Default::default()
        };

        assert_eq!(
            config.model_for_kind(TaskKind::TranslateWord),
            Some("new-model")
        );
        assert_eq!(
            config.model_for_kind(TaskKind::ExplainCode),
            Some("code-model")
        );
        assert_eq!(config.model_for_kind(TaskKind::ImageOcr), None);
        assert_eq!(config.keychain_id_for("deepseek"), Some("gloss/deepseek"));
        assert_eq!(config.keychain_id_for("openai"), None);
    }
}
