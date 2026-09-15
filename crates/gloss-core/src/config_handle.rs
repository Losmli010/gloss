//! 运行时配置共享与保存（M4-T3）：`ArcSwap<Config>` 快照替换（06 §6.3）。
//!
//! 落在 core 而不是 app：gloss-app 与 gloss-platform（M4-T4 的引擎取模型、
//! 取密钥）都要读同一份快照，而 platform 只依赖 core（依赖方向红线）——
//! 句柄必须放在两者共同的可见层。组装点负责把它注入两侧。
//!
//! 三条不变量：
//! - **读路径零锁**：`snapshot` 是 `ArcSwap::load_full`，没有 Mutex；
//! - **单次任务内配置一致**：调用方在**任务开始时取一次快照**并把值解析进
//!   任务（App 侧在触发时解析任务类型与选项），任务执行途中不再回头读句柄
//!   ——半路换配置不会让一个任务用上两个版本的参数；
//! - **先落盘再换快照**：`save` 里 `ConfigStore::save` 成功才替换运行时视图，
//!   落盘失败时内存保持旧版本（磁盘是唯一真相，内存不先行）。
//!
//! 密钥不进快照：`Config` 只存 keychain 条目标识，密钥用时经
//! `ConfigStore::secret` 直查（config 模块红线）。

use std::sync::Arc;

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
    /// 启动。06 §3.3 的「退出或降级」在这里选降级——配置文件是用户手改的，
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
        }
    }

    /// 当前快照（零锁）。调用方在任务开始时取一次，任务期间复用同一份。
    pub fn snapshot(&self) -> Arc<Config> {
        self.current.load_full()
    }

    /// 保存整份配置：先落盘、成功后才原子替换快照；落盘失败时运行时视图
    /// 保持旧版本，调用方拿到错误去提示用户（设置页保存路径）。
    pub fn save(&self, config: Config) -> Result<(), GlossError> {
        self.store.save(&config)?;
        self.current.store(Arc::new(config));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use crate::model::Lang;
    use crate::ports::mocks::MemoryConfigStore;
    use crate::task::TaskKind;

    use super::*;

    /// 版本 A：目标语言日语 + TTL 111（成对字段，用于识别「半个版本」）。
    fn version_a() -> Config {
        Config {
            target_lang: Lang::Ja,
            cache_ttl_secs: 111,
            ..Default::default()
        }
    }

    /// 版本 B：目标语言英语 + TTL 222。
    fn version_b() -> Config {
        Config {
            target_lang: Lang::En,
            cache_ttl_secs: 222,
            ..Default::default()
        }
    }

    /// 装配期：首份快照就是存储里那份配置。
    #[test]
    fn load_takes_first_snapshot_from_store() {
        let store = Arc::new(MemoryConfigStore::default());
        store.save(&version_a()).expect("store should accept save");

        let handle = ConfigHandle::load(store).expect("load should succeed");
        assert_eq!(*handle.snapshot(), version_a());
    }

    /// 验收标准：保存 = 写文件 + 原子替换快照——两条路径都拿到新版本。
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

    /// 落盘失败不得推进运行时视图：磁盘是唯一真相，内存不先行。
    #[test]
    fn failed_save_keeps_previous_snapshot() {
        let store = Arc::new(
            MemoryConfigStore::default()
                .with_save_failure(GlossError::Config("disk on fire".into())),
        );
        let handle = ConfigHandle::with_config(store.clone(), version_a());

        let err = handle
            .save(version_b())
            .expect_err("failing store must surface the error");
        assert!(matches!(err, GlossError::Config(_)), "got: {err:?}");
        assert_eq!(
            *handle.snapshot(),
            version_a(),
            "failed save must not advance the snapshot"
        );
        assert_eq!(
            store.load().expect("store read should succeed"),
            Config::default(),
            "failed save must not have written anything"
        );
    }

    /// 降级路径：配置文件损坏（存储层报错）时用出厂默认起，不阻断启动。
    #[test]
    fn load_or_default_falls_back_to_factory_defaults() {
        let store = Arc::new(
            MemoryConfigStore::default()
                .with_load_failure(GlossError::Config("corrupt toml".into())),
        );

        let handle = ConfigHandle::load_or_default(store);
        assert_eq!(*handle.snapshot(), Config::default());
    }

    /// 装配期硬错误：加载失败且调用方要求传播时，错误原样上抛。
    #[test]
    fn load_propagates_store_failure() {
        let store = Arc::new(
            MemoryConfigStore::default()
                .with_load_failure(GlossError::Config("corrupt toml".into())),
        );

        let err = ConfigHandle::load(store).expect_err("load must fail");
        assert!(matches!(err, GlossError::Config(_)), "got: {err:?}");
    }

    /// 并发读写下每个快照都是**某个完整版本**：读侧永远看不到两个版本的
    /// 字段混在一起（这正是快照替换相对「逐字段改」的价值）。
    #[test]
    fn concurrent_readers_never_see_a_mixed_version() {
        let store = Arc::new(MemoryConfigStore::default());
        let handle = Arc::new(ConfigHandle::with_config(store, version_a()));
        let done = Arc::new(AtomicBool::new(false));

        let readers: Vec<_> = (0..4)
            .map(|_| {
                let handle = Arc::clone(&handle);
                let done = Arc::clone(&done);
                std::thread::spawn(move || {
                    let mut consistent = true;
                    // 先读再判结束：读者至少观察一份快照，测试不因调度快慢空转。
                    loop {
                        let snapshot = handle.snapshot();
                        let is_a =
                            snapshot.target_lang == Lang::Ja && snapshot.cache_ttl_secs == 111;
                        let is_b =
                            snapshot.target_lang == Lang::En && snapshot.cache_ttl_secs == 222;
                        consistent &= is_a || is_b;
                        if done.load(Ordering::Relaxed) {
                            break;
                        }
                    }
                    consistent
                })
            })
            .collect();

        for _ in 0..200 {
            handle.save(version_a()).expect("save A should succeed");
            handle.save(version_b()).expect("save B should succeed");
        }
        done.store(true, Ordering::Relaxed);

        for reader in readers {
            let consistent = reader.join().expect("reader thread should not panic");
            assert!(consistent, "a reader observed a half-updated config");
        }
        assert_eq!(*handle.snapshot(), version_b(), "last save wins");
    }

    /// 快照是整份配置：解析进任务的目标语言与模型来自同一版本（回归：
    /// 「任务开始时取一次」的语义由调用方保证，这里锁住快照的完整性）。
    #[test]
    fn snapshot_carries_every_field_of_the_saved_version() {
        let store = Arc::new(MemoryConfigStore::default());
        let handle = ConfigHandle::with_config(store, Config::default());

        let config = Config {
            target_lang: Lang::Ko,
            model_by_kind: vec![crate::config::ModelBinding {
                kind: TaskKind::TranslateWord,
                model: "deepseek-chat".into(),
            }],
            ..Default::default()
        };
        handle.save(config.clone()).expect("save should succeed");

        let snapshot = handle.snapshot();
        assert_eq!(
            snapshot.model_for_kind(TaskKind::TranslateWord),
            Some("deepseek-chat")
        );
        assert_eq!(snapshot.target_lang, Lang::Ko);
    }
}
