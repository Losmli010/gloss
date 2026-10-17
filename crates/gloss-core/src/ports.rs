//! 端口定义：全部跨层 trait 的唯一定义点（Ports & Adapters）。
//!
//! core 只声明契约，实现由适配器 crate 提供（本 crate 只自带 [`crate::engine::llm`]
//! 这一份 [`AiEngine`] 实现）；各端口互不依赖，编排层只见到 trait，测试用
//! `tests/stubs/` 下的桩替换实现。
use std::pin::Pin;

use futures_core::Stream;

use crate::config::Config;
use crate::guard::SceneFacts;
use crate::model::GlossError;
use crate::prompt::ChatMessage;

/// 装箱 future：trait 方法要返回异步结果又不能丢对象安全（`dyn AiEngine`），
/// 只能装箱；`Send` 供消费端在 tokio 上轮询。
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 任务产物流的单个条目：文本增量或服务端声明的流结束。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskChunk {
    Delta(String),
    /// 服务端声明的流结束（SSE `data: [DONE]`），携带 `choices[0]` 的
    /// `finish_reason`（`stop` / `length` / `content_filter`…；未声明为 `None`）。
    /// 终结条目：其后的增量没有契约地位，实现方不再产出。
    Done {
        /// 结束原因原文（不转译，消费方按需判定）。
        finish_reason: Option<String>,
    },
}

/// 任务产物流：文本增量与服务端声明的流结束。上层的流式通道只转发
/// [`TaskChunk::Delta`]，结束条目由编排层消费。
///
/// `Err` 是普通增量的一种，**不保证终结流**：实现方可在 Err 后继续产出，
/// 消费方以**首个 `Err` 为终结**并丢弃半截产物。
pub type TaskStream = Pin<Box<dyn Stream<Item = Result<TaskChunk, GlossError>> + Send>>;

/// 引擎请求：**已渲染**的对话消息 + **已解析**的模型 id。
///
/// 渲染归 core 编排（含模态校验），引擎只负责送出请求、流回响应——不做渲染，
/// 也不回读配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineRequest {
    /// 渲染好的消息（多模态 content 数组随图像任务扩展）。
    pub messages: Vec<ChatMessage>,
    /// 本任务使用的模型 id（App 在触发时按配置冻结）。
    pub model: String,
    /// 回复的 token 上限（OpenAI 兼容 `max_tokens`）：`None` = 不设限。
    /// 需要截断回复的调用方（如分类只回一小段 JSON）按请求性质自行携带。
    pub max_tokens: Option<u32>,
}

/// 触发前场景探针（端口）：读一次当前场景事实（安全输入态与前台应用），
/// 供 [`crate::guard`] 的场景闸门判定。
///
/// 与其他端口不同，**没有失败面**：取不到事实就返回空事实
/// （[`SceneFacts::default`]），闸门按「无事实」放行——探针只该拦下能确定的
/// 危险场景，不该因为自己读不到而拦掉用户的正常划词。实现方必须纯查询、
/// 不阻塞、不索取新权限：每个平台事件都会读一次，无论它最后是否成为取材。
pub trait SceneProbe: Send + Sync {
    fn facts(&self) -> SceneFacts;
}

/// AI 引擎（端口）：统一入口，不按输入模态拆分——文本/图文只由消息 payload
/// 与模型 id 决定。**模型不回读配置**（随请求携带）；引擎自己读快照的只有
/// 端点与 provider 条目。
///
/// 实现方保证：`execute` 返回的 future 与流都是 `'static` 且 `Send`，**不得
/// 借用 `self`**；`request` 按值接收，实现方把数据移入 future 即可。取消不进
/// 本端口：调用方以 `CancellationToken` 在 await 侧竞速，实现方只需保证
/// future 被丢弃时连接随之关闭（异步客户端的默认行为）。
pub trait AiEngine: Send + Sync {
    fn execute(&self, request: EngineRequest)
    -> BoxFuture<'static, Result<TaskStream, GlossError>>;
}

/// 配置存储（端口）：应用配置与密钥的读写边界。
///
/// 实现通常由两半边组合：配置文档落 TOML，密钥按 keychain 条目标识落系统
/// 安全存储——密钥不进配置快照，用时直查。全部方法取 `&self`（实现方以内部
/// 同步保证并发安全），适配器才能以 `Arc<dyn ConfigStore>` 注入。
///
/// 密钥红线：入参与返回值都是凭据，实现方禁止将其写进日志、错误消息或
/// [`GlossError::EngineResponse`] 这类携带诊断文本的变体。读取配置失败时
/// 同理：错误文本不得转述配置文件内容（解析错误常引用出错行或取值，而用户
/// 可能把密钥贴错字段），只给位置与类别。
pub trait ConfigStore: Send + Sync {
    /// 缺文件时返回出厂默认（并尽力落盘）。
    fn load(&self) -> Result<Config, GlossError>;
    /// 整份写入（设置页保存路径）。
    fn save(&self, config: &Config) -> Result<(), GlossError>;
    /// 读取密钥；`None` 表示未设置。
    fn secret(&self, key: &str) -> Result<Option<String>, GlossError>;
    fn set_secret(&self, key: &str, value: &str) -> Result<(), GlossError>;
    /// 删除密钥；条目不存在视为成功（幂等，设置页「清除密钥」用）。
    fn delete_secret(&self, key: &str) -> Result<(), GlossError>;
}

/// 应用图标（端口）：把品牌图标交给平台外壳（Dock / 应用切换器）。
///
/// 这是**降级端口**：安装失败（调用线程不对、系统拒绝、字节解不出图）只让
/// 图标退回系统默认，不影响启动，因此**不返回错误**——只回报是否成功，供
/// 调用方记一行日志定位。
///
/// 调用方保证：在**主线程**上调用，且晚于 winit 的 `EventLoop` 构建——
/// macOS 的 `NSApplication` 单例在 `EventLoop::new` 之前访问不受支持。
pub trait AppIcon {
    fn install(&self, png: &[u8]) -> bool;
}
