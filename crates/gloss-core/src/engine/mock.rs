//! mock AiEngine（M3-T7）：按脚本吐预置流式 chunk 的假引擎。
//!
//! 供 core 单测（cfg(test)）与下游 crate 的 dev-dependencies
//! （开 `test-util` 特性）使用，验收 M3 出口标准与 T6/T8 的全链路测试：
//! - 可调延迟模拟真实流式（chunk 间 `tokio::time::sleep`）；
//! - 可注入失败：execute 整体失败，或流中任意位置插 `Err`；
//! - 调用计数供「缓存命中不调引擎」类断言。

use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll};
use std::time::Duration;

use futures_core::Stream;
use tokio::time::sleep;

use crate::model::GlossError;
use crate::ports::{AiEngine, BoxFuture, TaskStream};
use crate::task::Task;

/// 锁中毒恢复：测试基建不值得 panic，拿回守卫继续用（数据由测试自身
/// 单线程写入，中毒不可能源于本模块逻辑）。
fn lock_or_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

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
    fn execute(&self, _task: &Task) -> BoxFuture<'static, Result<TaskStream, GlossError>> {
        self.inner.calls.fetch_add(1, Ordering::Relaxed);
        let failure = lock_or_recover(&self.inner.execute_failure).clone();
        if let Some(error) = failure {
            return Box::pin(async move { Err(error) });
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

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use futures::StreamExt;

    use super::MockEngine;
    use crate::model::GlossError;
    use crate::ports::AiEngine;
    use crate::task::{Task, TaskInput, TaskKind, TaskOptions};

    fn sample_task() -> Task {
        Task {
            kind: TaskKind::TranslateWord,
            input: TaskInput::Text {
                text: "gloss".into(),
                hint: None,
            },
            options: TaskOptions::default(),
        }
    }

    /// 验收标准：可调延迟模拟真实流式——三个 chunk 的总耗时下界为两段
    /// chunk 间延迟。
    #[tokio::test]
    async fn chunk_delay_paces_the_stream() {
        let engine = MockEngine::new()
            .with_chunk_delay(Duration::from_millis(30))
            .with_chunks(vec![Ok("光".into()), Ok("泽".into()), Ok("注释".into())]);
        let mut stream = engine.execute(&sample_task()).await.expect("stream");

        let start = Instant::now();
        let mut seen = Vec::new();
        while let Some(chunk) = stream.next().await {
            seen.push(chunk.expect("chunk ok"));
        }
        assert_eq!(seen, vec!["光", "泽", "注释"]);
        assert!(
            start.elapsed() >= Duration::from_millis(60),
            "two inter-chunk delays must pace the stream, got {:?}",
            start.elapsed()
        );
    }

    /// 验收标准：可注入各类 GlossError——execute 整体失败与流中失败两路。
    #[tokio::test]
    async fn failures_are_injectable() {
        for error in [
            GlossError::EngineNetwork,
            GlossError::EngineAuth,
            GlossError::EngineRateLimited,
            GlossError::EngineResponse("bad json".into()),
        ] {
            let engine = MockEngine::new().with_execute_failure(error.clone());
            match engine.execute(&sample_task()).await {
                Err(actual) => assert_eq!(actual, error, "failure must surface verbatim"),
                Ok(_) => panic!("expected {error:?}, got a stream"),
            }
        }

        let mid_stream = MockEngine::new().with_chunks(vec![
            Ok("部分".into()),
            Err(GlossError::EngineNetwork),
            Ok("流继续".into()),
        ]);
        let mut stream = mid_stream.execute(&sample_task()).await.expect("stream");
        assert_eq!(stream.next().await, Some(Ok("部分".into())));
        assert_eq!(
            stream.next().await,
            Some(Err(GlossError::EngineNetwork)),
            "mid-stream failure must surface in order"
        );
        // 流本身按脚本播完，Err 只是普通增量——在 Err 处截断是编排层
        // （AiTaskService）的职责，见 engine::tests。
        assert_eq!(stream.next().await, Some(Ok("流继续".into())));
        assert!(stream.next().await.is_none(), "stream ends at script end");
    }

    /// 调用计数：缓存命中断言（T6 测试）的基础。
    #[tokio::test]
    async fn call_count_tracks_execute_invocations() {
        let engine = MockEngine::new().with_chunks(vec![Ok("x".into())]);
        assert_eq!(engine.call_count(), 0);
        let mut stream = engine.execute(&sample_task()).await.expect("stream");
        while let Some(chunk) = stream.next().await {
            chunk.expect("chunk ok");
        }
        assert_eq!(engine.call_count(), 1);

        // 克隆共享同一计数。
        let cloned = engine.clone();
        let _ = cloned.execute(&sample_task()).await.expect("stream");
        assert_eq!(engine.call_count(), 2);
    }

    /// 空脚本产出立即结束的流（全缓存命中路径的边界）。
    #[tokio::test]
    async fn empty_script_yields_empty_stream() {
        let engine = MockEngine::new();
        let mut stream = engine.execute(&sample_task()).await.expect("stream");
        assert!(stream.next().await.is_none());
    }
}
