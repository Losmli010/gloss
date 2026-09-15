//! ConfigStore 适配器：文档半边（[`FileConfigStore`]，TOML 落盘）与
//! 密钥半边（[`keychain::KeychainSecret`]，系统安全存储）各司其职，由
//! [`CompositeConfigStore`] 组合成完整端口——组装点注入的就是组合体。

pub mod keychain;

use std::fs;
use std::path::{Path, PathBuf};

use gloss_core::config::Config;
use gloss_core::log::{info, warn};
use gloss_core::model::GlossError;
use gloss_core::ports::ConfigStore;

/// 配置目录下的文件名。
const CONFIG_FILE: &str = "config.toml";

/// 配置文档半边：TOML 文件 + 原子写，不接触密钥。
///
/// 路径定位：系统标准配置目录下的 `gloss/config.toml`（macOS
/// `~/Library/Application Support/gloss`，Windows `%APPDATA%\gloss`，
/// Linux `$XDG_CONFIG_HOME/gloss`，由 directories crate 决定）。
///
/// 不实现 [`ConfigStore`]：端口要求文档与密钥一起应答，单独把文档半边
/// 当端口用会让密钥方法凭空失败——组合体才是注入单元。
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

    /// 读取整份配置；缺文件是首次运行的正常路径——落一份出厂默认（用
    /// 户拿到可直接手改的文件）并返回默认值，首次落盘失败降级为仅内存
    /// 默认、不阻断启动（06 §3.3）。
    pub fn load(&self) -> Result<Config, GlossError> {
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

    /// 原子写入整份配置（设置页保存路径）。
    pub fn save(&self, config: &Config) -> Result<(), GlossError> {
        self.write_atomic(config)
    }
}

/// 密钥半边契约：组合体对密钥存储的最小要求。公开仅为泛型签名可见
/// （组合体是公开类型），组装仍走本模块的构造子；外部实现不被支持——
/// 密钥语义（服务名、条目定位）由本 crate 掌控。macOS 出厂实现是
/// [`keychain::KeychainSecret`]，测试用内存桩（转发路径全平台可测，
/// 不碰真机 keychain）。
pub trait SecretsHalf: Send + Sync {
    /// 读密钥；`None` 表示未设置（语义同 [`ConfigStore::secret`]）。
    fn get(&self, key: &str) -> Result<Option<String>, GlossError>;
    /// 写入（或覆盖）密钥。
    fn set(&self, key: &str, value: &str) -> Result<(), GlossError>;
    /// 删除密钥；条目不存在视为成功（幂等）。
    fn delete(&self, key: &str) -> Result<(), GlossError>;
}

impl SecretsHalf for keychain::KeychainSecret {
    fn get(&self, key: &str) -> Result<Option<String>, GlossError> {
        Self::get(self, key)
    }

    fn set(&self, key: &str, value: &str) -> Result<(), GlossError> {
        Self::set(self, key, value)
    }

    fn delete(&self, key: &str) -> Result<(), GlossError> {
        Self::delete(self, key)
    }
}

/// [`ConfigStore`] 的组合实现：文档方法委托 [`FileConfigStore`]，密钥
/// 方法委托密钥半边（泛型参数，出厂即 [`keychain::KeychainSecret`]）。
/// 核心编排与组装点只见端口，不感知两半边的存在（06 §5.2）。
#[derive(Debug)]
pub struct CompositeConfigStore<S = keychain::KeychainSecret> {
    document: FileConfigStore,
    secrets: S,
}

impl CompositeConfigStore<keychain::KeychainSecret> {
    /// 用系统标准位置构造（配置文件在标准目录，密钥在出厂服务名下）。
    pub fn new() -> Result<Self, GlossError> {
        Ok(Self {
            document: FileConfigStore::new()?,
            secrets: keychain::KeychainSecret::new(),
        })
    }

    /// 在指定配置目录下构造（测试与自定义位置用）。
    pub fn in_dir(dir: PathBuf) -> Self {
        Self {
            document: FileConfigStore::in_dir(dir),
            secrets: keychain::KeychainSecret::new(),
        }
    }
}

impl<S: SecretsHalf> CompositeConfigStore<S> {
    /// 用指定密钥半边构造（测试注入隔离服务名或内存桩用）。
    pub fn with_secrets(document: FileConfigStore, secrets: S) -> Self {
        Self { document, secrets }
    }
}

impl<S: SecretsHalf> ConfigStore for CompositeConfigStore<S> {
    fn load(&self) -> Result<Config, GlossError> {
        self.document.load()
    }

    fn save(&self, config: &Config) -> Result<(), GlossError> {
        self.document.save(config)
    }

    fn secret(&self, key: &str) -> Result<Option<String>, GlossError> {
        self.secrets.get(key)
    }

    fn set_secret(&self, key: &str, value: &str) -> Result<(), GlossError> {
        self.secrets.set(key, value)
    }

    fn delete_secret(&self, key: &str) -> Result<(), GlossError> {
        self.secrets.delete(key)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;
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

    /// 组合体把文档方法完整委托给 FileConfigStore：save → load 往返
    /// 逐字段一致（密钥半边在这条路径上零参与，全平台可跑）。
    #[test]
    fn composite_delegates_document_methods() {
        let dir = tempfile::tempdir().expect("tempdir should create");
        let store = CompositeConfigStore::in_dir(dir.path().to_path_buf());
        let config = sample_config();

        store.save(&config).expect("save should succeed");
        assert_eq!(store.load().expect("load should succeed"), config);
    }

    /// stub 平台（Linux CI）上组合体的密钥方法明确失败：错误可区分，
    /// 调用方按错误降级而不是把「存储不可用」当「未设置」。
    #[test]
    #[cfg(not(target_os = "macos"))]
    fn composite_secret_methods_report_unsupported_platform() {
        let dir = tempfile::tempdir().expect("tempdir should create");
        let store = CompositeConfigStore::in_dir(dir.path().to_path_buf());

        assert!(matches!(
            ConfigStore::secret(&store, "gloss/deepseek"),
            Err(GlossError::Config(_))
        ));
        assert!(matches!(
            ConfigStore::set_secret(&store, "gloss/deepseek", "sk-x"),
            Err(GlossError::Config(_))
        ));
    }

    /// 密钥转发路径用内存桩全平台验证（不碰真机 keychain）：端口三方法
    /// 都完整到达密钥半边，删除后读回是 `None`。
    #[test]
    fn composite_forwards_secret_methods_to_half() {
        #[derive(Default)]
        struct MemorySecrets(Mutex<HashMap<String, String>>);

        impl SecretsHalf for MemorySecrets {
            fn get(&self, key: &str) -> Result<Option<String>, GlossError> {
                Ok(self.0.lock().expect("poisoned").get(key).cloned())
            }

            fn set(&self, key: &str, value: &str) -> Result<(), GlossError> {
                self.0
                    .lock()
                    .expect("poisoned")
                    .insert(key.to_owned(), value.to_owned());
                Ok(())
            }

            fn delete(&self, key: &str) -> Result<(), GlossError> {
                self.0.lock().expect("poisoned").remove(key);
                Ok(())
            }
        }

        let dir = tempfile::tempdir().expect("tempdir should create");
        let store = CompositeConfigStore::with_secrets(
            FileConfigStore::in_dir(dir.path().to_path_buf()),
            MemorySecrets::default(),
        );

        assert_eq!(
            ConfigStore::secret(&store, "gloss/deepseek").expect("read should succeed"),
            None
        );
        ConfigStore::set_secret(&store, "gloss/deepseek", "sk-test").expect("set should succeed");
        assert_eq!(
            ConfigStore::secret(&store, "gloss/deepseek").expect("read should succeed"),
            Some("sk-test".into())
        );
        ConfigStore::delete_secret(&store, "gloss/deepseek").expect("delete should succeed");
        assert_eq!(
            ConfigStore::secret(&store, "gloss/deepseek").expect("read should succeed"),
            None
        );
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
