//! 端口定义：全部跨层 trait 的唯一定义点（Ports & Adapters）。
//!
//! core 只声明契约；实现侧在 gloss-platform（`CompositeReader` /
//! `ScreenCapturer` / `LlmClient` / `FileConfigStore`），
//! 核心编排只见到这些 trait，测试用各 crate tests/stubs/ 下的桩。
//!
use std::pin::Pin;
use std::sync::Arc;

use futures_core::Stream;

use crate::config::Config;
use crate::guard::SceneFacts;
use crate::model::{GlossError, ScreenRect};
use crate::prompt::ChatMessage;

/// 装箱 future：让 trait 方法携带异步结果的同时保持对象安全（`dyn` 可用）。
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 任务产物流：流式增量（与通道④ `Event::TaskChunk` 同载荷）。
///
/// `Err` 是普通增量的一种，**不保证终结流**：实现方可在 Err 后继续产出，
/// 消费方以**首个 `Err` 为终结**并丢弃半截产物（编排层的实现语义，
/// 见 engine 模块）。
pub type TaskStream = Pin<Box<dyn Stream<Item = Result<String, GlossError>> + Send>>;

/// 文本取材（端口）：读前台应用的选中文本。
///
/// 实现方保证：模拟复制兜底必须保存并恢复剪贴板。
/// 调用方保证：在平台事件线程上调用（线程亲和性）。
///
pub trait SelectionReader: Send {
    /// 读取前台应用当前选中文本；权限缺失返回
    /// [`GlossError::AccessibilityDenied`]，选区为空返回
    /// [`GlossError::SelectionEmpty`]，其余取不到的情形返回
    /// [`GlossError::SelectionUnavailable`]。
    fn read(&mut self) -> Result<String, GlossError>;
}

/// 图像取材（端口）：截取屏幕指定区域。
///
/// 实现方保证：不写剪贴板（系统截图的默认行为需显式规避）。
/// 调用方保证：在平台事件线程上调用（线程亲和性）。
pub trait RegionCapture: Send {
    /// 截取 `rect` 区域，返回 PNG 字节。
    fn capture(&mut self, rect: ScreenRect) -> Result<Arc<[u8]>, GlossError>;
}

/// 引擎请求（[`AiEngine`] 的入参）：**已渲染**的对话消息 + **已解析**的
/// 模型 id。
///
/// 渲染是 core 编排的职责（`AiTaskService` 调 `PromptRegistry`，含模态校验），
/// 引擎只负责把请求送出去、把响应流回来——不做渲染，也不回读配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineRequest {
    /// 渲染好的消息（多模态 content 数组随图像任务扩展）。
    pub messages: Vec<ChatMessage>,
    /// 本任务使用的模型 id（App 在触发时按配置冻结）。
    pub model: String,
    /// 回复的 token 上限（OpenAI 兼容 `max_tokens`）：`None` = 不设限。
    /// 渲染与转发不感知它——需要截断回复的调用方（如分类这类只回一小段
    /// JSON 的请求）按请求性质自行携带。
    pub max_tokens: Option<u32>,
}

/// 触发前场景探针（端口）：读一次当前场景事实（安全输入态与前台应用），
/// 供 `gloss_core::guard` 的场景闸门判定。
///
/// 与其余端口的差别是**没有失败面**：探针取不到事实就返回空事实（
/// [`SceneFacts::default`]），闸门按「无事实」放行——探针的作用是拦下能
/// 确定的危险场景，不该因为自己读不到而把用户的正常划词也拦掉。同理，
/// 实现方必须**纯查询、不阻塞、不索取新权限**：每个平台事件（设置入口
/// 除外）都会读一次，无论它最后是否成为一次取材。
pub trait SceneProbe: Send + Sync {
    /// 读一次场景事实。
    fn facts(&self) -> SceneFacts;
}

/// AI 引擎（端口）：统一入口，不按输入模态拆分——文本/图文仅由消息
/// payload 与模型 id（[`EngineRequest`]）决定。渲染归 core 编排（引擎不做
/// 渲染）；**模型不回读配置**（随请求携带）。引擎自己读快照的只有端点与
/// provider 条目。
///
/// 实现方保证：`execute` 返回的 future 与流都是 `'static` 且 `Send`——
/// **不得借用 `self`**；`request` 按值接收，调用方构造后即不再使用，
/// 实现方把请求数据移入 future 即可。消费端在 tokio 上轮询。取消不进
/// 本端口，由调用方以 `CancellationToken` 在 await 侧竞速（单一取消
/// 机制）——实现方只需保证 future 被丢弃时连接随之关闭（异步客户端的
/// 默认行为）。
pub trait AiEngine: Send + Sync {
    /// 执行请求，返回流式产物流。
    fn execute(&self, request: EngineRequest)
    -> BoxFuture<'static, Result<TaskStream, GlossError>>;
}

/// 配置存储（端口）：应用配置与密钥的读写边界。
///
/// 实现侧由两半边组合（`CompositeConfigStore`）：配置文档走
/// [`ConfigStore::load`] / [`ConfigStore::save`] 落 TOML 文件
/// （`FileConfigStore`），密钥走条目标识落系统安全存储
/// （`KeychainSecret`）——密钥不进配置快照，用时直查。
/// 全部方法取 `&self`（实现方以内部同步保证并发安全），适配器才能以
/// `Arc<dyn ConfigStore>` 注入。
///
/// 密钥红线（AGENTS.md）：入参与返回值都是凭据，实现方禁止将其写进
/// 日志、错误消息或 `EngineResponse` 这类携带诊断文本的变体。读取配置
/// 失败时同理：错误文本不得转述配置文件内容（解析错误常引用出错行或
/// 取值，而用户可能把密钥贴错字段），只给位置与类别。
pub trait ConfigStore: Send + Sync {
    /// 读取整份配置；实现方保证缺文件时返回出厂默认（并尽力落盘）。
    fn load(&self) -> Result<Config, GlossError>;
    /// 原子写入整份配置（设置页保存路径）。
    fn save(&self, config: &Config) -> Result<(), GlossError>;
    /// 读取密钥；`None` 表示未设置。
    fn secret(&self, key: &str) -> Result<Option<String>, GlossError>;
    /// 写入（或覆盖）密钥。
    fn set_secret(&self, key: &str, value: &str) -> Result<(), GlossError>;
    /// 删除密钥；条目不存在视为成功（幂等，设置页「清除密钥」路径用）。
    fn delete_secret(&self, key: &str) -> Result<(), GlossError>;
}

/// 应用图标（端口）：把品牌图标交给平台外壳（Dock / 应用切换器）。
///
/// 与其余端口不同，本端口是**降级端口**：安装失败（调用线程不对、系统拒绝、
/// 字节解不出图）只让图标退回系统默认，不影响启动，因此**不返回错误**——
/// 只回报是否设置成功，供调用方记一行日志定位。
///
/// 调用方保证：在**主线程**上调用，且晚于 winit 的 `EventLoop` 构建——
/// macOS 的 `NSApplication` 单例在 `EventLoop::new` 之前访问不受支持
/// （winit 的 macOS 平台文档明确要求 `sharedApplication` 放在其后）。
pub trait AppIcon {
    /// 用 PNG 字节安装应用图标；返回是否设置成功。
    fn install(&self, png: &[u8]) -> bool;
}
