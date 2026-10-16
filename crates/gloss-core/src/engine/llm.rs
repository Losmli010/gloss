//! LlmClient：OpenAI 兼容端点的流式引擎适配器（[`crate::ports::AiEngine`]
//! 端口的内建实现）。
//!
//! 边界：**只负责把请求送出去、把响应流回来**。messages 由编排层渲染
//! （`super::AiTaskService` → `prompt::PromptRegistry`，含模态校验），模型
//! 也已由 App 按配置解析后随 [`EngineRequest`] 携带——本模块不渲染
//! prompt，也不看配置里的模型。域形态的消息（[`ChatMessage`]：文本或多
//! 模态部件）在这里映成 OpenAI 兼容 wire 形状；图像部件的 base64 与
//! data URL 前缀也只在请求体构造处编码（域类型不带 base64）。
//!
//! 端点与密钥：端点取配置快照的 `base_url`；密钥按 provider 条目的 keychain
//! 条目标识**每请求直查**（不缓存，除请求头外不进任何地方——错误消息与日志里
//! 只有状态码与服务端诊断文本；红线与措辞见 `crate::ports::ConfigStore` 的文档）。
//!
//! 超时与重试（均为传输层内建行为，编排层不重试、原样上抛）：建连超时
//! 之外，流式读取只设**空闲超时**（[`IDLE_TIMEOUT`]，连续无字节到达即断
//! 流）——长回复合法地跑满全预算，总超时只会误杀；传输失败在**首个流条
//! 目之前**自动重试（[`RETRY_ATTEMPTS`]，仅瞬态失败）。取消由调用方的
//! `CancellationToken` 竞速完成（future 被丢弃即断链）。

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use crate::config::Config;
use crate::config_handle::ConfigHandle;
use crate::log::{debug, thread, warn};
use crate::model::GlossError;
use crate::ports::{AiEngine, BoxFuture, ConfigStore, EngineRequest, TaskChunk, TaskStream};
use crate::prompt::{ChatMessage, ContentPart, MessageContent};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use bytes::Bytes;
use futures_core::Stream;
use reqwest::StatusCode;
use tokio::time::{Instant as TokioInstant, Sleep, sleep, sleep_until};

use super::sse::{SseDecoder, SseItem};

/// 建连超时（不含响应与流式读取）。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// 流式空闲超时：连续这么久没有任何字节到达即判传输停摆，流以
/// [`GlossError::EngineNetwork`] 终结（首个 `Err` 终结流的端口契约不变）。
/// 取值从宽以覆盖首字节等待——推理类模型出首个字节可以远慢于普通请求
/// ——也覆盖流中途的服务端生成间歇；只惩罚「连接还在、字节不来」的静默
/// 悬死，对长回复没有总超时那样的合法误杀。
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// 传输失败自动重试预算：只重试**首个流条目之前**的瞬态失败（请求没送达、
/// 服务端明确说稍后再来、连接初建即断）。连接阶段与首个流条目之前两个
/// 阶段共用同一份预算——前一阶段耗尽，后一阶段不再重试。
const RETRY_ATTEMPTS: u32 = 1;

/// 无 `Retry-After` 指引时的固定退避。
const RETRY_BACKOFF: Duration = Duration::from_millis(500);

/// `Retry-After` 指引的等待上限：更长的等待不自动进行——失败卡交还用户，
/// 重试按钮在手（状态机对网络/限流错误的既有出口）。
const RETRY_AFTER_CAP: Duration = Duration::from_secs(1);

/// 聊天补全路径（OpenAI 兼容）。
const CHAT_COMPLETIONS_PATH: &str = "chat/completions";

/// 错误响应体的读取上限：只要 `error.message`，多余字节没有价值，而错误
/// 网关的劫持页可能极大（截断后 JSON 解析失败会退化成「只报状态码」，
/// 与既有非 JSON 路径一致）。
const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;

/// 零增量完成的固定诊断：适配器对「成功收尾但一个增量都没有」的坏响应
/// 统一发这个 [`GlossError::EngineResponse`] 文本。重试循环（`super`）按
/// 它把这类坏回复识别为 [`crate::schema::Violation::EmptyReply`] 进修复
/// 轮——单点常量让两处措辞永不漂移。
pub(crate) const EMPTY_COMPLETION: &str = "empty completion";

/// 组装 OpenAI 兼容请求体（`chat/completions` 的形状）。消息在这里从域
/// 形态映成 wire 形状；`max_tokens` 只在设限时携带：部分严格端点会拒绝
/// 显式的 `null`，缺键才是「不设限」的 wire 语义。
///
/// `response_format` json_object **恒携带**（[`EngineRequest`] 不加开关）：
/// 所有请求都是 JSON 契约，不做能力协商——端点不认时以 4xx 拒绝，由
/// [`map_failure`] 的启发式映射成 [`GlossError::JsonModeUnsupported`]。
fn chat_request_body(request: &EngineRequest) -> serde_json::Value {
    let messages: Vec<serde_json::Value> = request.messages.iter().map(message_to_value).collect();
    let mut body = serde_json::json!({
        "model": request.model,
        "messages": messages,
        "stream": true,
        "response_format": { "type": "json_object" },
    });
    if let Some(max_tokens) = request.max_tokens {
        body["max_tokens"] = serde_json::json!(max_tokens);
    }
    body
}

/// 单条消息 → OpenAI 兼容 JSON：role 用小写契约名（`Role` 的 serde 拼写），
/// 正文按域形态分派（纯文本即 JSON 字符串，部件即 content 数组）。
fn message_to_value(message: &ChatMessage) -> serde_json::Value {
    serde_json::json!({
        "role": message.role,
        "content": content_to_value(&message.content),
    })
}

/// 消息正文 → wire 值：纯文本即字符串，部件数组即 OpenAI 兼容 content
/// 数组（`{"type":"text",…}` / `{"type":"image_url",…}`）。
fn content_to_value(content: &MessageContent) -> serde_json::Value {
    match content {
        MessageContent::Text(text) => serde_json::json!(text),
        MessageContent::Parts(parts) => {
            serde_json::Value::Array(parts.iter().map(part_to_value).collect())
        }
    }
}

/// 单个部件 → wire 值。图像字节在这里才做 base64 编码并拼 data URL 前缀
/// （`data:image/png;base64,…`）——域类型只携带原始字节，编码不出请求体
/// 构造这一步。
fn part_to_value(part: &ContentPart) -> serde_json::Value {
    match part {
        ContentPart::Text(text) => serde_json::json!({ "type": "text", "text": text }),
        ContentPart::ImagePng(png) => serde_json::json!({
            "type": "image_url",
            "image_url": {
                "url": format!("data:image/png;base64,{}", BASE64_STANDARD.encode(png)),
            },
        }),
    }
}

/// 读响应体但不超过 `limit` 字节（丢失的只是诊断文本，不是业务数据）。
async fn read_bounded_body(response: &mut reqwest::Response, limit: usize) -> String {
    let mut body = Vec::new();
    while let Ok(Some(chunk)) = response.chunk().await {
        body.extend_from_slice(&chunk);
        if body.len() >= limit {
            break;
        }
    }
    String::from_utf8_lossy(&body).into_owned()
}

/// OpenAI 兼容流式引擎。
pub struct LlmClient {
    /// 复用连接池的 HTTP 客户端（建连超时在这里定）。
    client: reqwest::Client,
    /// 端点等配置快照（模型不经这里）。
    config: Arc<ConfigHandle>,
    /// 密钥直查（keychain）。
    store: Arc<dyn ConfigStore>,
}

impl LlmClient {
    /// 组装：配置句柄与存储都由组装点注入（句柄给端点，存储给密钥）。
    pub fn new(config: Arc<ConfigHandle>, store: Arc<dyn ConfigStore>) -> Result<Self, GlossError> {
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            // 带一个明确的 UA：部分前置 CDN 对空 UA 返回 403，而 403 在这里
            // 会被映射成 EngineAuth，用户会被引去重填密钥——方向完全错了。
            .user_agent(concat!("gloss/", env!("CARGO_PKG_VERSION")))
            // 不跟随重定向。
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|err| GlossError::EngineResponse(format!("build http client: {err}")))?;
        Ok(Self {
            client,
            config,
            store,
        })
    }
}

impl AiEngine for LlmClient {
    fn execute(
        &self,
        request: EngineRequest,
    ) -> BoxFuture<'static, Result<TaskStream, GlossError>> {
        // reqwest::Client 内部是 Arc 连接池：克隆只是递引用计数（零堆分配），
        // 为满足 'static future 的所有权要求，勿改回传引用或重建客户端。
        let client = self.client.clone();
        let config = Arc::clone(&self.config);
        let store = Arc::clone(&self.store);
        let EngineRequest {
            messages,
            model,
            max_tokens,
        } = request;

        Box::pin(async move {
            if messages.is_empty() {
                // 渲染层（core 编排）保证至少一条 user 消息；空请求是接线错误，
                // 不该把一个空 body 发给付费端点。
                return Err(GlossError::EngineResponse("empty request".into()));
            }
            // 空白模型同样是配置问题：发了也是必然 400 的请求，还会把病因
            // 藏进服务端的错误文案里（手改配置或设置页存了空串）。
            if model.trim().is_empty() {
                return Err(GlossError::Config("empty model id".into()));
            }
            let snapshot = config.snapshot();
            // 端点与密钥分两条解析路径，不共用返回值。
            let resolved = Arc::new(Resolved::build(
                resolve_endpoint(&snapshot)?,
                resolve_api_key(&snapshot, store.as_ref())?,
                &EngineRequest {
                    messages,
                    model: model.trim().to_owned(),
                    max_tokens,
                },
            )?);
            // 单次尝试闭包：连接阶段与首个流条目之前的重试共用同一形状，
            // 重试即再调一次；端点、密钥与请求体都在 Arc 里，克隆只是递引
            // 用计数。
            let attempt = {
                let client = client.clone();
                let resolved = Arc::clone(&resolved);
                move || {
                    let client = client.clone();
                    let resolved = Arc::clone(&resolved);
                    Box::pin(async move { send_once(&client, &resolved).await })
                        as BoxFuture<'static, Result<TaskStream, TransportFail>>
                }
            };
            let (connected, retries_left) =
                connect_with_retry(attempt.clone(), RETRY_ATTEMPTS).await;
            let stream = connected?;
            debug!(
                model = %model,
                "llm stream established"
            );
            Ok(Box::pin(FirstByteStream::new(attempt, stream, retries_left)) as TaskStream)
        })
    }
}

/// 一次请求的解析产物：端点、密钥与 wire 请求体。请求体在构造处一次性
/// 序列化成 `Bytes`（图像任务的 base64 可达 MB 级，`.json(&Value)` 会在
/// 每次尝试时整体重序列化一份临时缓冲）；`Bytes` 克隆只递引用计数，重试
/// 复用零拷贝。
struct Resolved {
    url: String,
    key: String,
    body: Bytes,
}

impl Resolved {
    /// 组装并序列化请求体。序列化失败只可能是本地构造出非对象形态的
    /// `Value`（本模块恒产出 JSON 对象），按请求异常上抛。
    fn build(url: String, key: String, request: &EngineRequest) -> Result<Self, GlossError> {
        let body = serde_json::to_vec(&chat_request_body(request))
            .map_err(|err| GlossError::EngineResponse(format!("serialize request body: {err}")))?;
        Ok(Self {
            url,
            key,
            body: Bytes::from(body),
        })
    }
}

/// 一次传输尝试的失败载荷：错误分类 + 服务端的稍后重试指引（`Retry-After`
/// 头的秒数形态）。指引不进 [`GlossError`]——它是传输层重试的内部量，
/// 出了这层就是普通失败。
struct TransportFail {
    error: GlossError,
    retry_after: Option<Duration>,
}

/// 传输失败是否值得自动重试及等待时长（纯函数）：预算未尽且错误是瞬态
/// （网络/限流）才重试；等待取 `Retry-After` 指引与上限的较小值，无指引
/// 用固定退避。鉴权、参数与环境类拒绝重发无益，与预算耗尽同返回 None。
fn retry_delay(
    error: &GlossError,
    retry_after: Option<Duration>,
    retries_left: u32,
) -> Option<Duration> {
    if retries_left == 0 {
        return None;
    }
    match error {
        GlossError::EngineNetwork | GlossError::EngineRateLimited => {
            Some(retry_after.map_or(RETRY_BACKOFF, |wait| wait.min(RETRY_AFTER_CAP)))
        }
        _ => None,
    }
}

/// `Retry-After` 头的秒数形态解析（HTTP 日期形态返回 None，回落固定退避）。
fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

/// 空闲看门狗的到点时刻（纯函数）：`Sleep` 的初装与复位都用它给定时刻。
fn idle_deadline(now: TokioInstant) -> TokioInstant {
    now + IDLE_TIMEOUT
}

/// 发起一次传输尝试：端点、密钥与请求体已就绪（解析产物），发请求、查
/// 状态、包 SSE 流。失败以 [`TransportFail`] 上交（状态失败随附
/// `Retry-After` 指引）；重试判定归 [`connect_with_retry`] 与
/// [`FirstByteStream`]。
async fn send_once(
    client: &reqwest::Client,
    resolved: &Resolved,
) -> Result<TaskStream, TransportFail> {
    let mut response = client
        .post(&resolved.url)
        .bearer_auth(&resolved.key)
        // 请求体已在 [`Resolved::build`] 序列化成字节，这里只补 JSON 的
        // content-type（`.json` 的另一半职责），克隆只是递引用计数。
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(resolved.body.clone())
        .send()
        .await
        .map_err(|err| TransportFail {
            error: map_transport_error(err),
            retry_after: None,
        })?;

    let status = response.status();
    if !status.is_success() {
        // 服务端诊断文本（可能是 JSON 错误对象）：只取 message 字段，
        // 不转述整个响应体；密钥在请求头里，不会出现在这里。读取带上限
        // ——错误网关/WAF 可能回几百 MB 的劫持页，不能整份吃进内存。
        let retry_after = parse_retry_after(response.headers());
        let detail = read_bounded_body(&mut response, MAX_ERROR_BODY_BYTES).await;
        return Err(TransportFail {
            error: map_failure(status, &detail),
            retry_after,
        });
    }
    Ok(Box::pin(SseStream::new(response.bytes_stream())) as TaskStream)
}

/// 连接阶段的重试循环：逐次尝试，瞬态失败在预算内退避后重试，其余失败
/// 与预算耗尽原样上抛。预算余额随成功返回——首个流条目之前的重试
/// （[`FirstByteStream`]）接着用同一份预算。
async fn connect_with_retry<T>(
    mut attempt: impl FnMut() -> BoxFuture<'static, Result<T, TransportFail>>,
    mut retries_left: u32,
) -> (Result<T, GlossError>, u32) {
    loop {
        match attempt().await {
            Ok(stream) => return (Ok(stream), retries_left),
            Err(fail) => match retry_delay(&fail.error, fail.retry_after, retries_left) {
                Some(delay) => {
                    retries_left -= 1;
                    warn!(
                        thread = thread::TOKIO,
                        error = %fail.error,
                        delay = ?delay,
                        "llm request hit a transient failure, retrying before the first byte"
                    );
                    sleep(delay).await;
                }
                None => return (Err(fail.error), retries_left),
            },
        }
    }
}

/// 首个流条目之前的重试包装：内建流在产出**任何**条目之前的 `Err`，若是
/// 瞬态失败且预算未尽，丢弃空流、退避后重新发起请求（消费方无感）——
/// 此刻没有任何增量交付过，不存在半截产物要保。首个条目一旦交付（增量
/// 或终结条目），错误一律原样放行：中途断流多半是确定性故障，重头重播
/// 还会让已渲染的流式正文错乱。`attempt` 与连接阶段同形状，共用同一份
/// [`RETRY_ATTEMPTS`] 预算；重连只此一次、失败原样上交，不链式重判——
/// 现行预算下重连发出时余额必为 0，这一箭也是对失败端点的重试上限。
struct FirstByteStream<F> {
    attempt: F,
    inner: Option<TaskStream>,
    /// 进行中的重连（退避 + 重新请求）；在位时 `inner` 必为 None。
    connecting: Option<BoxFuture<'static, Result<TaskStream, TransportFail>>>,
    retries_left: u32,
    handed_first: bool,
}

impl<F> FirstByteStream<F>
where
    F: Fn() -> BoxFuture<'static, Result<TaskStream, TransportFail>>,
{
    fn new(attempt: F, inner: TaskStream, retries_left: u32) -> Self {
        Self {
            attempt,
            inner: Some(inner),
            connecting: None,
            retries_left,
            handed_first: false,
        }
    }
}

impl<F> Stream for FirstByteStream<F>
where
    F: Fn() -> BoxFuture<'static, Result<TaskStream, TransportFail>> + Unpin,
{
    type Item = Result<TaskChunk, GlossError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            if let Some(connect) = this.connecting.as_mut() {
                match connect.as_mut().poll(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Ok(stream)) => {
                        this.inner = Some(stream);
                        this.connecting = None;
                    }
                    Poll::Ready(Err(fail)) => {
                        this.connecting = None;
                        return Poll::Ready(Some(Err(fail.error)));
                    }
                }
                continue;
            }
            let Some(stream) = this.inner.as_mut() else {
                return Poll::Ready(None);
            };
            match stream.as_mut().poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Some(Ok(chunk))) => {
                    this.handed_first = true;
                    return Poll::Ready(Some(Ok(chunk)));
                }
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Ready(Some(Err(error))) => {
                    if this.handed_first {
                        return Poll::Ready(Some(Err(error)));
                    }
                    match retry_delay(&error, None, this.retries_left) {
                        Some(delay) => {
                            this.retries_left -= 1;
                            warn!(
                                thread = thread::TOKIO,
                                error = %error,
                                delay = ?delay,
                                "llm stream failed before the first byte, retrying"
                            );
                            this.inner = None;
                            let attempt = (this.attempt)();
                            this.connecting = Some(Box::pin(async move {
                                sleep(delay).await;
                                attempt.await
                            }));
                        }
                        None => return Poll::Ready(Some(Err(error))),
                    }
                }
            }
        }
    }
}

/// 请求端点：由配置快照解析 `{base_url}/chat/completions`。
fn resolve_endpoint(config: &Config) -> Result<String, GlossError> {
    let base_url = config.base_url.trim();
    // 结构性规则（非空/https/无凭据/无 query·fragment）与设置页逐字段
    // 校验共源：`crate::config::validate_base_url`。
    crate::config::validate_base_url(base_url)
        .map_err(|err| GlossError::Config(err.to_string()))?;
    let mut endpoint = reqwest::Url::parse(base_url)
        .map_err(|err| GlossError::Config(format!("invalid provider endpoint: {err}")))?;
    // 用 Url 设路径而不是字符串拼接：手改配置的尾斜杠写没写都得到同一结果。
    let path = format!(
        "{}/{CHAT_COMPLETIONS_PATH}",
        endpoint.path().trim_end_matches('/')
    );
    endpoint.set_path(&path);
    Ok(endpoint.to_string())
}

/// API 密钥：按 provider 条目的 keychain 标识**每请求直查**（不缓存，只进
/// 请求头）。未配置时返回 [`GlossError::EngineAuth`]，由 UI 引导去设置页。
fn resolve_api_key(config: &Config, store: &dyn ConfigStore) -> Result<String, GlossError> {
    let provider = config.resolved_provider();
    // trim 后再返回：从终端/文件复制粘贴进来的尾随换行会让 Authorization
    // 头非法（reqwest 报 builder 错 → 被当成网络问题的永久失败），尾随空格
    // 则变成服务端 401。
    store
        .secret(&provider.keychain_id)?
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty())
        .ok_or(GlossError::EngineAuth)
}

/// 传输层错误 → [`GlossError`]：连不上/超时/连接中断都可重试。
fn map_transport_error(err: reqwest::Error) -> GlossError {
    // builder 错是本地构造失败（非法 header 等）——重试永远不会成功，报成
    // EngineNetwork（可重试）会把用户引到错误的方向。
    if err.is_builder() {
        GlossError::EngineResponse(format!("request rejected: {err}"))
    } else if err.is_decode() {
        GlossError::EngineResponse(format!("decode response: {err}"))
    } else {
        GlossError::EngineNetwork
    }
}

/// JSON 模式不支持的诊断标记（小写比较）：请求体恒带 `response_format`
/// json_object，4xx 诊断文本提到它就是端点不认这个开关。
const JSON_MODE_MARKERS: [&str; 2] = ["response_format", "json_object"];

/// 图像输入不支持的诊断标记（小写比较）。
const MULTIMODAL_MARKERS: [&str; 3] = ["image", "multimodal", "vision"];

/// HTTP 状态 → [`GlossError`]（错误分类表）。其余 4xx 按服务端诊断文本做
/// 两个**启发式**识别（环境问题，重发同请求无益，不落可重试的协议错误）：
/// 诊断提到 JSON 模式开关 → [`GlossError::JsonModeUnsupported`]；提到图像
/// 输入 → [`GlossError::UnsupportedModality`]。JSON 模式检查在前：它是请求
/// 信封层的拒绝，命中时对所有任务都成立。识别不出才落
/// [`GlossError::EngineResponse`]。
fn map_failure(status: StatusCode, body: &str) -> GlossError {
    match status.as_u16() {
        401 | 403 => GlossError::EngineAuth,
        429 => GlossError::EngineRateLimited,
        500..=599 => GlossError::EngineNetwork,
        _ => {
            let message = server_message(body);
            let lowered = message.as_deref().unwrap_or("").to_lowercase();
            if JSON_MODE_MARKERS
                .iter()
                .any(|marker| lowered.contains(marker))
            {
                return GlossError::JsonModeUnsupported;
            }
            if MULTIMODAL_MARKERS
                .iter()
                .any(|marker| lowered.contains(marker))
            {
                return GlossError::UnsupportedModality;
            }
            match message {
                Some(message) => GlossError::EngineResponse(format!("HTTP {status}: {message}")),
                None => GlossError::EngineResponse(format!("HTTP {status}")),
            }
        }
    }
}

/// 从错误响应体里取服务端诊断文本（仅 `error.message`，OpenAI 兼容形状）。
fn server_message(body: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct ErrorEnvelope {
        error: ErrorBody,
    }
    #[derive(serde::Deserialize)]
    struct ErrorBody {
        message: String,
    }

    let envelope: ErrorEnvelope = serde_json::from_str(body).ok()?;
    let message = envelope.error.message;
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return None;
    }
    // 上限与 SSE 诊断一致：错误文本会进 UI 与日志。
    Some(trimmed.chars().take(200).collect())
}

/// 把响应的字节流适配成 [`TaskStream`]：SSE 逐行解码，吐文本增量，`[DONE]`
/// 上交一条携带结束原因的终结条目（[`TaskChunk::Done`]）。
///
/// 手动实现 `poll_next`（与编排层的手工轮询同一风格），不引 futures-util：
/// 状态机只有「攒够一行 → 产出条目 → 继续拉字节」三步。首个 `Err` 即终结流
/// （端口契约），此后不再拉取字节。
struct SseStream {
    inner: Pin<Box<dyn Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>>,
    decoder: SseDecoder,
    ready: VecDeque<Result<TaskChunk, GlossError>>,
    /// 是否产出过增量：零增量的「成功」是坏响应（网关忽略 `stream:true` 返回
    /// 非流式 JSON、劫持页返回 200 HTML、模型只回 refusal 等），不能当成功
    /// 产物写进缓存——用户会看到空白卡片，重复触发还命同一份空白。
    produced: bool,
    finished: bool,
    /// 空闲看门狗：每次收到字节即复位；到点未复位说明连接悬死（连着但不
    /// 来字节），轮询以 [`GlossError::EngineNetwork`] 终结流。
    idle: Pin<Box<Sleep>>,
}

impl SseStream {
    fn new(
        inner: impl Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send + 'static,
    ) -> Self {
        Self {
            inner: Box::pin(inner),
            decoder: SseDecoder::new(),
            ready: VecDeque::new(),
            produced: false,
            finished: false,
            idle: Box::pin(sleep_until(idle_deadline(TokioInstant::now()))),
        }
    }

    /// 收下解码器产出：增量入队，`Done` / `Failed` 终直流。
    fn absorb(&mut self, items: Vec<SseItem>) {
        for item in items {
            // 终结之后再无契约地位的字节（解码器对 [DONE] 后的输入同样沉默）。
            if self.finished {
                return;
            }
            match item {
                SseItem::Delta(delta) => {
                    self.produced = true;
                    self.ready.push_back(Ok(TaskChunk::Delta(delta)));
                }
                SseItem::Done { finish_reason } => {
                    if self.produced {
                        self.ready.push_back(Ok(TaskChunk::Done { finish_reason }));
                    } else {
                        // 零增量的「成功」是坏响应，即便服务端声明了正常结束。
                        self.ready
                            .push_back(Err(GlossError::EngineResponse(EMPTY_COMPLETION.into())));
                    }
                    self.finished = true;
                }
                SseItem::Failed(error) => {
                    self.ready.push_back(Err(error));
                    self.finished = true;
                }
            }
        }
    }

    /// 连接关闭的收尾（服务端没给 `[DONE]`）：一条增量都没产出过就是坏
    /// 响应；产出过则按流自然耗尽收场（结束原因未知，不产出 Done）。
    /// `ready` 先于 `finished` 被排空，所以这条 Err 一定排在已入队的增量之后。
    fn finish_without_error(&mut self) {
        if !self.produced {
            self.ready
                .push_back(Err(GlossError::EngineResponse(EMPTY_COMPLETION.into())));
        }
        self.finished = true;
    }
}

impl Stream for SseStream {
    type Item = Result<TaskChunk, GlossError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            if let Some(item) = this.ready.pop_front() {
                return Poll::Ready(Some(item));
            }
            if this.finished {
                return Poll::Ready(None);
            }
            // 看门狗先于内层轮询：连接悬死时内层恒 Pending，唯一能终结流的
            // 就是它。已入队的增量先交付（末次字节到达时看门狗已复位，能
            // 走到这说明静默期确实过满了）。
            if this.idle.as_mut().poll(cx).is_ready() {
                this.finished = true;
                warn!(
                    thread = thread::TOKIO,
                    idle_secs = IDLE_TIMEOUT.as_secs(),
                    "llm stream went silent, failing with a network error"
                );
                return Poll::Ready(Some(Err(GlossError::EngineNetwork)));
            }
            match this.inner.as_mut().poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Some(Ok(chunk))) => {
                    this.idle.as_mut().reset(idle_deadline(TokioInstant::now()));
                    let items = this.decoder.push(&chunk);
                    this.absorb(items);
                }
                Poll::Ready(Some(Err(err))) => {
                    // 连接断了：把错误排在已收下的增量之后（端口契约是「首个
                    // Err 终结流」，之前到达的增量仍应交付）。
                    this.ready.push_back(Err(map_transport_error(err)));
                    this.finished = true;
                }
                Poll::Ready(None) => {
                    // 连接关闭即流结束：服务端没给 [DONE] 也按正常收尾，
                    // 顺便收下尾巴里那条完整但没换行终止的增量。
                    let tail = this.decoder.finish();
                    this.absorb(tail);
                    this.finish_without_error();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex;

    use crate::stubs::ports::MemoryConfigStore;

    const FIXTURE_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0B, 0x49, 0x44, 0x41, 0x54, 0x78, 0xDA, 0x63, 0x60,
        0x00, 0x02, 0x00, 0x00, 0x05, 0x00, 0x01, 0xE9, 0xFA, 0xDC, 0xD8, 0x00, 0x00, 0x00, 0x00,
        0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    fn fixture(secret: Option<&str>) -> (Arc<ConfigHandle>, Arc<MemoryConfigStore>) {
        let store = Arc::new(MemoryConfigStore::default());
        if let Some(secret) = secret {
            store
                .set_secret("gloss/deepseek", secret)
                .expect("stub store accepts secret");
        }
        (
            Arc::new(ConfigHandle::with_config(store.clone(), Config::default())),
            store,
        )
    }

    #[test]
    fn resolves_endpoint_from_config() {
        let config = Config::default();
        assert_eq!(
            resolve_endpoint(&config).expect("resolve"),
            format!("{}/chat/completions", crate::config::DEFAULT_BASE_URL)
        );
    }

    #[test]
    fn endpoint_trimming_avoids_double_slash() {
        let config = Config {
            base_url: "https://example.test/v1/".into(),
            ..Default::default()
        };
        assert_eq!(
            resolve_endpoint(&config).expect("resolve"),
            "https://example.test/v1/chat/completions"
        );
    }

    #[test]
    fn cleartext_endpoints_are_rejected() {
        for base_url in [
            "http://api.example.test/v1",
            "http://localhost:11434/v1",
            "http://127.0.0.1:8080/v1",
        ] {
            let config = Config {
                base_url: base_url.into(),
                ..Default::default()
            };
            assert!(
                matches!(resolve_endpoint(&config), Err(GlossError::Config(_))),
                "cleartext endpoint must be rejected: {base_url}"
            );
        }
    }

    #[test]
    fn endpoints_with_credentials_query_or_fragment_are_rejected() {
        for base_url in [
            "https://user:pass@api.example.test/v1",
            "https://user@api.example.test/v1",
            "https://api.example.test/v1?key=x",
            "https://api.example.test/v1#frag",
        ] {
            let config = Config {
                base_url: base_url.into(),
                ..Default::default()
            };
            assert!(
                matches!(resolve_endpoint(&config), Err(GlossError::Config(_))),
                "endpoint must be rejected: {base_url}"
            );
        }
    }

    #[test]
    fn unusable_endpoints_report_config_error() {
        for base_url in ["", "   ", "file:///tmp/v1", "api.example.test/v1"] {
            let config = Config {
                base_url: base_url.into(),
                ..Default::default()
            };
            assert!(
                matches!(resolve_endpoint(&config), Err(GlossError::Config(_))),
                "endpoint {base_url:?} must be rejected"
            );
        }
    }

    #[test]
    fn resolves_key_from_keychain() {
        let (handle, store) = fixture(Some("test-key-value"));
        assert_eq!(
            resolve_api_key(&handle.snapshot(), store.as_ref()).expect("resolve"),
            "test-key-value"
        );
    }

    #[test]
    fn missing_key_reports_auth_error() {
        let (handle, store) = fixture(None);
        assert_eq!(
            resolve_api_key(&handle.snapshot(), store.as_ref()).expect_err("missing secret"),
            GlossError::EngineAuth
        );
    }

    #[test]
    fn blank_key_counts_as_missing() {
        let (handle, store) = fixture(Some("   "));
        assert_eq!(
            resolve_api_key(&handle.snapshot(), store.as_ref()).expect_err("blank secret"),
            GlossError::EngineAuth
        );
    }

    #[test]
    fn empty_provider_list_falls_back_to_the_factory_entry() {
        let store = Arc::new(MemoryConfigStore::default());
        store
            .set_secret("gloss/deepseek", "legacy-key")
            .expect("stub store accepts secret");
        let config = Config {
            provider_keys: Vec::new(),
            ..Default::default()
        };
        assert!(config.active_provider().is_none(), "纯查表仍应为空");
        assert_eq!(
            resolve_api_key(&config, store.as_ref()).expect("factory fallback"),
            "legacy-key"
        );
        assert_eq!(config.resolved_provider().keychain_id, "gloss/deepseek");
    }

    #[tokio::test]
    async fn adapter_streams_deltas_across_chunk_boundaries() {
        let payload = concat!(
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"光\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"泽\"}}]}\n\n",
            "data: [DONE]\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"多余\"}}]}\n",
        )
        .as_bytes();

        for chunk_size in [1, 2, 5, payload.len()] {
            let chunks: Vec<_> = payload
                .chunks(chunk_size)
                .map(|chunk| Ok(bytes::Bytes::copy_from_slice(chunk)))
                .collect();
            let mut adapted = SseStream::new(VecStream(chunks.into_iter()));

            let mut text = String::new();
            while let Some(item) =
                std::future::poll_fn(|cx| Pin::new(&mut adapted).poll_next(cx)).await
            {
                match item.expect("delta expected") {
                    TaskChunk::Delta(delta) => text.push_str(&delta),
                    TaskChunk::Done { .. } => {}
                }
            }
            assert_eq!(text, "光泽", "chunk_size = {chunk_size}");
        }
    }

    #[tokio::test]
    async fn adapter_delivers_the_done_marker_with_its_finish_reason() {
        let payload = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"半\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        );
        let mut adapted = SseStream::new(VecStream(
            vec![Ok(bytes::Bytes::from_static(payload.as_bytes()))].into_iter(),
        ));

        let mut items = Vec::new();
        while let Some(item) = std::future::poll_fn(|cx| Pin::new(&mut adapted).poll_next(cx)).await
        {
            items.push(item);
        }
        assert_eq!(
            items,
            vec![
                Ok(TaskChunk::Delta("半".into())),
                Ok(TaskChunk::Done {
                    finish_reason: Some("stop".into())
                }),
            ],
            "the declared end is delivered as the terminal chunk with its reason"
        );
    }

    #[tokio::test]
    async fn adapter_delivers_deltas_then_terminates_on_protocol_error() {
        let payload = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"半\"}}]}\n\n",
            "data: not json\n",
        );
        let mut adapted = SseStream::new(VecStream(
            vec![Ok(bytes::Bytes::from_static(payload.as_bytes()))].into_iter(),
        ));

        let mut items = Vec::new();
        while let Some(item) = std::future::poll_fn(|cx| Pin::new(&mut adapted).poll_next(cx)).await
        {
            items.push(item);
        }
        assert_eq!(items.len(), 2, "delta then one error: {items:?}");
        assert_eq!(items[0], Ok(TaskChunk::Delta("半".into())));
        assert!(
            matches!(items[1], Err(GlossError::EngineResponse(_))),
            "protocol error must terminate the stream as a failure"
        );
    }

    #[tokio::test]
    async fn adapter_ends_cleanly_without_the_done_marker() {
        let payload = "data: {\"choices\":[{\"delta\":{\"content\":\"尾\"}}]}\n";
        let mut adapted = SseStream::new(VecStream(
            vec![Ok(bytes::Bytes::from_static(payload.as_bytes()))].into_iter(),
        ));

        let mut text = String::new();
        while let Some(item) = std::future::poll_fn(|cx| Pin::new(&mut adapted).poll_next(cx)).await
        {
            match item.expect("delta expected") {
                TaskChunk::Delta(delta) => text.push_str(&delta),
                TaskChunk::Done { .. } => {}
            }
        }
        assert_eq!(text, "尾");
    }

    #[tokio::test]
    async fn adapter_reports_empty_completion_as_failure() {
        for payload in [
            "data: [DONE]\n\n",
            "<html>nope</html>\n",
            concat!(
                "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                "data: [DONE]\n\n",
            ),
        ] {
            let mut adapted = SseStream::new(VecStream(
                vec![Ok(bytes::Bytes::from_static(payload.as_bytes()))].into_iter(),
            ));
            let item = std::future::poll_fn(|cx| Pin::new(&mut adapted).poll_next(cx))
                .await
                .expect("one item expected");
            assert!(
                matches!(item, Err(GlossError::EngineResponse(_))),
                "payload {payload:?} must fail: {item:?}"
            );
            assert!(
                std::future::poll_fn(|cx| Pin::new(&mut adapted).poll_next(cx))
                    .await
                    .is_none(),
                "stream must end after the failure"
            );
        }
    }

    #[test]
    fn request_body_has_the_openai_envelope() {
        let request = EngineRequest {
            messages: vec![ChatMessage::system("把用户给的词翻成中文")],
            model: "deepseek-chat".into(),
            max_tokens: None,
        };
        assert_eq!(
            chat_request_body(&request),
            serde_json::json!({
                "model": "deepseek-chat",
                "messages": [{"role": "system", "content": "把用户给的词翻成中文"}],
                "stream": true,
                "response_format": {"type": "json_object"},
            })
        );
    }

    #[test]
    fn json_mode_switch_is_always_on_the_wire() {
        let text = chat_request_body(&EngineRequest {
            messages: vec![ChatMessage::user("hi")],
            model: "m".into(),
            max_tokens: Some(64),
        });
        assert_eq!(
            text.get("response_format"),
            Some(&serde_json::json!({"type": "json_object"})),
            "every request carries the JSON contract, capped or not"
        );

        let multimodal = chat_request_body(&EngineRequest {
            messages: vec![ChatMessage::user_parts(vec![
                ContentPart::Text("这张图里有什么".into()),
                ContentPart::ImagePng(FIXTURE_PNG.into()),
            ])],
            model: "m".into(),
            max_tokens: None,
        });
        assert_eq!(
            multimodal.get("response_format"),
            Some(&serde_json::json!({"type": "json_object"})),
            "image tasks are under the same JSON contract"
        );
    }

    #[test]
    fn text_content_stays_a_json_string_on_the_wire() {
        let request = EngineRequest {
            messages: vec![ChatMessage::user("hi")],
            model: "m".into(),
            max_tokens: None,
        };
        let body = chat_request_body(&request);
        assert_eq!(
            body["messages"][0],
            serde_json::json!({"role": "user", "content": "hi"}),
            "a plain-text message must not become a content array"
        );
    }

    #[test]
    fn multimodal_parts_map_to_the_openai_content_array() {
        let request = EngineRequest {
            messages: vec![
                ChatMessage::system("解读图片"),
                ChatMessage::user_parts(vec![
                    ContentPart::Text("这张图里有什么".into()),
                    ContentPart::ImagePng(FIXTURE_PNG.into()),
                ]),
            ],
            model: "deepseek-flash".into(),
            max_tokens: None,
        };
        assert_eq!(
            chat_request_body(&request),
            serde_json::json!({
                "model": "deepseek-flash",
                "messages": [
                    {"role": "system", "content": "解读图片"},
                    {"role": "user", "content": [
                        {"type": "text", "text": "这张图里有什么"},
                        {"type": "image_url", "image_url": {"url": concat!(
                            "data:image/png;base64,",
                            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAA",
                            "AAC0lEQVR42mNgAAIAAAUAAen63NgAAAAASUVORK5CYII=",
                        )}},
                    ]},
                ],
                "stream": true,
                "response_format": {"type": "json_object"},
            })
        );
    }

    #[test]
    fn max_tokens_is_carried_only_when_set() {
        let messages = vec![ChatMessage::user("hi")];
        let unset = chat_request_body(&EngineRequest {
            messages: messages.clone(),
            model: "m".into(),
            max_tokens: None,
        });
        assert!(
            unset.get("max_tokens").is_none(),
            "an unset limit must stay off the wire, not null"
        );
        let capped = chat_request_body(&EngineRequest {
            messages,
            model: "m".into(),
            max_tokens: Some(64),
        });
        assert_eq!(capped.get("max_tokens"), Some(&serde_json::json!(64)));
    }

    struct VecStream(std::vec::IntoIter<Result<bytes::Bytes, reqwest::Error>>);

    impl Stream for VecStream {
        type Item = Result<bytes::Bytes, reqwest::Error>;

        fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Poll::Ready(self.0.next())
        }
    }

    #[test]
    fn maps_http_status_to_error_variants() {
        assert_eq!(
            map_failure(StatusCode::UNAUTHORIZED, ""),
            GlossError::EngineAuth
        );
        assert_eq!(
            map_failure(StatusCode::FORBIDDEN, ""),
            GlossError::EngineAuth
        );
        assert_eq!(
            map_failure(StatusCode::TOO_MANY_REQUESTS, ""),
            GlossError::EngineRateLimited
        );
        assert_eq!(
            map_failure(StatusCode::INTERNAL_SERVER_ERROR, ""),
            GlossError::EngineNetwork,
            "server-side failure is retryable"
        );

        let err = map_failure(
            StatusCode::BAD_REQUEST,
            r#"{"error":{"message":"model `x` does not exist","type":"invalid_request_error"}}"#,
        );
        assert_eq!(
            err,
            GlossError::EngineResponse("HTTP 400 Bad Request: model `x` does not exist".into())
        );
        assert_eq!(
            map_failure(StatusCode::BAD_REQUEST, "<html>nope</html>"),
            GlossError::EngineResponse("HTTP 400 Bad Request".into())
        );
    }

    #[test]
    fn json_mode_and_multimodal_rejections_are_recognized_from_diagnostics() {
        assert_eq!(
            map_failure(
                StatusCode::BAD_REQUEST,
                r#"{"error":{"message":"response_format is not supported by this model","type":"invalid_request_error"}}"#,
            ),
            GlossError::JsonModeUnsupported
        );
        assert_eq!(
            map_failure(
                StatusCode::BAD_REQUEST,
                r#"{"error":{"message":"json_object output is unavailable on this endpoint"}}"#,
            ),
            GlossError::JsonModeUnsupported,
            "either marker of the JSON-mode switch is enough"
        );
        assert_eq!(
            map_failure(
                StatusCode::UNPROCESSABLE_ENTITY,
                r#"{"error":{"message":"This model does not support image input (vision)."}}"#,
            ),
            GlossError::UnsupportedModality
        );
        assert_eq!(
            map_failure(
                StatusCode::BAD_REQUEST,
                r#"{"error":{"message":"`messages` contains an unsupported multimodal content part"}}"#,
            ),
            GlossError::UnsupportedModality
        );
        assert_eq!(
            map_failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                r#"{"error":{"message":"vision routing exploded","type":"server_error"}}"#,
            ),
            GlossError::EngineNetwork,
            "the diagnostics heuristics are scoped to 4xx"
        );
        assert_eq!(
            map_failure(
                StatusCode::UNAUTHORIZED,
                r#"{"error":{"message":"invalid response_format"}}"#,
            ),
            GlossError::EngineAuth,
            "auth keeps its precedence over the diagnostics"
        );
    }

    #[test]
    fn retry_delay_covers_only_transient_failures_within_budget() {
        assert_eq!(
            retry_delay(&GlossError::EngineNetwork, None, RETRY_ATTEMPTS),
            Some(RETRY_BACKOFF)
        );
        assert_eq!(
            retry_delay(&GlossError::EngineRateLimited, None, RETRY_ATTEMPTS),
            Some(RETRY_BACKOFF)
        );
        assert_eq!(
            retry_delay(&GlossError::EngineNetwork, Some(Duration::from_secs(5)), 1),
            Some(RETRY_AFTER_CAP),
            "guidance beyond the cap is capped"
        );
        assert_eq!(
            retry_delay(
                &GlossError::EngineNetwork,
                Some(Duration::from_millis(100)),
                1
            ),
            Some(Duration::from_millis(100)),
            "guidance under the cap wins over the fixed backoff"
        );
        assert_eq!(
            retry_delay(&GlossError::EngineNetwork, None, 0),
            None,
            "an exhausted budget never retries"
        );
        assert_eq!(retry_delay(&GlossError::EngineAuth, None, 1), None);
        assert_eq!(
            retry_delay(&GlossError::EngineResponse("bad reply".into()), None, 1),
            None
        );
        assert_eq!(retry_delay(&GlossError::Config("x".into()), None, 1), None);
    }

    #[test]
    fn retry_after_header_parsing_accepts_seconds_only() {
        let mut headers = reqwest::header::HeaderMap::new();
        assert_eq!(parse_retry_after(&headers), None);

        headers.insert(
            reqwest::header::RETRY_AFTER,
            "2".parse().expect("header value"),
        );
        assert_eq!(parse_retry_after(&headers), Some(Duration::from_secs(2)));

        headers.insert(
            reqwest::header::RETRY_AFTER,
            " 1 ".parse().expect("header value"),
        );
        assert_eq!(
            parse_retry_after(&headers),
            Some(Duration::from_secs(1)),
            "padding around the number is tolerated"
        );

        headers.insert(
            reqwest::header::RETRY_AFTER,
            "Wed, 21 Oct 2015 07:28:00 GMT"
                .parse()
                .expect("header value"),
        );
        assert_eq!(
            parse_retry_after(&headers),
            None,
            "the HTTP-date form falls back to the fixed backoff"
        );

        headers.insert(
            reqwest::header::RETRY_AFTER,
            "soon".parse().expect("header value"),
        );
        assert_eq!(parse_retry_after(&headers), None);
    }

    #[tokio::test]
    async fn connect_retries_transient_failures_within_the_budget() {
        tokio::time::pause();
        let (attempt, calls) = queue_attempt(vec![
            Err(TransportFail {
                error: GlossError::EngineNetwork,
                retry_after: None,
            }),
            Ok(()),
        ]);
        let (result, retries_left) = connect_with_retry(attempt, RETRY_ATTEMPTS).await;
        assert!(result.is_ok(), "the second attempt lands");
        assert_eq!(*calls.lock().unwrap(), 2);
        assert_eq!(retries_left, 0);

        let (attempt, calls) = queue_attempt::<()>(vec![
            Err(TransportFail {
                error: GlossError::EngineNetwork,
                retry_after: None,
            }),
            Err(TransportFail {
                error: GlossError::EngineRateLimited,
                retry_after: None,
            }),
        ]);
        let (result, retries_left) = connect_with_retry(attempt, RETRY_ATTEMPTS).await;
        assert!(
            matches!(result, Err(GlossError::EngineRateLimited)),
            "the last failure is reported as-is: {result:?}"
        );
        assert_eq!(*calls.lock().unwrap(), 2);
        assert_eq!(retries_left, 0);

        let (attempt, calls) = queue_attempt::<()>(vec![Err(TransportFail {
            error: GlossError::EngineAuth,
            retry_after: None,
        })]);
        let (result, retries_left) = connect_with_retry(attempt, RETRY_ATTEMPTS).await;
        assert!(matches!(result, Err(GlossError::EngineAuth)));
        assert_eq!(*calls.lock().unwrap(), 1);
        assert_eq!(retries_left, RETRY_ATTEMPTS);
    }

    #[tokio::test]
    async fn first_byte_failures_retry_within_the_shared_budget() {
        tokio::time::pause();
        let good = chunk_stream(vec![
            Ok(TaskChunk::Delta("光".into())),
            Ok(TaskChunk::Done {
                finish_reason: Some("stop".into()),
            }),
        ]);
        let (attempt, calls) = queue_attempt(vec![Ok(good)]);
        let mut stream = FirstByteStream::new(
            attempt,
            chunk_stream(vec![Err(GlossError::EngineNetwork)]),
            RETRY_ATTEMPTS,
        );
        assert_eq!(
            next_item(&mut stream).await,
            Some(Ok(TaskChunk::Delta("光".into()))),
            "the retry is invisible to the consumer: no failure item is delivered"
        );
        assert_eq!(
            next_item(&mut stream).await,
            Some(Ok(TaskChunk::Done {
                finish_reason: Some("stop".into())
            }))
        );
        assert_eq!(next_item(&mut stream).await, None);
        assert_eq!(*calls.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn first_byte_budget_exhaustion_passes_the_error_through() {
        tokio::time::pause();
        let (attempt, calls) =
            queue_attempt(vec![Ok(chunk_stream(vec![Err(GlossError::EngineNetwork)]))]);
        let mut stream = FirstByteStream::new(
            attempt,
            chunk_stream(vec![Err(GlossError::EngineNetwork)]),
            RETRY_ATTEMPTS,
        );
        assert_eq!(
            next_item(&mut stream).await,
            Some(Err(GlossError::EngineNetwork)),
            "both attempts failed, the last error is delivered as-is"
        );
        assert_eq!(next_item(&mut stream).await, None);
        assert_eq!(*calls.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn non_retryable_first_byte_errors_pass_through_without_a_retry() {
        tokio::time::pause();
        let (attempt, calls) = queue_attempt(vec![]);
        let mut stream = FirstByteStream::new(
            attempt,
            chunk_stream(vec![Err(GlossError::EngineAuth)]),
            RETRY_ATTEMPTS,
        );
        assert_eq!(
            next_item(&mut stream).await,
            Some(Err(GlossError::EngineAuth))
        );
        assert_eq!(next_item(&mut stream).await, None);
        assert_eq!(*calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn errors_after_the_first_item_are_never_retried() {
        tokio::time::pause();
        let (attempt, calls) = queue_attempt(vec![]);
        let mut stream = FirstByteStream::new(
            attempt,
            chunk_stream(vec![
                Ok(TaskChunk::Delta("光".into())),
                Err(GlossError::EngineNetwork),
            ]),
            RETRY_ATTEMPTS,
        );
        assert_eq!(
            next_item(&mut stream).await,
            Some(Ok(TaskChunk::Delta("光".into())))
        );
        assert_eq!(
            next_item(&mut stream).await,
            Some(Err(GlossError::EngineNetwork)),
            "a mid-stream failure must not restart the request"
        );
        assert_eq!(next_item(&mut stream).await, None);
        assert_eq!(*calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn first_byte_retry_honors_a_zero_leftover_budget() {
        tokio::time::pause();
        let (attempt, calls) = queue_attempt(vec![]);
        let mut stream = FirstByteStream::new(
            attempt,
            chunk_stream(vec![Err(GlossError::EngineNetwork)]),
            0,
        );
        assert_eq!(
            next_item(&mut stream).await,
            Some(Err(GlossError::EngineNetwork)),
            "the connect stage spent the budget, the stream stage gets none"
        );
        assert_eq!(*calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn an_idle_stream_fails_after_the_silence_budget() {
        tokio::time::pause();
        let payload = "data: {\"choices\":[{\"delta\":{\"content\":\"光\"}}]}\n\n";
        let mut stream = SseStream::new(StallStream::new(bytes::Bytes::from_static(
            payload.as_bytes(),
        )));
        assert_eq!(
            next_item(&mut stream).await,
            Some(Ok(TaskChunk::Delta("光".into())))
        );
        assert_eq!(
            next_item(&mut stream).await,
            Some(Err(GlossError::EngineNetwork)),
            "a stalled connection ends the stream as a network failure"
        );
        assert_eq!(next_item(&mut stream).await, None);
    }

    #[tokio::test]
    async fn arriving_bytes_postpone_the_idle_deadline() {
        tokio::time::pause();
        let mut stream = SseStream::new(TimedChunks::new());
        for step in 0..3 {
            assert_eq!(
                next_item(&mut stream).await,
                Some(Ok(TaskChunk::Delta(step.to_string()))),
                "chunk {step} lands beyond the original deadline: arrivals reset the watchdog"
            );
        }
    }

    fn chunk_stream(items: Vec<Result<TaskChunk, GlossError>>) -> TaskStream {
        Box::pin(VecChunkStream(VecDeque::from(items)))
    }

    async fn next_item<S>(stream: &mut S) -> Option<Result<TaskChunk, GlossError>>
    where
        S: Stream<Item = Result<TaskChunk, GlossError>> + Unpin,
    {
        std::future::poll_fn(|cx| Pin::new(&mut *stream).poll_next(cx)).await
    }

    fn queue_attempt<T: Send + 'static>(
        outcomes: Vec<Result<T, TransportFail>>,
    ) -> (
        impl Fn() -> BoxFuture<'static, Result<T, TransportFail>>,
        Arc<Mutex<usize>>,
    ) {
        let queue = Arc::new(Mutex::new(VecDeque::from(outcomes)));
        let calls = Arc::new(Mutex::new(0usize));
        let attempt_queue = Arc::clone(&queue);
        let attempt_calls = Arc::clone(&calls);
        let attempt = move || {
            *attempt_calls.lock().unwrap() += 1;
            let outcome = attempt_queue
                .lock()
                .unwrap()
                .pop_front()
                .expect("attempt outcome queued");
            Box::pin(async move { outcome }) as BoxFuture<'static, Result<T, TransportFail>>
        };
        (attempt, calls)
    }

    struct VecChunkStream(VecDeque<Result<TaskChunk, GlossError>>);

    impl Stream for VecChunkStream {
        type Item = Result<TaskChunk, GlossError>;

        fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Poll::Ready(self.0.pop_front())
        }
    }

    struct StallStream(Option<bytes::Bytes>);

    impl StallStream {
        fn new(chunk: bytes::Bytes) -> Self {
            Self(Some(chunk))
        }
    }

    impl Stream for StallStream {
        type Item = Result<bytes::Bytes, reqwest::Error>;

        fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            match self.0.take() {
                Some(chunk) => Poll::Ready(Some(Ok(chunk))),
                None => Poll::Pending,
            }
        }
    }

    struct TimedChunks {
        step: u32,
        next: Pin<Box<Sleep>>,
    }

    impl TimedChunks {
        fn new() -> Self {
            Self {
                step: 0,
                next: Box::pin(sleep(Duration::from_secs(30))),
            }
        }
    }

    impl Stream for TimedChunks {
        type Item = Result<bytes::Bytes, reqwest::Error>;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            if self.next.as_mut().poll(cx).is_ready() {
                self.next
                    .as_mut()
                    .reset(TokioInstant::now() + Duration::from_secs(30));
                let payload = format!(
                    "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{}\"}}}}]}}\n\n",
                    self.step
                );
                self.step += 1;
                return Poll::Ready(Some(Ok(bytes::Bytes::from(payload))));
            }
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    use crate::prompt::PromptRegistry;
    use crate::schema;
    use crate::stubs::ports::MemoryConfigStore;
    use crate::task::{Task, TaskInput, TaskKind, TaskOptions};

    const ONE_PIXEL_PNG: [u8; 70] = [
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 218, 99, 100, 96, 248, 95, 15,
        0, 2, 135, 1, 128, 235, 71, 186, 146, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];

    fn require_live_env() -> (String, String, String) {
        let read = |name: &str| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
        };
        let (Some(base_url), Some(model), Some(key)) = (
            read("GLOSS_LIVE_BASE_URL"),
            read("GLOSS_LIVE_MODEL"),
            read("GLOSS_LIVE_API_KEY"),
        ) else {
            panic!(
                "真机测试缺少环境变量。修复：GLOSS_LIVE_API_KEY=sk-... \
                 GLOSS_LIVE_BASE_URL=https://api.deepseek.com/v1 \
                 GLOSS_LIVE_MODEL=deepseek-flash \
                 cargo test -p gloss-core -- --ignored live_llm"
            );
        };
        (base_url, model, key)
    }

    #[tokio::test]
    #[ignore = "真机：需 GLOSS_LIVE_API_KEY / GLOSS_LIVE_BASE_URL / GLOSS_LIVE_MODEL（见本模块文档）"]
    async fn live_llm_streams_a_translation() {
        let (base_url, model, key) = require_live_env();
        let store = Arc::new(MemoryConfigStore::default());
        store
            .set_secret("gloss/deepseek", &key)
            .expect("stub store accepts secret");
        let config = Config {
            base_url,
            model: model.clone(),
            ..Default::default()
        };
        let handle = Arc::new(ConfigHandle::with_config(store.clone(), config));
        let client = LlmClient::new(handle, store).expect("http client should build");

        let request = EngineRequest {
            messages: vec![
                ChatMessage::system("把用户给的句子翻成中文，只输出译文。"),
                ChatMessage::user("The quick brown fox jumps over the lazy dog."),
            ],
            model,
            max_tokens: None,
        };

        let mut stream = client.execute(request).await.expect("request should start");
        let mut text = String::new();
        while let Some(item) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
            match item {
                Ok(TaskChunk::Delta(delta)) => text.push_str(&delta),
                Ok(TaskChunk::Done { .. }) => {}
                Err(error) => panic!("stream failed: {error}"),
            }
            if text.chars().count() > 80 {
                break;
            }
        }
        assert!(
            !text.trim().is_empty(),
            "expected streamed translation text"
        );
    }

    #[tokio::test]
    #[ignore = "真机：需 GLOSS_LIVE_* 与支持图像输入的模型（bdd.md 人工测试有照抄步骤）"]
    async fn live_llm_streams_an_image_contract() {
        let (base_url, model, key) = require_live_env();
        let store = Arc::new(MemoryConfigStore::default());
        store
            .set_secret("gloss/deepseek", &key)
            .expect("stub store accepts secret");
        let config = Config {
            base_url,
            model: model.clone(),
            ..Default::default()
        };
        let handle = Arc::new(ConfigHandle::with_config(store.clone(), config));
        let client = LlmClient::new(handle, store).expect("http client should build");

        let task = Task {
            kind: TaskKind::ImageExplain,
            input: TaskInput::Image {
                png: Arc::from(&ONE_PIXEL_PNG[..]),
                region: None,
            },
            options: TaskOptions::default(),
        };
        let messages = PromptRegistry::new()
            .render(&task)
            .expect("image task passes the modality check");

        let mut stream = client
            .execute(EngineRequest {
                messages,
                model,
                max_tokens: None,
            })
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "真机图像请求失败（模型须支持图像输入与 JSON 输出模式）：{error}；\
                     修复：把 GLOSS_LIVE_MODEL 换成支持 vision 的模型后重跑"
                )
            });
        let mut text = String::new();
        while let Some(item) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
            match item {
                Ok(TaskChunk::Delta(delta)) => text.push_str(&delta),
                Ok(TaskChunk::Done { .. }) => {}
                Err(error) => panic!("stream failed: {error}"),
            }
        }
        let (note, _) =
            schema::validate_task(TaskKind::ImageExplain, &text).unwrap_or_else(|violation| {
                panic!(
                    "真机图像回复未过契约校验（{violation}）：端点未按 JSON 模式输出完整图注契约"
                )
            });
        assert!(
            !note.trim().is_empty(),
            "the streamed note must be non-empty"
        );
    }
}
