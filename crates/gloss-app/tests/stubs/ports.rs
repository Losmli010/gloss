//! 端口桩：本 crate 测试用到的配置存储与场景探针预置替身。

use std::collections::HashMap;
use std::sync::Mutex;

use gloss_core::config::Config;
use gloss_core::guard::SceneFacts;
use gloss_core::model::GlossError;
use gloss_core::ports::{ConfigStore, SceneProbe};

use super::lock_or_recover;

/// 内存版配置存储桩：密钥键值对 + 单份配置文档，可按需注入失败。
#[derive(Default)]
pub struct MemoryConfigStore {
    secrets: Mutex<HashMap<String, String>>,
    config: Mutex<Option<Config>>,
    /// `Some` 时 `load` 直接返回它（模拟损坏的配置文件）。
    load_failure: Mutex<Option<GlossError>>,
    /// `Some` 时 `save` 直接返回它（模拟落盘失败）。
    save_failure: Mutex<Option<GlossError>>,
    /// `Some` 时 `secret` 直接返回它（模拟 keychain 授权失败）。
    secret_failure: Mutex<Option<GlossError>>,
}

impl MemoryConfigStore {
    /// 让后续 `load` 一律失败（配置文件损坏路径）。
    pub fn with_load_failure(self, error: GlossError) -> Self {
        *lock_or_recover(&self.load_failure) = Some(error);
        self
    }

    /// 让后续 `save` 一律失败（落盘失败路径）。
    pub fn with_save_failure(self, error: GlossError) -> Self {
        *lock_or_recover(&self.save_failure) = Some(error);
        self
    }

    /// 让后续 `secret` 一律失败（keychain 读取失败路径）。
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

/// 场景探针桩：事实可预置、可中途改写，默认「无事实」（两道场景闸门
/// 都放行）——测试不碰真机的安全输入态与前台应用。
#[derive(Default)]
pub struct StubSceneProbe {
    facts: Mutex<SceneFacts>,
}

impl StubSceneProbe {
    /// 预置场景事实。
    pub fn with_facts(facts: SceneFacts) -> Self {
        Self {
            facts: Mutex::new(facts),
        }
    }

    /// 改写当前事实（模拟两次触发之间的场景变化）。
    pub fn set_facts(&self, facts: SceneFacts) {
        *lock_or_recover(&self.facts) = facts;
    }
}

impl SceneProbe for StubSceneProbe {
    fn facts(&self) -> SceneFacts {
        lock_or_recover(&self.facts).clone()
    }
}
