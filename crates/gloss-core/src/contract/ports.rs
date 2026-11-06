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
//!
//! 任务流契约：任务流（TaskStream）的条目序里，错误条目即终结——终结后
//! 不再产项，消费方以首个错误条目收尾并丢弃半截产物；完成条目（Done）为
//! 末项（正常终结），其后不再有增量。完成条目携带的结束原因
//! （finish_reason）仅供诊断，不承载行为。
//!
//! 平台能力端口：选区手势源、选区文本取材、剪贴板监视、剪贴板图片取材、
//! 剪贴板文本写入、应用菜单装配、权限预检与引导。轮询型端口（手势源、
//! 文本取材、监视、图片取材）按“一次装配、单线程消费”的形状声明，方法
//! 带内部状态故取可变接收者，装配句柄把它们按值移交事件线程；文本取材
//! 的产物与读后还原收尾分离，收尾在产物上抛之后、在取材线程上执行
//! （还原收尾是可执行的收尾动作，读取方不感知其内部形态，不跨线程）。
//! 权限引导监视以节流轮询闭包移交，闭包跨入事件线程须可发送，轮询返回
//! 真即授权达成。
use std::pin::Pin;
use std::sync::Arc;

use futures_core::Stream;

use crate::config::Config;
use crate::guard::SceneFacts;
use crate::model::{GlossError, Locale, ScreenPoint};
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

pub trait AppIcon: Send + Sync {
    fn install(&self, png: &[u8]) -> Result<(), GlossError>;
}

pub trait SelectionGestureSource: Send {
    fn poll(&mut self) -> Vec<ScreenPoint>;
}

pub struct TextRead {
    outcome: Result<String, GlossError>,
    settle: TextSettle,
}

impl TextRead {
    pub fn new(outcome: Result<String, GlossError>, settle: TextSettle) -> Self {
        Self { outcome, settle }
    }

    pub fn into_parts(self) -> (Result<String, GlossError>, TextSettle) {
        (self.outcome, self.settle)
    }
}

pub struct TextSettle(Option<Box<dyn FnOnce()>>);

impl TextSettle {
    pub fn idle() -> Self {
        Self(None)
    }

    pub fn deferred(restore: impl FnOnce() + 'static) -> Self {
        Self(Some(Box::new(restore)))
    }

    pub fn finish(self) {
        if let Some(restore) = self.0 {
            restore();
        }
    }
}

pub trait TextAcquirer: Send {
    fn read(&mut self) -> TextRead;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardImageObserved;

pub trait ClipboardWatcher: Send {
    fn poll(&mut self) -> Vec<ClipboardImageObserved>;
}

pub trait ClipboardImageReader: Send {
    fn read(&mut self) -> Result<Option<Arc<[u8]>>, GlossError>;
}

pub trait ClipboardWriter: Send + Sync {
    fn copy_text(&self, text: &str) -> Result<(), String>;
}

pub trait MenuController: Send + Sync {
    fn install(
        &self,
        locale: Locale,
        on_open_settings: Box<dyn Fn() + Send>,
        on_quit: Box<dyn Fn() + Send>,
    ) -> bool;
}

pub trait PermissionProbe: Send + Sync {
    fn preflight(&self) -> bool;
    fn request(&self) -> bool;
    fn watch(&self, grant_pending: bool) -> Box<dyn FnMut() -> bool + Send>;
}
