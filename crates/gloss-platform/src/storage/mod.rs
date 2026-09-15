//! ConfigStore 适配器：配置文档落 TOML 文件；密钥走系统 keychain，由
//! 独立适配器实现并与这里组合成完整 `ConfigStore`（组合前本模块的密钥
//! 方法返回错误，见 [`FileConfigStore`]）。

use std::fs;
use std::path::{Path, PathBuf};

use gloss_core::config::Config;
use gloss_core::log::{info, warn};
use gloss_core::model::GlossError;
use gloss_core::ports::ConfigStore;

/// 配置目录下的文件名。
const CONFIG_FILE: &str = "config.toml";

/// [`ConfigStore`] 的文件实现：TOML 文档 + 原子写。
///
/// 路径定位：系统标准配置目录下的 `gloss/config.toml`（macOS
/// `~/Library/Application Support/gloss`，Windows `%APPDATA%\gloss`，
/// Linux `$XDG_CONFIG_HOME/gloss`，由 directories crate 决定）。
///
/// 密钥方法返回 [`GlossError::Config`]（keychain 适配器接入前无实现，
/// 与文档方法组合才是完整端口），调用方按错误降级，不 panic。
#[derive(Debug)]
pub struct FileConfigStore {
    path: PathBuf,
}

impl FileConfigStore {
    /// 用系统标准配置目录定位配置文件。
    pub fn new() -> Result<Self, GlossError> {
        let dirs = directories::BaseDirs::new().ok_or_else(|| {
            GlossError::Config("cannot determine the user config directory".into())
        })?;
        Ok(Self::in_dir(dirs.config_dir().join("gloss")))
    }

    /// 在指定目录下定位配置文件（测试与自定义位置用）。
    pub fn in_dir(dir: PathBuf) -> Self {
        Self {
            path: dir.join(CONFIG_FILE),
        }
    }

    /// 配置文件完整路径（诊断与「打开配置」入口用）。
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 原子写：先写同目录临时文件再 rename，断电/崩溃不会留下半份配置。
    /// tmp 名带进程号：并发 save 时各写各的临时文件，rename 不会搬走别
    /// 人的内容；不 fsync——配置可由缺文件路径重建，断电最多回上一版。
    fn write_atomic(&self, config: &Config) -> Result<(), GlossError> {
        let text = toml::to_string_pretty(config)
            .map_err(|e| GlossError::Config(format!("serialize config: {e}")))?;
        let parent = self.path.parent().ok_or_else(|| {
            GlossError::Config(format!("config path {} has no parent", self.path.display()))
        })?;
        fs::create_dir_all(parent)
            .map_err(|e| GlossError::Config(format!("create {}: {e}", parent.display())))?;
        let tmp = parent.join(format!("{CONFIG_FILE}.{}.tmp", std::process::id()));
        fs::write(&tmp, text)
            .map_err(|e| GlossError::Config(format!("write {}: {e}", tmp.display())))?;
        if let Err(err) = fs::rename(&tmp, &self.path) {
            // 清理是尽力而为：残留的 tmp 只占一个文件名，下次写入会截断
            // 覆盖，不值得为它放大错误。
            let _ = fs::remove_file(&tmp);
            return Err(GlossError::Config(format!(
                "rename {} -> {}: {err}",
                tmp.display(),
                self.path.display()
            )));
        }
        Ok(())
    }
}

impl ConfigStore for FileConfigStore {
    fn load(&self) -> Result<Config, GlossError> {
        match fs::read_to_string(&self.path) {
            Ok(text) => toml::from_str(&text)
                .map_err(|e| GlossError::Config(format!("parse {}: {e}", self.path.display()))),
            // 缺文件是首次运行的正常路径：落一份出厂默认，用户可直接改文件。
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let config = Config::default();
                match self.write_atomic(&config) {
                    Ok(()) => info!("created default config at {}", self.path.display()),
                    // 首次落盘失败不阻断启动：降级为仅内存默认（06 §3.3）。
                    Err(err) => warn!(
                        "could not persist default config to {}: {err}",
                        self.path.display()
                    ),
                }
                Ok(config)
            }
            Err(err) => Err(GlossError::Config(format!(
                "read {}: {err}",
                self.path.display()
            ))),
        }
    }

    fn save(&self, config: &Config) -> Result<(), GlossError> {
        self.write_atomic(config)
    }

    fn secret(&self, _key: &str) -> Result<Option<String>, GlossError> {
        Err(GlossError::Config(
            "secrets live in the system keychain; adapter not wired yet".into(),
        ))
    }

    fn set_secret(&self, _key: &str, _value: &str) -> Result<(), GlossError> {
        Err(GlossError::Config(
            "secrets live in the system keychain; adapter not wired yet".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gloss_core::config::{ModelBinding, ProviderKey, Theme};
    use gloss_core::model::Lang;
    use gloss_core::task::{HotkeyBinding, InputSource, TaskKind};

    use super::*;

    /// 带齐全量字段差异的样例配置，用于锁定往返一致性。
    fn sample_config() -> Config {
        Config {
            provider_keys: vec![ProviderKey {
                provider: "deepseek".into(),
                keychain_id: "gloss/deepseek".into(),
            }],
            model_by_kind: vec![ModelBinding {
                kind: TaskKind::ImageOcr,
                model: "vision-model".into(),
            }],
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
        }
    }

    /// 验收标准：save → load 往返逐字段一致。
    #[test]
    fn save_then_load_round_trips_every_field() {
        let dir = tempfile::tempdir().expect("tempdir should create");
        let store = FileConfigStore::in_dir(dir.path().to_path_buf());
        let config = sample_config();

        store.save(&config).expect("save should succeed");
        assert_eq!(store.load().expect("load should succeed"), config);
    }

    /// 验收标准：缺文件时生成出厂默认——返回值与落盘文件内容都应是
    /// 完整默认配置（用户拿到的是可直接手改的文件）。
    #[test]
    fn load_generates_default_when_file_missing() {
        let dir = tempfile::tempdir().expect("tempdir should create");
        let store = FileConfigStore::in_dir(dir.path().to_path_buf());

        let config = store.load().expect("first load should succeed");
        assert_eq!(config, Config::default());
        assert!(store.path().is_file(), "default config must be persisted");

        let reread = store.load().expect("second load should succeed");
        assert_eq!(reread, Config::default(), "persisted file must re-parse");
    }

    /// load 缺文件时建的目录含父级：配置目录本身不存在也能落盘。
    #[test]
    fn load_creates_missing_directories() {
        let dir = tempfile::tempdir().expect("tempdir should create");
        let store = FileConfigStore::in_dir(dir.path().join("gloss").join("nested"));

        store.load().expect("load should create directories");
        assert!(store.path().is_file());
    }

    /// 损坏的配置文件是硬错误（read 端区别于缺文件的降级路径）。
    #[test]
    fn load_rejects_corrupt_file() {
        let dir = tempfile::tempdir().expect("tempdir should create");
        let store = FileConfigStore::in_dir(dir.path().to_path_buf());
        std::fs::write(store.path(), "not [ valid toml").expect("write should succeed");

        let err = store.load().expect_err("corrupt file must fail");
        assert!(matches!(err, GlossError::Config(_)), "got: {err:?}");
    }

    /// 密钥方法明确拒绝（返回可区分的 Config 错误）：keychain 适配器
    /// 接入前调用方应降级而不是重试。
    #[test]
    fn secret_methods_defer_to_keychain_milestone() {
        let dir = tempfile::tempdir().expect("tempdir should create");
        let store = FileConfigStore::in_dir(dir.path().to_path_buf());

        assert!(matches!(
            store.secret("gloss/deepseek"),
            Err(GlossError::Config(_))
        ));
        assert!(matches!(
            store.set_secret("gloss/deepseek", "sk-x"),
            Err(GlossError::Config(_))
        ));
    }

    /// save 写出的 TOML 不含密钥本体字段：provider_keys 只有条目标识。
    #[test]
    fn persisted_document_stores_no_secret_material() {
        let dir = tempfile::tempdir().expect("tempdir should create");
        let store = FileConfigStore::in_dir(dir.path().to_path_buf());

        store.save(&sample_config()).expect("save should succeed");
        let text = std::fs::read_to_string(store.path()).expect("read should succeed");
        assert!(text.contains("keychain_id"), "identifier must be present");
        assert!(
            !text.contains("sk-"),
            "no secret material may appear in the document"
        );
    }

    /// Default 构造在拿到真实 Home 目录时定位到 gloss/config.toml
    /// （沙箱与 CI 都有 HOME，路径尾段断言与平台无关）。
    #[test]
    fn default_store_targets_standard_config_dir() {
        if directories::BaseDirs::new().is_none() {
            return;
        }
        let store = FileConfigStore::new().expect("new should succeed");
        let path = store.path();
        assert_eq!(
            path.file_name().map(|n| n.to_string_lossy()),
            Some("config.toml".into())
        );
        assert!(
            path.components()
                .any(|c| c.as_os_str() == std::ffi::OsStr::new("gloss")),
            "expected a gloss-scoped directory, got {path:?}"
        );
    }

    /// 样例配置的 TTL 字段与 cache 出厂语义的对照（120 秒 < 出厂 1 小时，
    /// 若未来改动 Config 字段语义，此处提醒同步 cache.rs）。
    #[test]
    fn default_cache_ttl_matches_core_cache_semantics() {
        let default = Config::default();
        assert_eq!(
            Duration::from_secs(default.cache_ttl_secs),
            Duration::from_secs(60 * 60)
        );
    }
}
