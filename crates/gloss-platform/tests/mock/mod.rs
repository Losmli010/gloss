//! gloss-platform 测试桩库：本 crate 测试所需的端口替身副本。
//!
//! 桩按 crate 自持、跨 crate 刻意不共享（一份桩的行为变化不得静默改写
//! 另一个 crate 测试套件的语义），本文件与 gloss-core `tests/mock/` 的
//! 同名桩保持逐字一致；改注入语义时两边同步。当前仅 `src/` 内联单测
//! 使用（`lib.rs` 以 `#[cfg(test)] #[path]` 包含为 `crate::mock`）。
//! 本模块按生产代码对待（lint 与注释规则同 `src/`，见 AGENTS.md）。

// 桩是按需取用的能力全集：每个编译目标只用到其中一部分，未用能力不算死代码。
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use gloss_core::config::Config;
use gloss_core::model::GlossError;
use gloss_core::ports::ConfigStore;

/// 锁中毒恢复：测试基建不值得 panic，拿回守卫继续用（数据由测试自身
/// 单线程写入，中毒不可能源于本模块逻辑）。
fn lock_or_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
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
