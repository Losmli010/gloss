//! 端口定义：全部跨层 trait 的唯一定义点（06 §5.2，Ports & Adapters）。
//!
//! core 只声明契约；实现侧在 gloss-platform（`CompositeReader` /
//! `ScreenCapturer` / `LlmClient` / `FileConfigStore` / moka `Cache`），
//! 核心编排只见到这些 trait，测试用 `ports::mocks` 里的桩（`test-util`
//! 特性门控，下游 crate 也经它复用）。
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
use crate::prompt::ChatMessage;
use crate::task::{HotkeyBinding, TaskKind, TaskOutcome};

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

/// 引擎请求（[`AiEngine`] 的入参）：**已渲染**的对话消息 + **已解析**的
/// 模型 id。
///
/// 渲染是 core 编排的职责（`AiTaskService` 调 `PromptRegistry`，含模态校验），
/// 引擎只负责把请求送出去、把响应流回来——不做渲染，也不回读配置。模型随
/// 请求携带而非由引擎查表：否则「缓存 key 用的模型」与「实际请求的模型」可以
/// 来自两份配置快照（模型参与缓存 key，见 `AiTaskService::execute`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineRequest {
    /// 任务类型：引擎侧只用于诊断与能力判断，不参与渲染。
    pub kind: TaskKind,
    /// 渲染好的消息（多模态 content 数组随 M5-T4 扩展）。
    pub messages: Vec<ChatMessage>,
    /// 本任务使用的模型 id（App 在触发时按配置解析）。
    pub model: String,
}

/// AI 引擎（端口）：统一入口，不按输入模态拆分——文本/图文仅由消息
/// payload 与模型 id（[`EngineRequest`]）决定。渲染归 core 编排（引擎不做
/// 渲染）；**模型不回读配置**（随请求携带）。引擎自己读快照的只有端点与
/// provider 条目——这两者不进缓存 key，每请求取一次换来改端点无需重启。
///
/// 实现方保证：`execute` 返回的 future 与流都是 `'static` 且 `Send`——
/// **不得借用 `request` 或 `self`**，请求数据需克隆或移入 future；消费端
/// 在 tokio 上轮询。取消不进本端口，由调用方以 `CancellationToken` 在
/// await 侧竞速（08 §4.2 的单一取消机制）——实现方只需保证 future 被丢弃
/// 时连接随之关闭（异步客户端的默认行为）。
pub trait AiEngine: Send + Sync {
    /// 执行请求，返回流式产物流。
    fn execute(
        &self,
        request: &EngineRequest,
    ) -> BoxFuture<'static, Result<TaskStream, GlossError>>;
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

/// 热键重绑定（端口，M4-T7）：把配置里的绑定表交给平台侧注册。
///
/// 与其余端口不同，本端口**有意不加 `Send + Sync`**——热键的注册与注销
/// 必须在创建管理器的线程上执行：macOS 后端要求主线程跑 NSApp 事件循环，
/// Windows 后端的隐藏窗口、`WM_HOTKEY` 投递连同 `Drop` 的 `DestroyWindow`
/// 都亲和创建线程，其 `HWND` 本身就是 `!Send`。实现只允许在主线程使用；
/// 调用方（设置页保存路径）本来就跑在主线程的事件循环里，因此这里是同步
/// 调用而非通道命令——加 `Send` 反而会把一个用不上的跨线程承诺强加给实现。
///
/// 降级契约：绑定解析失败、被其他应用占用、管理器不可用等一律告警跳过，
/// **不返回错误**——热键是可降级功能，某个键被占用不该让一次配置保存整体
/// 失败（保存的落盘与快照替换照常完成）。
pub trait HotkeyBinder {
    /// 用给定绑定表**替换**当前注册（不是追加），返回实际生效的条数供
    /// 调用方记日志；条数少于入参说明有绑定被降级跳过。
    fn rebind(&self, bindings: &[HotkeyBinding]) -> usize;
}

/// 端口桩实现（测试辅助）：crate 内单测直接用，下游 crate 开 `test-util`
/// 特性后可用（gloss-app 的 dev-dependencies 已开，L1 集成测试与 App 单测
/// 靠它拿到配置存储桩）。
///
/// 桩只承担两件事：**预置返回值**与**可注入的失败**——注入点要能造出想测
/// 的那种时序，观测点要能证明它发生过（见 AGENTS.md「测试」一节）。需要新
/// 能力时扩展本模块，别在测试里手搓 fake。本模块在 `--all-features` 下按
/// 生产代码 lint（禁 unwrap/expect/panic），新增桩沿用 `lock_or_recover`
/// 式的降级写法。
#[cfg(any(test, feature = "test-util"))]
pub mod mocks {
    use std::collections::HashMap;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
    use std::task::{Context, Poll};

    use futures_core::Stream;

    use crate::config::Config;
    use crate::task::HotkeyBinding;

    use super::{
        AiEngine, BoxFuture, Cache, ConfigStore, EngineRequest, GlossError, HotkeyBinder,
        RegionCapture, ScreenRect, SelectionReader, TaskOutcome, TaskStream,
    };

    /// 锁中毒恢复：测试基建不值得 panic，拿回守卫继续用（数据由测试自身
    /// 单线程写入，中毒不可能源于本模块逻辑）。
    fn lock_or_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock().unwrap_or_else(PoisonError::into_inner)
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
    /// 才调），够覆盖 M4-T7 的接线契约；真实的降级行为（键被别的应用占用
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
            // 桩不做平台注册，全部绑定视为生效——「几条被占用」是平台侧的
            // 事实，桩不替它编一个结果。
            bindings.len()
        }
    }

    /// 把预置增量序列变成流（futures-core 无构造子，测试自备最小适配）。
    pub fn delta_stream(chunks: Vec<Result<String, GlossError>>) -> TaskStream {
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

    /// 返回预置增量流的引擎桩（不注入延迟/失败时比 `MockEngine` 轻）。
    pub struct ScriptedEngine(
        /// 产出的增量序列（可含 `Err` 模拟流中失败）。
        pub Vec<Result<String, GlossError>>,
    );

    impl AiEngine for ScriptedEngine {
        fn execute(
            &self,
            _request: &EngineRequest,
        ) -> BoxFuture<'static, Result<TaskStream, GlossError>> {
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
        FixedRegionCapture, FixedSelectionReader, MemoryCache, MemoryConfigStore,
        RecordingHotkeyBinder, ScriptedEngine, delta_stream,
    };
    use super::*;
    use crate::model::ScreenRect as Rect;
    use crate::task::{HotkeyBinding, InputSource, TaskKind};

    /// 引擎请求样例：桩不读它，只为满足端口签名。
    fn sample_request() -> EngineRequest {
        EngineRequest {
            kind: TaskKind::TranslateWord,
            messages: vec![ChatMessage {
                role: crate::prompt::Role::User,
                content: "gloss".into(),
            }],
            model: "mock-model".into(),
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

    /// HotkeyBinder 契约：桩按序记下每次收到的绑定表，并如实回报生效条数。
    /// 「从未调用」也要可区分——否则「保存没触发重注册」会被误判为通过。
    #[test]
    fn hotkey_binder_mock_records_every_call() {
        let binder = RecordingHotkeyBinder::default();
        assert_eq!(binder.call_count(), 0);
        assert!(binder.last().is_none(), "no call means nothing to report");

        let first = vec![HotkeyBinding {
            trigger: "Cmd+Shift+D".into(),
            kind: TaskKind::TranslateWord,
            source: InputSource::Selection,
        }];
        assert_eq!(binder.rebind(&first), 1, "applied count mirrors the input");

        let second = vec![HotkeyBinding {
            trigger: "Cmd+Shift+E".into(),
            kind: TaskKind::ExplainCode,
            source: InputSource::Selection,
        }];
        binder.rebind(&second);
        assert_eq!(binder.call_count(), 2);
        assert_eq!(
            binder.last(),
            Some(second),
            "the last call must win, not the first"
        );
        assert_eq!(binder.rebind(&[]), 0, "an empty table applies nothing");
    }

    /// AiEngine 契约：桩按脚本吐出流式增量，流可完整消费。
    #[tokio::test]
    async fn ai_engine_mock_streams_scripted_deltas() {
        let engine = ScriptedEngine(vec![Ok("光".into()), Ok("泽".into())]);
        let mut stream = engine
            .execute(&sample_request())
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
