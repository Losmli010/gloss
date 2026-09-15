//! 配置模型：`Config` 结构、出厂默认与查找助手（06 §6.3）。
//!
//! 与 06 §6.3 草案的偏差（字段一一对应，仅类型收窄）：
//! - `provider_keys` / `model_by_kind` 由元组数组改为命名字段结构——
//!   配置文件面向用户手改，TOML 的 `[[provider_keys]]` 段落比二元数组可读；
//! - `target_lang` 由 `String` 收窄为 [`Lang`]——与 `TaskOptions::target_lang`
//!   同型，避免运行时二次解析。
//!
//! 反序列化不带 `deny_unknown_fields`：未知键被静默忽略，换取配置文件的
//! 向前兼容；手改配置时注意键名拼写。
//!
//! 密钥红线（06 ADR）：配置里只存 keychain 条目标识，密钥本体永不进
//! `Config`——运行时整份快照可被任意线程读取，不能携带凭据。

use serde::{Deserialize, Serialize};

use crate::model::Lang;
use crate::task::{HotkeyBinding, InputSource, TaskKind};

/// 出厂默认热键表：与 `gloss-platform::events::hotkey` 的写死默认一致。
/// 有意避开 macOS 截图（Cmd+Shift+3/4/5）等系统级组合。
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

/// 一个 provider 的密钥条目：`keychain_id` 是密钥在系统 keychain 里的
/// 条目标识，读取走 `ConfigStore::secret`，配置文件中永不出现密钥本体。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderKey {
    /// provider 名称，如 "deepseek"。
    pub provider: String,
    /// keychain 条目标识。
    pub keychain_id: String,
}

/// 任务类型 → 默认模型 id 的绑定：统一 LLM 客户端下，模态能力差异是
/// 配置问题（06 §5.1）——文本任务配文本模型、图像任务配视觉模型。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelBinding {
    /// 任务类型。
    pub kind: TaskKind,
    /// 该任务默认使用的模型 id。
    pub model: String,
}

/// 应用配置：持久化为 TOML（`FileConfigStore`），运行时以整份快照在
/// 各线程间共享（共享形态不携带密钥，见模块文档红线）。反序列化带 `#[serde(default)]`：
/// 手改配置缺字段时按出厂默认补齐，不允许半份配置带病运行。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
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
    /// 取材成功后是否自动弹出浮层。
    pub auto_show: bool,
    /// 缓存条目存活时长（秒）。
    pub cache_ttl_secs: u64,
    /// 界面主题。
    pub theme: Theme,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider_keys: Vec::new(),
            model_by_kind: Vec::new(),
            target_lang: Lang::Zh,
            hotkey_bindings: default_hotkey_bindings(),
            default_text_kind: TaskKind::TranslateWord,
            auto_show: true,
            // 与 gloss-core::cache 的出厂 TTL（1 小时）一致。
            cache_ttl_secs: 60 * 60,
            theme: Theme::System,
        }
    }
}

impl Config {
    /// 查某任务类型的默认模型 id。同 kind 多条时**后条覆盖前条**（设置页
    /// 保存按追加语义更新）；未配置返回 `None`，兜底策略由调用方决定。
    pub fn model_for_kind(&self, kind: TaskKind) -> Option<&str> {
        self.model_by_kind
            .iter()
            .rev()
            .find(|binding| binding.kind == kind)
            .map(|binding| binding.model.as_str())
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

    /// 出厂默认值契约：与 06 §6.3 及热键写死默认对齐的抽查。
    #[test]
    fn factory_defaults_match_spec() {
        let config = Config::default();
        assert_eq!(config.target_lang, Lang::Zh);
        assert_eq!(config.default_text_kind, TaskKind::TranslateWord);
        assert!(config.auto_show);
        assert_eq!(config.cache_ttl_secs, 60 * 60);
        assert_eq!(config.theme, Theme::System);
        assert!(config.provider_keys.is_empty());
        assert!(config.model_by_kind.is_empty());

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

    /// 完整自定义配置经 serde 往返无损（含 `Lang::Other` 携载数据的
    /// 变体）；TOML 形态的往返由 platform 侧 storage 测试覆盖。
    #[test]
    fn config_round_trips_through_serde() {
        let config = Config {
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
            auto_show: false,
            cache_ttl_secs: 120,
            theme: Theme::Dark,
        };
        let json = serde_json::to_string(&config).expect("config should serialize");
        let back: Config = serde_json::from_str(&json).expect("config should deserialize");
        assert_eq!(back, config);
    }

    /// 验收标准：手改配置缺字段时按出厂默认补齐，不允许半份配置。
    #[test]
    fn partial_document_fills_factory_defaults() {
        let config: Config =
            serde_json::from_str(r#"{"theme": "Light"}"#).expect("partial config should parse");
        assert_eq!(config.theme, Theme::Light, "present field must be kept");
        assert_eq!(config.target_lang, Lang::Zh, "missing field must default");
        assert_eq!(config.cache_ttl_secs, 60 * 60);
        assert_eq!(config.hotkey_bindings.len(), 3);
    }

    /// 查找助手：后条覆盖前条；未配置返回 None。
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
