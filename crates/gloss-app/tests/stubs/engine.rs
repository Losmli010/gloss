//! 脚本引擎桩：按预置序列产流，支持整体失败、块间延迟、故障注入
//! 与调用计数。

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use futures::Stream;
use tokio::time::sleep;

use gloss_core::model::GlossError;
use gloss_core::ports::{AiEngine, BoxFuture, EngineRequest, TaskChunk, TaskStream};

use super::lock_or_recover;

#[derive(Debug, Clone)]
pub struct MockEngine {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    execute_failure: std::sync::Mutex<Option<GlossError>>,
    once_failures: std::sync::Mutex<std::collections::VecDeque<GlossError>>,
    execute_panics: std::sync::Mutex<std::collections::VecDeque<()>>,
    scripts: std::sync::Mutex<std::collections::VecDeque<Vec<Result<TaskChunk, GlossError>>>>,
    last_script: std::sync::Mutex<Vec<Result<TaskChunk, GlossError>>>,
    chunk_delay: std::sync::Mutex<Duration>,
    calls: AtomicUsize,
    requests: std::sync::Mutex<Vec<EngineRequest>>,
}

impl MockEngine {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                execute_failure: std::sync::Mutex::new(None),
                once_failures: std::sync::Mutex::new(Default::default()),
                execute_panics: std::sync::Mutex::new(Default::default()),
                scripts: std::sync::Mutex::new(Default::default()),
                last_script: std::sync::Mutex::new(Vec::new()),
                chunk_delay: std::sync::Mutex::new(Duration::ZERO),
                calls: AtomicUsize::new(0),
                requests: std::sync::Mutex::new(Vec::new()),
            }),
        }
    }

    pub fn with_chunks(self, chunks: Vec<Result<String, GlossError>>) -> Self {
        let script = chunks
            .into_iter()
            .map(|item| item.map(TaskChunk::Delta))
            .collect();
        *lock_or_recover(&self.inner.scripts) = [script].into_iter().collect();
        self
    }

    pub fn with_script_queue(self, scripts: Vec<Vec<Result<TaskChunk, GlossError>>>) -> Self {
        *lock_or_recover(&self.inner.scripts) = scripts.into_iter().collect();
        self
    }

    pub fn with_chunk_delay(self, delay: Duration) -> Self {
        *lock_or_recover(&self.inner.chunk_delay) = delay;
        self
    }

    pub fn with_execute_failure(self, error: GlossError) -> Self {
        *lock_or_recover(&self.inner.execute_failure) = Some(error);
        self
    }

    pub fn with_execute_failure_once(self, error: GlossError) -> Self {
        lock_or_recover(&self.inner.once_failures).push_back(error);
        self
    }

    #[allow(clippy::panic)]
    pub fn with_execute_panic(self) -> Self {
        lock_or_recover(&self.inner.execute_panics).push_back(());
        self
    }

    pub fn call_count(&self) -> usize {
        self.inner.calls.load(Ordering::Relaxed)
    }

    pub fn recorded_requests(&self) -> Vec<EngineRequest> {
        lock_or_recover(&self.inner.requests).clone()
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
        request: EngineRequest,
    ) -> BoxFuture<'static, Result<TaskStream, GlossError>> {
        self.inner.calls.fetch_add(1, Ordering::Relaxed);
        lock_or_recover(&self.inner.requests).push(request);
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
            #[allow(clippy::panic)]
            return Box::pin(async move { panic!("mock engine exploded") });
        }
        let script = {
            let mut queue = lock_or_recover(&self.inner.scripts);
            match queue.pop_front() {
                Some(script) => {
                    *lock_or_recover(&self.inner.last_script) = script.clone();
                    script
                }
                None => lock_or_recover(&self.inner.last_script).clone(),
            }
        };
        let delay = *lock_or_recover(&self.inner.chunk_delay);
        Box::pin(async move { Ok(Box::pin(ChunkStream::new(script, delay)) as TaskStream) })
    }
}

struct ChunkStream {
    chunks: std::vec::IntoIter<Result<TaskChunk, GlossError>>,
    delay: Duration,
    pending_sleep: Option<Pin<Box<tokio::time::Sleep>>>,
    first: bool,
}

impl ChunkStream {
    fn new(chunks: Vec<Result<TaskChunk, GlossError>>, delay: Duration) -> Self {
        Self {
            chunks: chunks.into_iter(),
            delay,
            pending_sleep: None,
            first: true,
        }
    }
}

impl Stream for ChunkStream {
    type Item = Result<TaskChunk, GlossError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        if this.first {
            this.first = false;
            this.pending_sleep = Some(Box::pin(sleep(this.delay)));
        }
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
