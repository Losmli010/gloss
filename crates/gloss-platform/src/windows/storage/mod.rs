//! 配置存储适配器（系统（Windows）占位）：组合存储的桩。
//!
//! 文档半边（标记语言（TOML）文件加目录定位）与密钥半边（凭据管理器
//! （CredReadW／CredWriteW／CredDeleteW））按移植方案后续阶段补齐；本桩
//! 构造可用、读取失败回落出厂默认（应用壳的既有降级路径），读写与密钥
//! 操作一律报告未支持。

use gloss_core::config::Config;
use gloss_core::model::GlossError;
use gloss_core::ports::{ConfigStore, SecretStore};

#[derive(Debug, Default)]
pub struct CompositeConfigStore;

impl CompositeConfigStore {
    pub fn new() -> Result<Self, GlossError> {
        Ok(Self)
    }
}

impl ConfigStore for CompositeConfigStore {
    fn load(&self) -> Result<Config, GlossError> {
        Err(GlossError::PlatformUnsupported)
    }

    fn save(&self, _config: &Config) -> Result<(), GlossError> {
        Err(GlossError::PlatformUnsupported)
    }
}

impl SecretStore for CompositeConfigStore {
    fn secret(&self, _key: &str) -> Result<Option<String>, GlossError> {
        Err(GlossError::PlatformUnsupported)
    }

    fn set_secret(&self, _key: &str, _value: &str) -> Result<(), GlossError> {
        Err(GlossError::PlatformUnsupported)
    }

    fn delete_secret(&self, _key: &str) -> Result<(), GlossError> {
        Err(GlossError::PlatformUnsupported)
    }
}
