//! gloss-core 测试桩库：端口替身与脚本引擎的唯一存放处。
//!
//! 供两类编译上下文共享同一份源：
//! - 集成测试目标（`tests/*.rs` 经 `mod mock;` 引入）；
//! - `src/` 内联单测（`lib.rs` 以 `#[cfg(test)] #[path]` 包含为 `crate::mock`）。
//!
//! 因此本文件统一以 `gloss_core::` 绝对路径引用库条目（lib 侧靠
//! `extern crate self as gloss_core` 让自引用成立）。本模块按生产代码
//! 对待（lint 与注释规则同 `src/`，见 AGENTS.md「注释纪律」）。
//!
//! 桩只承担两件事：**预置返回值**与**可注入的失败**——注入点要能造出想测
//! 的那种时序，观测点要能证明它发生过（见 AGENTS.md「测试」一节）。需要
//! 新能力时扩展本模块，别在测试里手搓 fake。桩按 crate 自持：跨 crate
//! 刻意不共享（各 crate 的 `tests/mock/` 各留所需副本），一份桩的行为变化
//! 不得静默改写另一个 crate 测试套件的语义。

// 桩是按需取用的能力全集：每个编译目标只用到其中一部分，未用能力不算死代码。
#![allow(dead_code)]

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll};
use std::time::Duration;

use futures::Stream;
use tokio::time::sleep;

use gloss_core::config::Config;
use gloss_core::model::{GlossError, ScreenRect};
use gloss_core::ports::{
    AiEngine, BoxFuture, Cache, ConfigStore, EngineRequest, HotkeyBinder, RegionCapture,
    SelectionReader, TaskStream,
};
use gloss_core::task::{HotkeyBinding, TaskOutcome};

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

/// 返回预置结果（成功文本或失败）的选区读取桩。
pub struct FixedSelectionReader(
    /// 每次 `read` 原样返回的结果。
    pub Result<String, GlossError>,
);

impl SelectionReader for FixedSelectionReader {
    fn read(&mut self) -> Result<String, GlossError> {
        self.0.clone()
    }
}

/// 返回预置 PNG 字节的截图桩。
pub struct FixedRegionCapture(
    /// 每次 `capture` 原样返回的结果。
    pub Result<Arc<[u8]>, GlossError>,
);

impl RegionCapture for FixedRegionCapture {
    fn capture(&mut self, _rect: ScreenRect) -> Result<Arc<[u8]>, GlossError> {
        self.0.clone()
    }
}

/// 内存版配置存储桩：密钥键值对 + 单份配置文档，可按需注入失败。
#[derive(Default)]
pub struct MemoryConfigStore {
    secrets: Mutex<HashMap<String, String>>,
    config: Mutex<Option<Config>>,
    /// `Some` 时 `load` 直接返回它（模拟损坏的配置文件）。
    load_failure: Mutex<Option<GlossError>>,
    /// `Some` 时 `save` 直接返回它（模拟落盘失败）。
    save_failure: Mutex<Option<GlossError>>,
}

impl MemoryConfigStore {
    /// 让后续 `load` 一律失败（配置文件损坏路径）。
    pub fn with_load_failure(self, error: GlossError) -> Self {
        *lock_or_recover(&self.load_failure) = Some(error);
        self
    }

    /// 让后续 `save` 一律失败（落盘失败路径）。
    pub fn with_save_failure(self, error: GlossError) -> Self {
        *lock_or_recover(&self.save_failure) = Some(error);
        self
    }
}

impl ConfigStore for MemoryConfigStore {
    fn load(&self) -> Result<Config, GlossError> {
        if let Some(err) = lock_or_recover(&self.load_failure).clone() {
            return Err(err);
        }
        Ok(lock_or_recover(&self.config).clone().unwrap_or_default())
    }

    fn save(&self, config: &Config) -> Result<(), GlossError> {
        if let Some(err) = lock_or_recover(&self.save_failure).clone() {
            return Err(err);
        }
        *lock_or_recover(&self.config) = Some(config.clone());
        Ok(())
    }

    fn secret(&self, key: &str) -> Result<Option<String>, GlossError> {
        Ok(lock_or_recover(&self.secrets).get(key).cloned())
    }

    fn set_secret(&self, key: &str, value: &str) -> Result<(), GlossError> {
        lock_or_recover(&self.secrets).insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete_secret(&self, key: &str) -> Result<(), GlossError> {
        lock_or_recover(&self.secrets).remove(key);
        Ok(())
    }
}

/// 内存键值缓存桩。
#[derive(Default)]
pub struct MemoryCache(
    /// key → 产物。
    Mutex<HashMap<u64, TaskOutcome>>,
);

impl Cache for MemoryCache {
    fn get(&self, key: u64) -> Option<TaskOutcome> {
        lock_or_recover(&self.0).get(&key).cloned()
    }

    fn set(&self, key: u64, value: TaskOutcome) {
        lock_or_recover(&self.0).insert(key, value);
    }
}

/// 记录每次重绑定的热键桩。
///
/// 与真实实现不同，它不接触任何平台资源。观测点是「调用发生过」与
/// 「收到的是哪份绑定表」，注入点是调用次数（首次装配不调、保存成功
/// 才调），够覆盖接线契约；真实的降级行为（键被别的应用占用
/// 而跳过、管理器不可用）由 gloss-platform 的 registrar 单测覆盖。
#[derive(Default)]
pub struct RecordingHotkeyBinder {
    calls: Mutex<Vec<Vec<HotkeyBinding>>>,
}

impl RecordingHotkeyBinder {
    /// 收到过的重绑定次数。
    pub fn call_count(&self) -> usize {
        lock_or_recover(&self.calls).len()
    }

    /// 最近一次收到的绑定表；从未被调用过时返回 `None`。
    pub fn last(&self) -> Option<Vec<HotkeyBinding>> {
        lock_or_recover(&self.calls).last().cloned()
    }
}

impl HotkeyBinder for RecordingHotkeyBinder {
    fn rebind(&self, bindings: &[HotkeyBinding]) -> usize {
        lock_or_recover(&self.calls).push(bindings.to_vec());
        // 桩不做平台注册，全部绑定视为生效——即模拟一个一切正常的平台。
        // 「几条被占用、几级被降级」是平台侧的事实，桩不替它编结果。
        bindings.len()
    }
}
