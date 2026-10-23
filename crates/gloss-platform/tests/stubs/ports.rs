//! 端口桩：本 `crate` 测试用到的配置存储预置替身。

use std::collections::HashMap;
use std::sync::Mutex;

use gloss_core::config::Config;
use gloss_core::model::GlossError;
use gloss_core::ports::ConfigStore;

use super::lock_or_recover;

#[derive(Default)]
pub struct MemoryConfigStore {
    secrets: Mutex<HashMap<String, String>>,
    config: Mutex<Option<Config>>,
    load_failure: Mutex<Option<GlossError>>,
    save_failure: Mutex<Option<GlossError>>,
    secret_failure: Mutex<Option<GlossError>>,
}

impl MemoryConfigStore {
    pub fn with_load_failure(self, error: GlossError) -> Self {
        *lock_or_recover(&self.load_failure) = Some(error);
        self
    }

    pub fn with_save_failure(self, error: GlossError) -> Self {
        *lock_or_recover(&self.save_failure) = Some(error);
        self
    }

    pub fn with_secret_failure(self, error: GlossError) -> Self {
        *lock_or_recover(&self.secret_failure) = Some(error);
        self
    }
}

impl ConfigStore for MemoryConfigStore {
    fn load(&self) -> Result<Config, GlossError> {
        if let Some(err) = lock_or_recover(&self.load_failure).clone() {
            return Err(err);
        }
        Ok(lock_or_recover(&self.config).clone().unwrap_or_default())
    }

    fn save(&self, config: &Config) -> Result<(), GlossError> {
        if let Some(err) = lock_or_recover(&self.save_failure).clone() {
            return Err(err);
        }
        *lock_or_recover(&self.config) = Some(config.clone());
        Ok(())
    }

    fn secret(&self, key: &str) -> Result<Option<String>, GlossError> {
        if let Some(err) = lock_or_recover(&self.secret_failure).clone() {
            return Err(err);
        }
        Ok(lock_or_recover(&self.secrets).get(key).cloned())
    }

    fn set_secret(&self, key: &str, value: &str) -> Result<(), GlossError> {
        lock_or_recover(&self.secrets).insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete_secret(&self, key: &str) -> Result<(), GlossError> {
        lock_or_recover(&self.secrets).remove(key);
        Ok(())
    }
}
