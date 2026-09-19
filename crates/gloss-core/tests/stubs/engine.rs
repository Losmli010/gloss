//! 脚本引擎桩：按预置序列产流，支持整体失败、chunk 间延迟、panic 注入
//! 与调用计数。

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use futures::Stream;
use tokio::time::sleep;

use gloss_core::model::GlossError;
use gloss_core::ports::{AiEngine, BoxFuture, EngineRequest, TaskStream};

use super::lock_or_recover;

/// 脚本引擎：按预置序列产流，支持整体失败、chunk 间延迟与调用计数。
///
/// 克隆共享脚本与计数（内部 `Arc`）——同一实例注入多处时断言的是同一台
/// 引擎的总调用量。
#[derive(Debug, Clone)]
pub struct MockEngine {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    /// execute 的整体失败：Some 时 execute 直接返回 Err，不产流。
    execute_failure: std::sync::Mutex<Option<GlossError>>,
    /// 一次性失败队列：每次 execute 消费一个，耗尽后照常产流。
    once_failures: std::sync::Mutex<std::collections::VecDeque<GlossError>>,
    /// 一次性 panic 队列：每次 execute 消费一个（驱动「后台 panic 被
    /// tokio 捕获」的验收）。
    execute_panics: std::sync::Mutex<std::collections::VecDeque<()>>,
    /// 产出的增量序列（可含 Err 模拟流中失败）。
    chunks: std::sync::Mutex<Vec<Result<String, GlossError>>>,
    /// 相邻 chunk 之间的延迟，模拟真实流式节奏。
    chunk_delay: std::sync::Mutex<Duration>,
    /// execute 被调用的次数。
    calls: AtomicUsize,
}

impl MockEngine {
    /// 无延迟、脚本为空、不失败的默认引擎。
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                execute_failure: std::sync::Mutex::new(None),
                once_failures: std::sync::Mutex::new(Default::default()),
                execute_panics: std::sync::Mutex::new(Default::default()),
                chunks: std::sync::Mutex::new(Vec::new()),
                chunk_delay: std::sync::Mutex::new(Duration::ZERO),
                calls: AtomicUsize::new(0),
            }),
        }
    }

    /// 注入流式增量脚本（可含 `Err` 模拟流中失败）。脚本作用于共享状态，
    /// 克隆体同样可见。
    pub fn with_chunks(self, chunks: Vec<Result<String, GlossError>>) -> Self {
        *lock_or_recover(&self.inner.chunks) = chunks;
        self
    }

    /// 注入 chunk 间延迟（作用于共享状态，克隆体同样可见）。
    pub fn with_chunk_delay(self, delay: Duration) -> Self {
        *lock_or_recover(&self.inner.chunk_delay) = delay;
        self
    }

    /// 注入 execute 整体失败。
    pub fn with_execute_failure(self, error: GlossError) -> Self {
        *lock_or_recover(&self.inner.execute_failure) = Some(error);
        self
    }

    /// 注入一次性 execute 失败：仅下一次 execute 返回 Err，之后照常产流
    /// （「失败落 Error 态可重试」的验收驱动）。
    pub fn with_execute_failure_once(self, error: GlossError) -> Self {
        lock_or_recover(&self.inner.once_failures).push_back(error);
        self
    }

    /// 注入一次 execute panic：下一次 execute 的任务 future 在首次 poll
    /// 时炸掉，之后照常产流（panic 是这里的注入语义，tokio 桥的
    /// catch_unwind 是被测行为；一次性语义让「循环存活」可与正常任务
    /// 同引擎验证）。
    #[allow(clippy::panic)] // 测试桩：panic 即注入语义，见方法文档
    pub fn with_execute_panic(self) -> Self {
        lock_or_recover(&self.inner.execute_panics).push_back(());
        self
    }

    /// 引擎被调用的次数（「缓存命中不调引擎」断言用）。
    pub fn call_count(&self) -> usize {
        self.inner.calls.load(Ordering::Relaxed)
    }
}

impl Default for MockEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AiEngine for MockEngine {
    fn execute(
        &self,
        _request: &EngineRequest,
    ) -> BoxFuture<'static, Result<TaskStream, GlossError>> {
        self.inner.calls.fetch_add(1, Ordering::Relaxed);
        let once = lock_or_recover(&self.inner.once_failures).pop_front();
        if let Some(error) = once {
            return Box::pin(async move { Err(error) });
        }
        let failure = lock_or_recover(&self.inner.execute_failure).clone();
        if let Some(error) = failure {
            return Box::pin(async move { Err(error) });
        }
        if lock_or_recover(&self.inner.execute_panics)
            .pop_front()
            .is_some()
        {
            #[allow(clippy::panic)] // 测试桩：panic 即注入语义
            return Box::pin(async move { panic!("mock engine exploded") });
        }
        let chunks = lock_or_recover(&self.inner.chunks).clone();
        let delay = *lock_or_recover(&self.inner.chunk_delay);
        Box::pin(async move { Ok(Box::pin(ChunkStream::new(chunks, delay)) as TaskStream) })
    }
}

/// 逐 chunk 吐脚本的流：每个 chunk 之前等待 `delay`（首个 chunk 也等，
/// 统一模拟「首 token 延迟 + 后续节奏」）。
struct ChunkStream {
    chunks: std::vec::IntoIter<Result<String, GlossError>>,
    delay: Duration,
    pending_sleep: Option<Pin<Box<tokio::time::Sleep>>>,
    first: bool,
}

impl ChunkStream {
    fn new(chunks: Vec<Result<String, GlossError>>, delay: Duration) -> Self {
        Self {
            chunks: chunks.into_iter(),
            delay,
            pending_sleep: None,
            first: true,
        }
    }
}

impl Stream for ChunkStream {
    type Item = Result<String, GlossError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        if this.first {
            this.first = false;
            this.pending_sleep = Some(Box::pin(sleep(this.delay)));
        }
        // 有下一个 chunk 才需要等待；等待完成后吐 chunk。
        if let Some(pending) = this.pending_sleep.as_mut() {
            match pending.as_mut().poll(cx) {
                Poll::Ready(()) => this.pending_sleep = None,
                Poll::Pending => return Poll::Pending,
            }
        }
        match this.chunks.next() {
            Some(item) => {
                if !this.chunks.as_slice().is_empty() {
                    this.pending_sleep = Some(Box::pin(sleep(this.delay)));
                }
                Poll::Ready(Some(item))
            }
            None => Poll::Ready(None),
        }
    }
}
