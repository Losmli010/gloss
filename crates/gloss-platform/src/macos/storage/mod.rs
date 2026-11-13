//! 配置存储的密钥半边（系统（macOS）绑定）：钥匙串密钥句柄即系统安全存储，
//! 由密钥端口（SecretStore）的出厂实现落到其上。组合体与文档半边住平台
//! 中立共享层（common::storage），本模块只做密钥半边绑定：出厂组合存储即
//! 共享层的组合体接上钥匙串密钥半边，组装点注入的就是组合体。

pub mod keychain;

use gloss_core::model::GlossError;
use gloss_core::ports::SecretStore;

use crate::common::storage::FileConfigStore;

pub type CompositeConfigStore =
    crate::common::storage::CompositeConfigStore<keychain::KeychainSecret>;

impl CompositeConfigStore {
    pub fn new() -> Result<Self, GlossError> {
        Ok(Self::with_secrets(
            FileConfigStore::new()?,
            keychain::KeychainSecret::new(),
        ))
    }
}

impl SecretStore for keychain::KeychainSecret {
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
