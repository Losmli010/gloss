//! 一次传输尝试与响应流的适配：请求发送（状态检查、错误体上限读取）、
//! 瞬态失败判定与重试调度、“首个流条目之前”的自动重连，以及流式响应（SSE）
//! 到任务流事件的适配（空闲看门狗内建）。
//!
//! 重试只覆盖首个流条目之前的瞬态失败（重试次数常量，仅网络
//! 与限流类）；连接阶段与流阶段共用同一份预算——前一阶段耗尽，后一阶段
//! 不再重试。取消由调用方丢弃异步体完成。
//!
//! 公开面：线级发送入口（send_with_retry）收口组装解析产物、连接阶段重试
//! 与首字节重连的完整序列——客户端实现（库入口）与线级集成测试共用同一
//! 生产序列；其余条目收在包内。

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use crate::openai::sse::{SseDecoder, SseItem};
use bytes::Bytes;
use futures_core::Stream;
use gloss_core::log::{debug, thread, warn};
use gloss_core::model::GlossError;
use gloss_core::ports::{BoxFuture, EngineRequest, TaskChunk, TaskStream};
use tokio::time::{Instant as TokioInstant, Sleep, sleep, sleep_until};

use crate::openai::failure::{MAX_ERROR_BODY_BYTES, map_failure, map_transport_error};
use crate::openai::request_body::chat_request_body;

pub(super) const RETRY_ATTEMPTS: u32 = 1;

pub(super) const RETRY_BACKOFF: Duration = Duration::from_millis(500);

pub(super) const RETRY_AFTER_CAP: Duration = Duration::from_secs(1);

const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

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

pub(super) struct Resolved {
    url: String,
    key: String,
    body: Bytes,
}

impl Resolved {
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

pub(super) struct TransportFail {
    pub(super) error: GlossError,
    pub(super) retry_after: Option<Duration>,
}

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

fn idle_deadline(now: TokioInstant) -> TokioInstant {
    now + IDLE_TIMEOUT
}

pub(super) async fn send_once(
    client: &reqwest::Client,
    resolved: &Resolved,
) -> Result<TaskStream, TransportFail> {
    let mut response = client
        .post(&resolved.url)
        .bearer_auth(&resolved.key)
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
        let retry_after = parse_retry_after(response.headers());
        let detail = read_bounded_body(&mut response, MAX_ERROR_BODY_BYTES).await;
        return Err(TransportFail {
            error: map_failure(status, &detail),
            retry_after,
        });
    }
    Ok(Box::pin(SseStream::new(response.bytes_stream())) as TaskStream)
}

pub async fn send_with_retry(
    client: &reqwest::Client,
    endpoint: &str,
    key: &str,
    request: &EngineRequest,
) -> Result<TaskStream, GlossError> {
    let resolved = Arc::new(Resolved::build(
        endpoint.to_owned(),
        key.to_owned(),
        request,
    )?);
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
    let (connected, retries_left) = connect_with_retry(attempt.clone(), RETRY_ATTEMPTS).await;
    let stream = connected?;
    Ok(Box::pin(FirstByteStream::new(attempt, stream, retries_left)) as TaskStream)
}

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

pub(super) struct FirstByteStream<F> {
    attempt: F,
    inner: Option<TaskStream>,
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

pub(super) struct SseStream {
    inner: Pin<Box<dyn Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>>,
    decoder: SseDecoder,
    ready: VecDeque<Result<TaskChunk, GlossError>>,
    produced: bool,
    finished: bool,
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

    fn absorb(&mut self, items: Vec<SseItem>) {
        for item in items {
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
                if this.idle.as_mut().poll(cx).is_ready() {
                    return Poll::Ready(None);
                }
                match this.inner.as_mut().poll_next(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Some(Ok(tail_bytes))) => {
                        this.idle.as_mut().reset(idle_deadline(TokioInstant::now()));
                        let _ = tail_bytes;
                    }
                    Poll::Ready(Some(Err(_))) | Poll::Ready(None) => return Poll::Ready(None),
                }
                continue;
            }
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
                    this.ready.push_back(Err(map_transport_error(err)));
                    this.finished = true;
                }
                Poll::Ready(None) => {
                    let tail = this.decoder.finish();
                    this.absorb(tail);
                    this.finish_without_error();
                }
            }
        }
    }
}
