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

/// 出厂默认 OpenAI 兼容端点：DeepSeek（06 §八 的参考实现）。客户端按
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
/// 配置问题（06 §5.1）——文本任务配文本模型、图像任务配视觉模型。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelBinding {
    /// 任务类型。
    pub kind: TaskKind,
    /// 该任务默认使用的模型 id。
    pub model: String,
}

/// 出厂默认模型表：文本类任务先给出可用默认（用户装完只差一把钥匙），
/// 图像类留空——视觉模型随 M5-T4 接入，未配置时由引擎报能力不匹配。
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
/// `default_text_kind` 已在 M4-T3 接线（触发时解析进任务）；`base_url` /
/// `provider_keys` 已在 M4-T4 接线（引擎每请求解析端点、按条目直查 keychain）；
/// `enabled_kinds` 已在 M4-T6 接线（触发时过滤）；`hotkey_bindings` /
/// `theme` / `auto_show` 已在 M4-T7 接线（保存后重注册热键；主题施加到两个
/// egui 上下文；auto_show 决定浮层何时自动露面）——这三项都**不**在触发时
/// 冻结，见 06 §6.3；`cache_ttl_secs` 归缓存构造接线。
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
            base_url: DEFAULT_BASE_URL.to_owned(),
            provider_keys: default_provider_keys(),
            model_by_kind: default_model_bindings(),
            target_lang: Lang::Zh,
            hotkey_bindings: default_hotkey_bindings(),
            default_text_kind: TaskKind::TranslateWord,
            enabled_kinds: ALL_KINDS.to_vec(),
            auto_show: true,
            // 与 gloss-core::cache 的出厂 TTL（1 小时）一致。
            cache_ttl_secs: 60 * 60,
            theme: Theme::System,
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
    /// 按取材源收口。图像取材是 M5-T3 的框选路径，届时由它消费
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
    /// 视觉模型随 M5-T4 接入，不猜一个文本模型去接图像任务。
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
    /// 兜底不只是「方便」：M4-T3 时代的出厂值是空数组，而 `#[serde(default)]`
    /// 只补缺失字段——那批机器上落盘的 `provider_keys = []` 会一直留着，设置页
    /// （M4-T6）之前又没有改它的 UI；`resolved_model` 对同一类问题已有对称兜底。
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

    /// 出厂默认值契约：与 06 §6.3 及热键写死默认对齐的抽查。
    #[test]
    fn factory_defaults_match_spec() {
        let config = Config::default();
        assert_eq!(config.target_lang, Lang::Zh);
        assert_eq!(config.default_text_kind, TaskKind::TranslateWord);
        assert!(config.auto_show);
        assert_eq!(config.cache_ttl_secs, 60 * 60);
        assert_eq!(config.theme, Theme::System);
        assert_eq!(config.base_url, DEFAULT_BASE_URL);

        // 文本任务装完即可用（只差 keychain 里那把钥匙）；图像任务留空，
        // 免得拿文本模型去接图像任务。
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

    /// 完整自定义配置经 serde 往返无损（含 `Lang::Other` 携载数据的
    /// 变体）；TOML 形态的往返由 platform 侧 storage 测试覆盖。
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
        // 端点与 provider 条目也是缺字段时的出厂值（M4-T4 新增）。
        assert_eq!(config.base_url, DEFAULT_BASE_URL);
        assert_eq!(config.resolved_provider().provider, "deepseek");
        assert_eq!(
            config.resolved_model(TaskKind::TranslateWord),
            Some(DEFAULT_TEXT_MODEL)
        );
        // 任务开关缺字段按全启用补齐（M4-T6；老配置没有这个键）。
        assert!(ALL_KINDS.iter().all(|&kind| config.is_kind_enabled(kind)));
    }

    /// 任务开关语义：显式空表 = 全部停用（与 `provider_keys` 的「显式空
    /// 保持为空」同一条 serde 规则的两面）。
    #[test]
    fn explicit_empty_enabled_kinds_disables_everything() {
        let config: Config =
            serde_json::from_str(r#"{"enabled_kinds": []}"#).expect("explicit empty should parse");
        assert!(
            ALL_KINDS.iter().all(|&kind| !config.is_kind_enabled(kind)),
            "explicit empty table must disable every kind"
        );
    }

    /// 两条语义要分清：**缺失**字段走出厂默认（老版本没写过的键），**显式空
    /// 数组**保持为空（M4-T3 时代落盘的 `provider_keys = []` 就是这样）——后者
    /// 由 `resolved_provider` / `resolved_model` 兜底，老用户不会卡在配置错误上。
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

    /// 划词路径的 kind 收口：配置里写成图像 kind（手改误配）时回退
    /// TranslateWord——划词只取得到文本，图像 kind 必被模态校验拒。
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

    /// 出厂 TTL 与缓存实现同源：一条断言把两处钉在一起（此前只有注释声称
    /// 一致，改一边不会有人红）。
    #[test]
    fn default_cache_ttl_matches_cache_implementation() {
        assert_eq!(
            Duration::from_secs(Config::default().cache_ttl_secs),
            crate::cache::DEFAULT_TTL
        );
    }

    /// 任务开关与模型表的编辑助手（设置页落点）：启用不重复追加、停用可
    /// 幂等移除；模型条目按 kind 整体重建，空白视为未配置。
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
