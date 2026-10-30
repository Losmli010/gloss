//! 端口定义：全部跨层契约的唯一定义点（端口与适配器架构）。
//!
//! 核心层只声明契约，实现由适配器包提供——本包只自带大语言模型（LLM）
//! 客户端这一份引擎实现；各端口互不依赖，编排层只见到契约，测试用
//! 测试桩目录下的桩替换实现。
//!
//! 配置持久化与密钥存取是两个独立端口：配置存储（ConfigStore）只答配置
//! 文档的整读整存，密钥存储（SecretStore）只答密钥的读写删（条目不存在
//! 视为删除成功）；密钥不进配置快照，只需密钥的调用方持密钥存储的句柄
//! ——密钥来源唯一由类型系统承载。
use std::pin::Pin;

use futures_core::Stream;

use crate::config::Config;
use crate::guard::SceneFacts;
use crate::model::GlossError;
use crate::prompt::ChatMessage;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskChunk {
    Delta(String),
    Done { finish_reason: Option<String> },
}

pub type TaskStream = Pin<Box<dyn Stream<Item = Result<TaskChunk, GlossError>> + Send>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineRequest {
    pub messages: Vec<ChatMessage>,
    pub model: String,
    pub max_tokens: Option<u32>,
}

pub trait SceneProbe: Send + Sync {
    fn facts(&self) -> SceneFacts;
}

pub trait AiEngine: Send + Sync {
    fn execute(&self, request: EngineRequest)
    -> BoxFuture<'static, Result<TaskStream, GlossError>>;
}

pub trait ConfigStore: Send + Sync {
    fn load(&self) -> Result<Config, GlossError>;
    fn save(&self, config: &Config) -> Result<(), GlossError>;
}

pub trait SecretStore: Send + Sync {
    fn secret(&self, key: &str) -> Result<Option<String>, GlossError>;
    fn set_secret(&self, key: &str, value: &str) -> Result<(), GlossError>;
    fn delete_secret(&self, key: &str) -> Result<(), GlossError>;
}

pub trait AppIcon {
    fn install(&self, png: &[u8]) -> bool;
}
