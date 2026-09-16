//! ConfigStore 适配器：文档半边（[`FileConfigStore`]，TOML 落盘）与
//! 密钥半边（[`keychain::KeychainSecret`]，系统安全存储）各司其职，由
//! [`CompositeConfigStore`] 组合成完整端口——组装点注入的就是组合体。

pub mod keychain;

use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use gloss_core::config::Config;
use gloss_core::log::{info, warn};
use gloss_core::model::GlossError;
use gloss_core::ports::ConfigStore;

/// 配置目录下的文件名。
const CONFIG_FILE: &str = "config.toml";

/// 临时文件序号：只用进程号不够——同进程多线程并发写会用同一个 tmp 名互相
/// 截断，加上单调序号后每次写入各用各的名字。
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// 取下一个临时文件序号。
fn tmp_sequence() -> u64 {
    TMP_SEQ.fetch_add(1, Ordering::Relaxed)
}

/// 解析错误的行号（1 基）。只取位置、不回传解析器文本（端口红线，见
/// [`FileConfigStore::load`]）：span 缺失（部分数据错误）返回 `None`；span
/// 落在字符中间时向前收敛——宁可报一个偏早的行号，也不让读配置的路径 panic。
fn error_line(text: &str, span: Option<Range<usize>>) -> Option<usize> {
    let span = span?;
    let mut end = span.start.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    Some(text[..end].matches('\n').count() + 1)
}

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
    /// tmp 名带进程号 + 序号：并发 save（含同进程多线程）各写各的临时文件，
    /// 不会互相截断、rename 也不会搬走别人的内容；不 fsync——配置可由缺文件
    /// 路径重建，断电最多回上一版。
    fn write_atomic(&self, config: &Config) -> Result<(), GlossError> {
        let text = toml::to_string_pretty(config)
            .map_err(|e| GlossError::Config(format!("serialize config: {e}")))?;
        let parent = self.path.parent().ok_or_else(|| {
            GlossError::Config(format!("config path {} has no parent", self.path.display()))
        })?;
        fs::create_dir_all(parent)
            .map_err(|e| GlossError::Config(format!("create {}: {e}", parent.display())))?;
        let tmp = parent.join(format!(
            "{CONFIG_FILE}.{}.{}.tmp",
            std::process::id(),
            tmp_sequence()
        ));
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

    /// 把读不出来的配置挪到一边（`config.toml.<秒级时间戳>.<序号>.bak`），
    /// 返回备份路径。名字唯一，不覆盖既有备份，也不删原内容——用户手改的
    /// 配置里可能有他真正想保留的部分，而下次保存会把 `config.toml` 整份
    /// 覆盖掉。
    fn quarantine(&self) -> Result<PathBuf, std::io::Error> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since_epoch| since_epoch.as_secs());
        let backup = self
            .path
            .with_extension(format!("toml.{stamp}.{}.bak", tmp_sequence()));
        fs::rename(&self.path, &backup)?;
        Ok(backup)
    }

    /// 读取整份配置；缺文件是首次运行的正常路径——落一份出厂默认（用
    /// 户拿到可直接手改的文件）并返回默认值，首次落盘失败降级为仅内存
    /// 默认、不阻断启动（06 §3.3）。
    ///
    /// 解析失败是硬错误：坏文件先被隔离备份，错误里只给路径与行号，不转述
    /// 解析器文本（它常引用出错行/取值，而用户可能把密钥贴错字段——端口
    /// 红线）。
    pub fn load(&self) -> Result<Config, GlossError> {
        match fs::read_to_string(&self.path) {
            Ok(text) => match toml::from_str(&text) {
                Ok(config) => Ok(config),
                Err(err) => {
                    let location = match error_line(&text, err.span()) {
                        Some(line) => format!("line {line}"),
                        None => "line unknown".to_owned(),
                    };
                    // 隔离失败只记 warn：读不出来才是主要错误，不该被它顶掉。
                    match self.quarantine() {
                        Ok(backup) => Err(GlossError::Config(format!(
                            "config file {} is not valid toml ({location}); original kept at {}",
                            self.path.display(),
                            backup.display()
                        ))),
                        Err(io) => {
                            warn!(
                                "could not move aside unreadable config {}: {io}",
                                self.path.display()
                            );
                            Err(GlossError::Config(format!(
                                "config file {} is not valid toml ({location})",
                                self.path.display()
                            )))
                        }
                    }
                }
            },
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
            base_url: "https://example.test/v1".into(),
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
            enabled_kinds: vec![TaskKind::ImageOcr, TaskKind::ImageExplain],
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

    /// 损坏的配置文件是硬错误（read 端区别于缺文件的降级路径），并且要留下
    /// 证据：原文件被挪到 `.bak`（字节原样），而不是等着被下次保存覆盖掉。
    /// 错误文本只给路径与行号，不转述解析器内容——用户可能把密钥贴错字段，
    /// 而这条错误会进日志（端口红线）。
    #[test]
    fn load_rejects_corrupt_file_and_quarantines_it() {
        let dir = tempfile::tempdir().expect("tempdir should create");
        let store = FileConfigStore::in_dir(dir.path().to_path_buf());
        // 合法 TOML、非法取值：解析器文本会引用出错取值（这里就是「密钥」）。
        let broken = "target_lang = \"sk-secret-123\"\n";
        std::fs::write(store.path(), broken).expect("write should succeed");

        let err = store.load().expect_err("corrupt file must fail");
        assert!(matches!(err, GlossError::Config(_)), "got: {err:?}");
        assert!(
            !err.to_string().contains("sk-secret-123"),
            "error text must not quote the config content: {err}"
        );
        assert!(
            !store.path().is_file(),
            "broken document must be moved aside"
        );

        let backups = backups_in(dir.path());
        assert_eq!(backups.len(), 1, "exactly one backup expected: {backups:?}");
        assert_eq!(
            std::fs::read_to_string(&backups[0]).expect("backup read should succeed"),
            broken,
            "backup must keep the original bytes"
        );
    }

    /// 老版本（M4-T3 时代）落盘的配置：显式空数组、没有 base_url。读进来必须
    /// 拿到出厂端点与出厂 provider 条目——否则升级上来的用户每个任务都报
    /// 「no provider configured」，而设置页（M4-T6）之前没有改它的入口。
    #[test]
    fn legacy_document_gets_factory_endpoint_and_provider() {
        let dir = tempfile::tempdir().expect("tempdir should create");
        let store = FileConfigStore::in_dir(dir.path().to_path_buf());
        std::fs::write(store.path(), "provider_keys = []\nmodel_by_kind = []\n")
            .expect("write should succeed");

        let config = store.load().expect("legacy document must load");
        assert_eq!(config.base_url, gloss_core::config::DEFAULT_BASE_URL);
        assert!(config.active_provider().is_none());
        assert_eq!(config.resolved_provider().keychain_id, "gloss/deepseek");
        assert_eq!(
            config.resolved_model(gloss_core::task::TaskKind::TranslateWord),
            Some(gloss_core::config::DEFAULT_TEXT_MODEL)
        );
    }

    /// 隔离之后能自愈：下一次 load 走缺文件路径，落一份可用的出厂默认。
    #[test]
    fn load_recovers_after_quarantine() {
        let dir = tempfile::tempdir().expect("tempdir should create");
        let store = FileConfigStore::in_dir(dir.path().to_path_buf());
        std::fs::write(store.path(), "not [ valid toml").expect("write should succeed");
        store.load().expect_err("corrupt file must fail");

        let config = store.load().expect("second load should recover");
        assert_eq!(config, Config::default());
        assert!(store.path().is_file(), "fresh default must be persisted");
    }

    /// 同进程并发写不互踩：并发 save 全部成功，落盘文件仍是完整可解析的一份
    /// 配置（tmp 名带序号后各写各的，rename 不会搬走别人的临时文件）。
    #[test]
    fn concurrent_saves_keep_the_document_parseable() {
        let dir = tempfile::tempdir().expect("tempdir should create");
        let store = std::sync::Arc::new(FileConfigStore::in_dir(dir.path().to_path_buf()));

        let writers: Vec<_> = [Theme::Light, Theme::Dark, Theme::System, Theme::Light]
            .into_iter()
            .map(|theme| {
                let store = std::sync::Arc::clone(&store);
                std::thread::spawn(move || {
                    let config = Config {
                        theme,
                        ..Default::default()
                    };
                    for _ in 0..20 {
                        store.save(&config).expect("concurrent save should succeed");
                    }
                })
            })
            .collect();
        for writer in writers {
            writer.join().expect("writer thread should not panic");
        }

        let reread = store.load().expect("document must stay parseable");
        assert!(
            matches!(reread.theme, Theme::Light | Theme::Dark | Theme::System),
            "document must hold one complete version, got {reread:?}"
        );
    }

    /// 目录里的隔离备份（`config.toml.<秒级时间戳>.<序号>.bak`），按名排序。
    fn backups_in(dir: &Path) -> Vec<PathBuf> {
        let mut found: Vec<_> = std::fs::read_dir(dir)
            .expect("read_dir should succeed")
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.to_string_lossy().ends_with(".bak"))
            .collect();
        found.sort();
        found
    }

    /// 行号定位：解析错误落在哪一行就报哪一行，span 缺失时返回 None。
    #[test]
    fn error_line_reports_the_offending_line() {
        let text = "theme = \"Light\"\ntarget_lang = 7\n";
        let offset = text.find('7').expect("fixture must contain the value");
        assert_eq!(error_line(text, Some(offset..offset + 1)), Some(2));
        assert_eq!(error_line(text, None), None);
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
