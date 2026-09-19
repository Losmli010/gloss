//! 运行时配置共享与保存：`ArcSwap<Config>` 快照替换。
//!
//! 落在 core 而不是 app：gloss-app 与 gloss-platform 都要读同一份快照（应用
//! 侧在触发时解析任务选项；platform 侧的引擎每请求读端点与 keychain 条目
//! 标识），而 platform 只依赖 core（依赖方向红线）——句柄必须放在两者共同的
//! 可见层。组装点把同一句柄注入 app 与 platform 两侧。
//!
//! **模型不经此解析**：引擎从 `EngineRequest::model` 取模型（App 已在触发时按
//! `Config::resolved_model` 解析并随请求携带），不读快照。
//! 引擎读快照的只有端点与 provider 条目：这两者不参与缓存 key。
//!
//! 三条不变量：
//! - **读路径零锁**：`snapshot` 是 `ArcSwap::load_full`，不碰任何锁；`save_lock`
//!   只串行化保存，读路径从不获取它；
//! - **单次任务内配置一致**：任务参数（类型 / 目标语言 / 模型）在触发时由
//!   调用方取**一次**快照解析进任务，执行途中不再回头读句柄——半路换配置不会
//!   让一个任务用上两个版本的参数；
//! - **先落盘再换快照**：`save` 里 `ConfigStore::save` 成功才替换运行时视图，
//!   落盘失败时内存保持旧版本（磁盘是唯一真相，内存不先行）。
//!
//! 密钥不进快照：`Config` 只存 keychain 条目标识，密钥用时经
//! `ConfigStore::secret` 直查（config 模块红线）。

use std::sync::{Arc, Mutex, PoisonError};

use arc_swap::ArcSwap;

use crate::config::Config;
use crate::log::error;
use crate::model::GlossError;
use crate::ports::ConfigStore;

/// 配置句柄：跨线程共享的当前配置快照 + 整份保存入口。
pub struct ConfigHandle {
    /// 当前快照；替换即换版本，读取永远拿到某个完整版本。
    current: ArcSwap<Config>,
    /// 保存的落盘目标（文档部分）；密钥半边不经此句柄，用时直查
    /// `ConfigStore::secret`。
    store: Arc<dyn ConfigStore>,
    /// 保存临界区：把「落盘 + 换快照」收成一步，并发保存不会让磁盘与内存
    /// 各留一版。只串行化写侧——`snapshot` 不碰它（读路径仍然零锁）。
    save_lock: Mutex<()>,
}

impl std::fmt::Debug for ConfigHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // store 是 trait 对象（未约束 Debug），只打印可读的快照。
        f.debug_struct("ConfigHandle")
            .field("current", &self.current)
            .finish_non_exhaustive()
    }
}

impl ConfigHandle {
    /// 装配期构造：从存储加载首份快照；加载失败原样返回错误，由调用方决定
    /// 退出还是降级（[`ConfigHandle::load_or_default`] 是降级版）。
    pub fn load(store: Arc<dyn ConfigStore>) -> Result<Self, GlossError> {
        let config = store.load()?;
        Ok(Self::with_config(store, config))
    }

    /// 装配期构造（宽容版）：加载失败记 error 日志后退回出厂默认，不阻断
    /// 启动。「退出或降级」在这里选降级——配置文件是用户手改的，
    /// 因一份改坏的 TOML 拒绝启动会让用户连设置界面都进不去；存储里的坏文件
    /// 保持原样，用户可自行修复（或由设置页保存覆盖）。
    pub fn load_or_default(store: Arc<dyn ConfigStore>) -> Self {
        match store.load() {
            Ok(config) => Self::with_config(store, config),
            Err(err) => {
                error!(
                    error = %err,
                    "config load failed, falling back to factory defaults"
                );
                Self::with_config(store, Config::default())
            }
        }
    }

    /// 用给定快照构造（装配期降级路径与测试注入用）。
    pub fn with_config(store: Arc<dyn ConfigStore>, config: Config) -> Self {
        Self {
            current: ArcSwap::from_pointee(config),
            store,
            save_lock: Mutex::new(()),
        }
    }

    /// 当前快照（零锁）。调用方在任务开始时取一次，任务期间复用同一份。
    pub fn snapshot(&self) -> Arc<Config> {
        self.current.load_full()
    }

    /// 保存整份配置：先落盘、成功后才原子替换快照；落盘失败时运行时视图
    /// 保持旧版本，调用方拿到错误去提示用户（设置页保存路径）。
    ///
    /// 并发调用由 `save_lock` 串行化：后进入临界区的那次保存同时决定磁盘与
    /// 内存的版本，不会出现「磁盘是 B、运行时还在 A」。锁中毒按测试基建的
    /// 同一套做法恢复（`PoisonError::into_inner`）：临界区里只有端口调用，
    /// 中毒不影响数据一致性。
    pub fn save(&self, config: Config) -> Result<(), GlossError> {
        let _guard = self
            .save_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.store.save(&config)?;
        self.current.store(Arc::new(config));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Barrier;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use crate::model::Lang;
    use crate::stubs::ports::MemoryConfigStore;

    use super::*;

    fn version_a() -> Config {
        Config {
            target_lang: Lang::Ja,
            cache_ttl_secs: 111,
            ..Default::default()
        }
    }

    fn version_b() -> Config {
        Config {
            target_lang: Lang::En,
            cache_ttl_secs: 222,
            ..Default::default()
        }
    }

    #[test]
    fn load_takes_first_snapshot_from_store() {
        let store = Arc::new(MemoryConfigStore::default());
        store.save(&version_a()).expect("store should accept save");

        let handle = ConfigHandle::load(store).expect("load should succeed");
        assert_eq!(*handle.snapshot(), version_a());
    }

    #[test]
    fn save_writes_through_and_swaps_snapshot() {
        let store = Arc::new(MemoryConfigStore::default());
        let handle = ConfigHandle::load(store.clone()).expect("load should succeed");
        assert_eq!(*handle.snapshot(), Config::default());

        handle.save(version_b()).expect("save should succeed");
        assert_eq!(
            *handle.snapshot(),
            version_b(),
            "runtime snapshot must advance"
        );
        assert_eq!(
            store.load().expect("store read should succeed"),
            version_b(),
            "document must be persisted"
        );
    }

    #[test]
    fn failed_save_keeps_previous_snapshot() {
        let store = Arc::new(
            MemoryConfigStore::default()
                .with_save_failure(GlossError::Config("disk on fire".into())),
        );
        let handle = ConfigHandle::with_config(store, version_a());

        let err = handle
            .save(version_b())
            .expect_err("failing store must surface the error");
        assert!(matches!(err, GlossError::Config(_)), "got: {err:?}");
        assert_eq!(
            *handle.snapshot(),
            version_a(),
            "failed save must not advance the snapshot"
        );
    }

    #[test]
    fn load_or_default_uses_store_snapshot_or_falls_back() {
        let store = Arc::new(MemoryConfigStore::default());
        store.save(&version_a()).expect("store should accept save");
        let handle = ConfigHandle::load_or_default(store);
        assert_eq!(
            *handle.snapshot(),
            version_a(),
            "healthy store must provide the first snapshot"
        );

        let broken = Arc::new(
            MemoryConfigStore::default()
                .with_load_failure(GlossError::Config("corrupt toml".into())),
        );
        let fallback = ConfigHandle::load_or_default(broken);
        assert_eq!(*fallback.snapshot(), Config::default());
    }

    #[test]
    fn load_propagates_store_failure() {
        let store = Arc::new(
            MemoryConfigStore::default()
                .with_load_failure(GlossError::Config("corrupt toml".into())),
        );

        let err = ConfigHandle::load(store).expect_err("load must fail");
        assert!(matches!(err, GlossError::Config(_)), "got: {err:?}");
    }

    #[test]
    fn saved_version_is_visible_from_another_thread() {
        let store = Arc::new(MemoryConfigStore::default());
        let handle = Arc::new(ConfigHandle::with_config(store, version_a()));
        let (release, wait) = std::sync::mpsc::channel();

        let reader = {
            let handle = Arc::clone(&handle);
            std::thread::spawn(move || {
                wait.recv().expect("writer should signal after saving");
                handle.snapshot()
            })
        };

        handle.save(version_b()).expect("save should succeed");
        release.send(()).expect("reader should still be waiting");
        assert_eq!(
            *reader.join().expect("reader thread should not panic"),
            version_b(),
            "another thread must observe the replaced snapshot"
        );
    }

    #[test]
    fn concurrent_readers_never_see_a_mixed_version() {
        const READERS: usize = 4;
        let store = Arc::new(MemoryConfigStore::default());
        let handle = Arc::new(ConfigHandle::with_config(store, version_a()));
        let done = Arc::new(AtomicBool::new(false));
        let reads = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(READERS + 1));

        let readers: Vec<_> = (0..READERS)
            .map(|_| {
                let handle = Arc::clone(&handle);
                let done = Arc::clone(&done);
                let reads = Arc::clone(&reads);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let mut consistent = true;
                    barrier.wait();
                    loop {
                        consistent &= is_whole_version(&handle.snapshot());
                        reads.fetch_add(1, Ordering::Relaxed);
                        if done.load(Ordering::Relaxed) {
                            break;
                        }
                    }
                    consistent
                })
            })
            .collect();

        barrier.wait();
        for _ in 0..200 {
            handle.save(version_a()).expect("save A should succeed");
            handle.save(version_b()).expect("save B should succeed");
        }
        done.store(true, Ordering::Relaxed);

        for reader in readers {
            let consistent = reader.join().expect("reader thread should not panic");
            assert!(consistent, "a reader observed a half-updated config");
        }
        assert!(
            reads.load(Ordering::Relaxed) >= READERS,
            "each reader must have observed at least one snapshot"
        );
        assert_eq!(*handle.snapshot(), version_b(), "last save wins");
    }

    fn is_whole_version(config: &Config) -> bool {
        let is_a = config.target_lang == Lang::Ja && config.cache_ttl_secs == 111;
        let is_b = config.target_lang == Lang::En && config.cache_ttl_secs == 222;
        is_a || is_b
    }

    #[test]
    fn concurrent_saves_keep_disk_and_snapshot_in_step() {
        let store = Arc::new(MemoryConfigStore::default());
        let handle = Arc::new(ConfigHandle::with_config(store.clone(), version_a()));
        let barrier = Arc::new(Barrier::new(4));

        let writers: Vec<_> = (0..4)
            .map(|index| {
                let handle = Arc::clone(&handle);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let config = if index % 2 == 0 {
                        version_a()
                    } else {
                        version_b()
                    };
                    barrier.wait();
                    for _ in 0..50 {
                        handle.save(config.clone()).expect("save should succeed");
                    }
                })
            })
            .collect();
        for writer in writers {
            writer.join().expect("writer thread should not panic");
        }

        assert_eq!(
            store.load().expect("store read should succeed"),
            *handle.snapshot(),
            "disk and runtime snapshot must agree after concurrent saves"
        );
    }
}
