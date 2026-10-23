//! 一次传输尝试与响应流的适配：请求发送（状态检查、错误体上限读取）、
//! 瞬态失败判定与重试调度、「首个流条目之前」的自动重连，以及 SSE 响应
//! 到 [`TaskStream`] 的适配（空闲看门狗内建）。
//!
//! 重试只覆盖**首个流条目之前**的瞬态失败（[`RETRY_ATTEMPTS`]，仅网络/
//! 限流类）；连接阶段与流阶段共用同一份预算——前一阶段耗尽，后一阶段
//! 不再重试。取消由调用方丢弃 future 完成。

use std::collections::VecDeque;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use crate::engine::sse::{SseDecoder, SseItem};
use crate::log::{debug, thread, warn};
use crate::model::GlossError;
use crate::ports::{BoxFuture, EngineRequest, TaskChunk, TaskStream};
use bytes::Bytes;
use futures_core::Stream;
use tokio::time::{Instant as TokioInstant, Sleep, sleep, sleep_until};

use super::failure::{MAX_ERROR_BODY_BYTES, map_failure, map_transport_error};
use super::request_body::chat_request_body;

/// 传输失败自动重试预算：只重试**首个流条目之前**的瞬态失败（请求没送达、
/// 服务端明确说稍后再来、连接初建即断）。连接阶段与首个流条目之前两个
/// 阶段共用同一份预算——前一阶段耗尽，后一阶段不再重试。
pub(super) const RETRY_ATTEMPTS: u32 = 1;

/// 无 `Retry-After` 指引时的固定退避。
pub(super) const RETRY_BACKOFF: Duration = Duration::from_millis(500);

/// `Retry-After` 指引的等待上限：更长的等待不自动进行——失败卡交还用户，
/// 重试按钮在手（状态机对网络/限流错误的既有出口）。
pub(super) const RETRY_AFTER_CAP: Duration = Duration::from_secs(1);

/// 流式空闲超时：连续这么久没有任何字节到达即判传输停摆，流以
/// [`GlossError::EngineNetwork`] 终结（首个 `Err` 终结流的端口契约不变）。
/// 取值从宽以覆盖首字节等待——推理类模型出首个字节可以远慢于普通请求
/// ——也覆盖流中途的服务端生成间歇；只惩罚「连接还在、字节不来」的静默
/// 悬死，对长回复没有总超时那样的合法误杀。
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// 读响应体但不超过 `limit` 字节（丢失的只是诊断文本，不是业务数据）。
/// 读取中断不当作「正常读完」静默截断：留一条 debug，错误体按已读部分
/// 交给诊断解析（半截 JSON 会退化成「只报状态码」）。
async fn read_bounded_body(response: &mut reqwest::Response, limit: usize) -> String {
    let mut body = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                body.extend_from_slice(&chunk);
                if body.len() >= limit {
                    break;
                }
            }
            Ok(None) => break,
            Err(error) => {
                debug!(
                    thread = thread::TOKIO,
                    error = %error,
                    "error body read stopped before the limit"
                );
                break;
            }
        }
    }
    String::from_utf8_lossy(&body).into_owned()
}

/// 一次请求的解析产物：端点、密钥与 wire 请求体。请求体在构造处一次性
/// 序列化成 `Bytes`（图像任务的 base64 可达 MB 级，`.json(&Value)` 会在
/// 每次尝试时整体重序列化一份临时缓冲）；`Bytes` 克隆只递引用计数，重试
/// 复用零拷贝。
pub(super) struct Resolved {
    url: String,
    key: String,
    body: Bytes,
}

impl Resolved {
    /// 组装并序列化请求体。序列化失败只可能是本地构造出非对象形态的
    /// `Value`（请求体构造恒产出 JSON 对象），按请求异常上抛。
    pub(super) fn build(
        url: String,
        key: String,
        request: &EngineRequest,
    ) -> Result<Self, GlossError> {
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
pub(super) struct TransportFail {
    pub(super) error: GlossError,
    pub(super) retry_after: Option<Duration>,
}

/// 传输失败是否值得自动重试及等待时长（纯函数）：预算未尽且错误是瞬态
/// （网络/限流）才重试；等待取 `Retry-After` 指引与上限的较小值，无指引
/// 用固定退避。鉴权、参数与环境类拒绝重发无益，与预算耗尽同返回 None。
pub(super) fn retry_delay(
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
pub(super) fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
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
pub(super) async fn send_once(
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
pub(super) async fn connect_with_retry<T>(
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
pub(super) struct FirstByteStream<F> {
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
    pub(super) fn new(attempt: F, inner: TaskStream, retries_left: u32) -> Self {
        Self {
            attempt,
            inner: Some(inner),
            connecting: None,
            retries_left,
            handed_first: false,
        }
    }

    /// 推进进行中的重连：`None` = 新流已装上，回主循环继续；`Some` = 挂起
    /// 或以错误终结。
    fn poll_reconnect(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Option<Poll<Option<Result<TaskChunk, GlossError>>>> {
        let connect = self.connecting.as_mut()?;
        match connect.as_mut().poll(cx) {
            Poll::Pending => Some(Poll::Pending),
            Poll::Ready(Ok(stream)) => {
                self.inner = Some(stream);
                self.connecting = None;
                None
            }
            Poll::Ready(Err(fail)) => {
                self.connecting = None;
                Some(Poll::Ready(Some(Err(fail.error))))
            }
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
            if let Some(verdict) = this.poll_reconnect(cx) {
                return verdict;
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

/// 把响应的字节流适配成 [`TaskStream`]：SSE 逐行解码，吐文本增量，`[DONE]`
/// 上交一条携带结束原因的终结条目（[`TaskChunk::Done`]）。
///
/// 手动实现 `poll_next`（与编排层的手工轮询同一风格），不引 futures-util：
/// 状态机只有「攒够一行 → 产出条目 → 继续拉字节」三步。首个 `Err` 即终结流
/// （端口契约），此后不再拉取字节。
pub(super) struct SseStream {
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
    pub(super) fn new(
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
                        self.ready.push_back(Err(GlossError::EmptyCompletion));
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
            self.ready.push_back(Err(GlossError::EmptyCompletion));
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
