//! 端口桩：本 crate 测试用到的配置存储与热键重绑定预置替身。

use std::collections::HashMap;
use std::sync::Mutex;

use gloss_core::config::Config;
use gloss_core::model::GlossError;
use gloss_core::ports::{ConfigStore, HotkeyBinder};
use gloss_core::task::HotkeyBinding;

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

/// 记录每次重绑定的热键桩。
///
/// 与真实实现不同，它不接触任何平台资源。观测点是「调用发生过」与
/// 「收到的是哪份绑定表」，注入点是调用次数（首次装配不调、保存成功
/// 才调），够覆盖接线契约。
#[derive(Default)]
pub struct RecordingHotkeyBinder {
    calls: Mutex<Vec<Vec<HotkeyBinding>>>,
}

impl RecordingHotkeyBinder {
    /// 收到过的重绑定次数。
    pub fn call_count(&self) -> usize {
        lock_or_recover(&self.calls).len()
    }

    /// 最近一次收到的绑定表；从未被调用过时返回 `None`。
    pub fn last(&self) -> Option<Vec<HotkeyBinding>> {
        lock_or_recover(&self.calls).last().cloned()
    }
}

impl HotkeyBinder for RecordingHotkeyBinder {
    fn rebind(&self, bindings: &[HotkeyBinding]) -> usize {
        lock_or_recover(&self.calls).push(bindings.to_vec());
        // 桩不做平台注册，全部绑定视为生效——即模拟一个一切正常的平台。
        // 「几条被占用、几级被降级」是平台侧的事实，桩不替它编结果。
        bindings.len()
    }
}
