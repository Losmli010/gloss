//! 端口定义：全部跨层 trait 的唯一定义点（06 §5.2，Ports & Adapters）。
//!
//! core 只声明契约；实现侧在 gloss-platform（`CompositeReader` /
//! `ScreenCapturer` / `LlmClient` / `FileConfigStore` / moka `Cache`），
//! 核心编排只见到这些 trait，单测用 [`mocks`]。
//!
//! async 方案定案：**不用 async-trait / trait-variant**——[`AiEngine::execute`]
//! 以普通方法返回 [`BoxFuture`]，签名本身对象安全（`dyn AiEngine` 可用），
//! 零宏；除 `futures-core`（[`TaskStream`] 的 `Stream`）外零额外依赖。
//! 若未来出现需要原生 `async fn` 的端口（无对象安全诉求时），再评估
//! AFIT，不回头改此决策。

use std::pin::Pin;
use std::sync::Arc;

use futures_core::Stream;

use crate::config::Config;
use crate::model::{GlossError, ScreenRect};
use crate::task::{Task, TaskOutcome};

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
/// 调用方保证：在平台事件线程上调用（线程亲和性，08 §4.4）。
///
/// 与 06 §5.2 草案（`Option<String>`）的偏差：落地时错误需要区分权限
/// 缺失（引导授权）与普通取不到（静默降级），故收窄为 `Result`——
/// 文档以本定义为准。
pub trait SelectionReader: Send {
    /// 读取前台应用当前选中文本；权限缺失返回
    /// [`GlossError::AccessibilityDenied`]，取不到返回
    /// [`GlossError::SelectionUnavailable`]。
    fn read(&mut self) -> Result<String, GlossError>;
}

/// 图像取材（端口）：截取屏幕指定区域。
///
/// 实现方保证：不写剪贴板（macOS 系统截图默认行为需显式规避）。
/// 调用方保证：在平台事件线程上调用（线程亲和性）。
pub trait RegionCapture: Send {
    /// 截取 `rect` 区域，返回 PNG 字节。
    fn capture(&mut self, rect: ScreenRect) -> Result<Arc<[u8]>, GlossError>;
}

/// AI 引擎（端口）：统一入口，不按输入模态拆分——文本/图文仅由消息
/// payload 与模型 id（`Task.options` + 配置）决定。
///
/// 实现方保证：`execute` 返回的 future 与流都是 `'static` 且 `Send`——
/// **不得借用 `task` 或 `self`**，任务数据需克隆（图像字节走 `Arc` 克隆
/// 为 O(1)）或移入 future；消费端在 tokio 上轮询。取消不进本端口，由
/// 调用方以 `CancellationToken` 在 await 侧竞速（08 §4.2 的单一取消机制）。
pub trait AiEngine: Send + Sync {
    /// 执行任务，返回流式产物流。
    fn execute(&self, task: &Task) -> BoxFuture<'static, Result<TaskStream, GlossError>>;
}

/// 配置存储（端口）：应用配置与密钥的读写边界。
///
/// 实现侧由两半边组合（`CompositeConfigStore`）：配置文档走
/// [`ConfigStore::load`] / [`ConfigStore::save`] 落 TOML 文件
/// （`FileConfigStore`），密钥走条目标识落系统安全存储
/// （`KeychainSecret`）——密钥不进配置快照（06 ADR），用时直查。
/// 全部方法取 `&self`（实现方以内部同步保证并发安全），适配器才能以
/// `Arc<dyn ConfigStore>` 注入。
///
/// 密钥红线（AGENT.md）：入参与返回值都是凭据，实现方禁止将其写进
/// 日志、错误消息或 `EngineResponse` 这类携带诊断文本的变体。
pub trait ConfigStore: Send + Sync {
    /// 读取整份配置；实现方保证缺文件时返回出厂默认（并尽力落盘）。
    fn load(&self) -> Result<Config, GlossError>;
    /// 原子写入整份配置（设置页保存路径）。
    fn save(&self, config: &Config) -> Result<(), GlossError>;
    /// 读取密钥；`None` 表示未设置。
    fn secret(&self, key: &str) -> Result<Option<String>, GlossError>;
    /// 写入（或覆盖）密钥。
    fn set_secret(&self, key: &str, value: &str) -> Result<(), GlossError>;
}

/// 缓存（端口）：core 内置 moka 内存实现（M3-T5）。
///
/// key = hash(kind, input, options, model)——同一文本在不同任务下不共享
/// 缓存（06 §5.2 ADR）；调用方保证 key 由统一哈希函数派生，且派生函数
/// 须抗碰撞：碰撞不是缓存 miss，而是把别的任务的产物交给用户（桌面规模
/// 下 64 位摘要概率可忽略，属接受的取舍）。
pub trait Cache: Send + Sync {
    /// 取缓存产物。
    fn get(&self, key: u64) -> Option<TaskOutcome>;
    /// 写缓存产物。
    fn set(&self, key: u64, value: TaskOutcome);
}

/// 供单测的桩实现（crate 内测试使用）。跨 crate 复用时（M3-T7 的延迟/
/// 失败注入 mock 引擎按计划落在 gloss-platform::engine::mock）需要以
/// test-util 特性门控导出或由 platform 自带，届时二选一。
#[cfg(test)]
pub(crate) mod mocks {
    use std::collections::HashMap;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll};

    use futures_core::Stream;

    use crate::config::Config;

    use super::{
        AiEngine, BoxFuture, Cache, ConfigStore, GlossError, RegionCapture, ScreenRect,
        SelectionReader, Task, TaskOutcome, TaskStream,
    };

    /// 固定返回预置结果的选区读取桩。
    pub(crate) struct FixedSelectionReader(pub Result<String, GlossError>);

    impl SelectionReader for FixedSelectionReader {
        fn read(&mut self) -> Result<String, GlossError> {
            self.0.clone()
        }
    }

    /// 固定返回预置 PNG 的截图桩。
    pub(crate) struct FixedRegionCapture(pub Result<Arc<[u8]>, GlossError>);

    impl RegionCapture for FixedRegionCapture {
        fn capture(&mut self, _rect: ScreenRect) -> Result<Arc<[u8]>, GlossError> {
            self.0.clone()
        }
    }

    /// 内存版配置存储桩：密钥键值对 + 单份配置文档。
    #[derive(Default)]
    pub(crate) struct MemoryConfigStore {
        secrets: Mutex<HashMap<String, String>>,
        config: Mutex<Option<Config>>,
    }

    impl ConfigStore for MemoryConfigStore {
        fn load(&self) -> Result<Config, GlossError> {
            Ok(self
                .config
                .lock()
                .expect("poisoned")
                .clone()
                .unwrap_or_default())
        }

        fn save(&self, config: &Config) -> Result<(), GlossError> {
            *self.config.lock().expect("poisoned") = Some(config.clone());
            Ok(())
        }

        fn secret(&self, key: &str) -> Result<Option<String>, GlossError> {
            Ok(self.secrets.lock().expect("poisoned").get(key).cloned())
        }

        fn set_secret(&self, key: &str, value: &str) -> Result<(), GlossError> {
            self.secrets
                .lock()
                .expect("poisoned")
                .insert(key.to_owned(), value.to_owned());
            Ok(())
        }
    }

    /// 内存键值缓存桩。
    #[derive(Default)]
    pub(crate) struct MemoryCache(Mutex<HashMap<u64, TaskOutcome>>);

    impl Cache for MemoryCache {
        fn get(&self, key: u64) -> Option<TaskOutcome> {
            self.0.lock().expect("poisoned").get(&key).cloned()
        }

        fn set(&self, key: u64, value: TaskOutcome) {
            self.0.lock().expect("poisoned").insert(key, value);
        }
    }

    /// 把预置增量序列变成流（futures-core 无构造子，测试自备最小适配）。
    pub(crate) fn delta_stream(chunks: Vec<Result<String, GlossError>>) -> TaskStream {
        struct Chunks(std::vec::IntoIter<Result<String, GlossError>>);

        impl Stream for Chunks {
            type Item = Result<String, GlossError>;

            fn poll_next(
                mut self: Pin<&mut Self>,
                _cx: &mut Context<'_>,
            ) -> Poll<Option<Self::Item>> {
                Poll::Ready(self.0.next())
            }
        }

        Box::pin(Chunks(chunks.into_iter()))
    }

    /// 返回预置增量流的引擎桩。
    pub(crate) struct ScriptedEngine(pub Vec<Result<String, GlossError>>);

    impl AiEngine for ScriptedEngine {
        fn execute(&self, _task: &Task) -> BoxFuture<'static, Result<TaskStream, GlossError>> {
            let chunks = self.0.clone();
            Box::pin(async move { Ok(delta_stream(chunks)) })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use futures::StreamExt;

    use super::mocks::{
        FixedRegionCapture, FixedSelectionReader, MemoryCache, MemoryConfigStore, ScriptedEngine,
        delta_stream,
    };
    use super::*;
    use crate::model::ScreenRect as Rect;
    use crate::task::{TaskInput, TaskKind, TaskOptions};

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

    fn outcome(body: &str) -> TaskOutcome {
        TaskOutcome {
            kind: TaskKind::TranslateWord,
            body: body.into(),
            structured: crate::task::OutcomeStructured::Plain { title: None },
        }
    }

    /// SelectionReader 契约：桩原样返回预置结果（成功与失败两路）。
    #[test]
    fn selection_reader_mock_returns_presets() {
        let mut ok = FixedSelectionReader(Ok("selected".into()));
        assert_eq!(ok.read(), Ok("selected".into()));

        let mut denied = FixedSelectionReader(Err(GlossError::AccessibilityDenied));
        assert_eq!(denied.read(), Err(GlossError::AccessibilityDenied));
    }

    /// RegionCapture 契约：桩原样返回 PNG 字节。
    #[test]
    fn region_capture_mock_returns_png() {
        let png: Arc<[u8]> = vec![1, 2, 3].into();
        let mut capture = FixedRegionCapture(Ok(Arc::clone(&png)));
        let got = capture
            .capture(Rect {
                x: 0,
                y: 0,
                width: 4,
                height: 4,
            })
            .expect("capture should succeed");
        assert!(Arc::ptr_eq(&png, &got));
    }

    /// ConfigStore 契约：未设置返回 None，写入后可读回。
    #[test]
    fn config_store_mock_round_trips_secrets() {
        let store = MemoryConfigStore::default();
        assert_eq!(store.secret("api_key"), Ok(None));
        store
            .set_secret("api_key", "sk-test")
            .expect("set should succeed");
        assert_eq!(store.secret("api_key"), Ok(Some("sk-test".into())));
    }

    /// ConfigStore 契约：未保存过返回出厂默认，保存后原样读回。
    #[test]
    fn config_store_mock_round_trips_document() {
        let store = MemoryConfigStore::default();
        assert_eq!(store.load(), Ok(Config::default()));

        let config = Config {
            auto_show: false,
            ..Default::default()
        };
        store.save(&config).expect("save should succeed");
        assert_eq!(store.load(), Ok(config));
    }

    /// Cache 契约：miss → set → hit，且不同 key 互不可见。
    #[test]
    fn cache_mock_stores_and_isolates_keys() {
        let cache = MemoryCache::default();
        assert!(cache.get(1).is_none());
        cache.set(1, outcome("cached"));
        assert_eq!(cache.get(1).map(|o| o.body), Some("cached".into()));
        assert!(cache.get(2).is_none(), "unrelated key must not see entry");
    }

    /// AiEngine 契约：桩按脚本吐出流式增量，流可完整消费。
    #[tokio::test]
    async fn ai_engine_mock_streams_scripted_deltas() {
        let engine = ScriptedEngine(vec![Ok("光".into()), Ok("泽".into())]);
        let mut stream = engine
            .execute(&sample_task())
            .await
            .expect("execute should succeed");

        let mut seen = String::new();
        while let Some(chunk) = stream.next().await {
            seen.push_str(&chunk.expect("chunk should be ok"));
        }
        assert_eq!(seen, "光泽");
    }

    /// TaskStream 构造助手：空脚本产出立即结束的流。
    #[tokio::test]
    async fn delta_stream_ends_without_chunks() {
        let mut stream = delta_stream(Vec::new());
        assert!(stream.next().await.is_none());
    }

    /// 失败增量能穿透 TaskStream：Err chunk 按序到达，流随后正常结束。
    #[tokio::test]
    async fn delta_stream_carries_failure_chunks() {
        let mut stream = delta_stream(vec![
            Ok("a".into()),
            Err(GlossError::EngineNetwork),
            Ok("b".into()),
        ]);
        assert_eq!(stream.next().await, Some(Ok("a".into())));
        assert_eq!(stream.next().await, Some(Err(GlossError::EngineNetwork)));
        assert_eq!(stream.next().await, Some(Ok("b".into())));
        assert!(stream.next().await.is_none());
    }
}
