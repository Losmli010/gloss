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

const CONFIG_FILE: &str = "config.toml";

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

fn tmp_sequence() -> u64 {
    TMP_SEQ.fetch_add(1, Ordering::Relaxed)
}

fn error_line(text: &str, span: Option<Range<usize>>) -> Option<usize> {
    let span = span?;
    let mut end = span.start.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    Some(text[..end].matches('\n').count() + 1)
}

#[derive(Debug)]
pub struct FileConfigStore {
    path: PathBuf,
}

impl FileConfigStore {
    pub fn new() -> Result<Self, GlossError> {
        let dirs = directories::BaseDirs::new().ok_or_else(|| {
            GlossError::Config("cannot determine the user config directory".into())
        })?;
        Ok(Self::in_dir(dirs.config_dir().join("gloss")))
    }

    pub fn in_dir(dir: PathBuf) -> Self {
        Self {
            path: dir.join(CONFIG_FILE),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

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
            #[allow(clippy::let_underscore_must_use)]
            let _ = fs::remove_file(&tmp);
            return Err(GlossError::Config(format!(
                "rename {} -> {}: {err}",
                tmp.display(),
                self.path.display()
            )));
        }
        Ok(())
    }

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

    pub fn load(&self) -> Result<Config, GlossError> {
        match fs::read_to_string(&self.path) {
            Ok(text) => match toml::from_str(&text) {
                Ok(config) => Ok(config),
                Err(err) => {
                    let location = match error_line(&text, err.span()) {
                        Some(line) => format!("line {line}"),
                        None => "line unknown".to_owned(),
                    };
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
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let config = Config::default();
                match self.write_atomic(&config) {
                    Ok(()) => info!("created default config at {}", self.path.display()),
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

    pub fn save(&self, config: &Config) -> Result<(), GlossError> {
        self.write_atomic(config)
    }
}

pub trait SecretsHalf: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>, GlossError>;
    fn set(&self, key: &str, value: &str) -> Result<(), GlossError>;
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

#[derive(Debug)]
pub struct CompositeConfigStore<S = keychain::KeychainSecret> {
    document: FileConfigStore,
    secrets: S,
}

impl CompositeConfigStore<keychain::KeychainSecret> {
    pub fn new() -> Result<Self, GlossError> {
        Ok(Self {
            document: FileConfigStore::new()?,
            secrets: keychain::KeychainSecret::new(),
        })
    }

    pub fn in_dir(dir: PathBuf) -> Self {
        Self {
            document: FileConfigStore::in_dir(dir),
            secrets: keychain::KeychainSecret::new(),
        }
    }
}

impl<S: SecretsHalf> CompositeConfigStore<S> {
    #[cfg(test)]
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

    use gloss_core::config::{Language, ProviderKey, Theme};
    use gloss_core::model::Lang;

    use super::*;

    fn sample_config() -> Config {
        Config {
            base_url: "https://example.test/v1".into(),
            provider_keys: vec![ProviderKey {
                provider: "deepseek".into(),
                keychain_id: "gloss/deepseek".into(),
            }],
            model: "vision-model".into(),
            target_lang: Lang::Other("ko".into()),
            cache_ttl_secs: 120,
            theme: Theme::Dark,
            language: Language::En,
            watch_clipboard_images: true,
        }
    }

    #[test]
    fn save_then_load_round_trips_every_field() {
        let dir = tempfile::tempdir().expect("tempdir should create");
        let store = FileConfigStore::in_dir(dir.path().to_path_buf());
        let config = sample_config();

        store.save(&config).expect("save should succeed");
        assert_eq!(store.load().expect("load should succeed"), config);
    }

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

    #[test]
    fn load_creates_missing_directories() {
        let dir = tempfile::tempdir().expect("tempdir should create");
        let store = FileConfigStore::in_dir(dir.path().join("gloss").join("nested"));

        store.load().expect("load should create directories");
        assert!(store.path().is_file());
    }

    #[test]
    fn composite_delegates_document_methods() {
        let dir = tempfile::tempdir().expect("tempdir should create");
        let store = CompositeConfigStore::in_dir(dir.path().to_path_buf());
        let config = sample_config();

        store.save(&config).expect("save should succeed");
        assert_eq!(store.load().expect("load should succeed"), config);
    }

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

    #[test]
    fn load_rejects_corrupt_file_and_quarantines_it() {
        let dir = tempfile::tempdir().expect("tempdir should create");
        let store = FileConfigStore::in_dir(dir.path().to_path_buf());
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
        assert_eq!(config.model, gloss_core::config::DEFAULT_MODEL);
    }

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

    fn backups_in(dir: &Path) -> Vec<PathBuf> {
        let mut found: Vec<_> = std::fs::read_dir(dir)
            .expect("read_dir should succeed")
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.to_string_lossy().ends_with(".bak"))
            .collect();
        found.sort();
        found
    }

    #[test]
    fn error_line_reports_the_offending_line() {
        let text = "theme = \"Light\"\ntarget_lang = 7\n";
        let offset = text.find('7').expect("fixture must contain the value");
        assert_eq!(error_line(text, Some(offset..offset + 1)), Some(2));
        assert_eq!(error_line(text, None), None);
    }

    #[test]
    fn default_cache_ttl_matches_core_cache_semantics() {
        let default = Config::default();
        assert_eq!(
            Duration::from_secs(default.cache_ttl_secs),
            Duration::from_secs(60 * 60)
        );
    }
}
