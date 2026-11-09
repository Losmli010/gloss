//! 配置存储适配器：文档半边与密钥半边各司其职——文档半边是文件存储，以
//! 标记语言（TOML）落盘；密钥半边是凭据管理器密钥句柄，即系统安全存储
//! ——由组合存储同时实现配置持久化（ConfigStore）与密钥（SecretStore）
//! 两个端口，组装点注入的就是组合体。与系统（macOS）的组合存储同名镜像
//! （平台目录自持）。

pub mod cred;
pub mod file;

use std::path::PathBuf;

use gloss_core::config::Config;
use gloss_core::model::GlossError;
use gloss_core::ports::{ConfigStore, SecretStore};

use self::cred::CredentialSecret;
use self::file::FileConfigStore;

#[derive(Debug)]
pub struct CompositeConfigStore {
    document: FileConfigStore,
    secrets: CredentialSecret,
}

impl CompositeConfigStore {
    pub fn new() -> Result<Self, GlossError> {
        Ok(Self {
            document: FileConfigStore::new()?,
            secrets: CredentialSecret::new(),
        })
    }

    pub fn in_dir(dir: PathBuf) -> Self {
        Self {
            document: FileConfigStore::in_dir(dir),
            secrets: CredentialSecret::new(),
        }
    }
}

impl ConfigStore for CompositeConfigStore {
    fn load(&self) -> Result<Config, GlossError> {
        self.document.load()
    }

    fn save(&self, config: &Config) -> Result<(), GlossError> {
        self.document.save(config)
    }
}

impl SecretStore for CompositeConfigStore {
    fn secret(&self, key: &str) -> Result<Option<String>, GlossError> {
        self.secrets.secret(key)
    }

    fn set_secret(&self, key: &str, value: &str) -> Result<(), GlossError> {
        self.secrets.set_secret(key, value)
    }

    fn delete_secret(&self, key: &str) -> Result<(), GlossError> {
        self.secrets.delete_secret(key)
    }
}

impl SecretStore for CredentialSecret {
    fn secret(&self, key: &str) -> Result<Option<String>, GlossError> {
        Self::get(self, key)
    }

    fn set_secret(&self, key: &str, value: &str) -> Result<(), GlossError> {
        Self::set(self, key, value)
    }

    fn delete_secret(&self, key: &str) -> Result<(), GlossError> {
        Self::delete(self, key)
    }
}

#[cfg(test)]
mod tests {
    use gloss_core::config::{Language, ProviderKey, Theme};
    use gloss_core::model::Lang;
    use gloss_core::ports::ConfigStore;

    use super::*;

    fn sample_config() -> Config {
        Config {
            base_url: "https://example.test/v1".into(),
            provider_keys: vec![ProviderKey {
                provider: "deepseek".into(),
                keychain_id: "gloss/deepseek".into(),
            }],
            model: "vision-model".into(),
            target_lang: Lang::En,
            cache_ttl_secs: 120,
            theme: Theme::Dark,
            language: Language::En,
            watch_clipboard_images: true,
        }
    }

    #[test]
    fn composite_save_then_load_round_trips_the_document_half() {
        let dir = tempfile::tempdir().expect("tempdir should create");
        let store = CompositeConfigStore::in_dir(dir.path().to_path_buf());
        let config = sample_config();

        store.save(&config).expect("save should succeed");
        assert_eq!(store.load().expect("load should succeed"), config);
    }
}
