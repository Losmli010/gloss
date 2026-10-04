//! 端口桩：选区读取、截图与配置存储的预置替身。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use gloss_core::config::Config;
use gloss_core::model::{GlossError, ScreenRect};
use gloss_core::ports::{ConfigStore, RegionCapture, SelectionReader};

use super::lock_or_recover;

/// 返回预置结果（成功文本或失败）的选区读取桩。
pub struct FixedSelectionReader(
    /// 每次 `read` 原样返回的结果。
    pub Result<String, GlossError>,
);

impl SelectionReader for FixedSelectionReader {
    fn read(&mut self) -> Result<String, GlossError> {
        self.0.clone()
    }
}

/// 返回预置 PNG 字节的截图桩。
pub struct FixedRegionCapture(
    /// 每次 `capture` 原样返回的结果。
    pub Result<Arc<[u8]>, GlossError>,
);

impl RegionCapture for FixedRegionCapture {
    fn capture(&mut self, _rect: ScreenRect) -> Result<Arc<[u8]>, GlossError> {
        self.0.clone()
    }
}

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
