//! 配置存储的密钥半边的语义骨架：进程内读取缓存与读写删编排只写一份，系统
//! （macOS）与系统（Windows）两侧平台目录共用，两侧平台目录只留各自的系统
//! 调用面，底层读写删经密钥底层读写删接口（SecretIo）注入；本模块零目标
//! 条件编译、零外部函数接口（FFI）、零不安全块。三项骨架语义单源：缺条目
//! 读空——底层读回空即缓存空值，其后读取不再触底层；缓存一致性——值含空
//! 值、写删路径同步更新，克隆出的副本共享同一份缓存，未命中首读以先落者
//! 为准；错误降级——底层错误原样上抛为配置错误形态，锁中毒持锁期间恐慌
//! 取内值继续，失败读不落缓存。条目服务名（SERVICE）两侧单源：共用同一应
//! 用标识。密钥永不落明文、不进日志或错误消息——密钥红线。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use gloss_core::model::GlossError;

pub(crate) const SERVICE: &str = "io.github.losmli010.gloss";

#[derive(Debug, Clone)]
pub(crate) struct CachedSecretStore<I> {
    service: String,
    io: I,
    cache: SecretCache,
}

impl<I> CachedSecretStore<I> {
    pub(crate) fn with_io(service: impl Into<String>, io: I) -> Self {
        Self {
            service: service.into(),
            io,
            cache: SecretCache::default(),
        }
    }
}

impl<I: SecretIo> CachedSecretStore<I> {
    pub(crate) fn get(&self, key: &str) -> Result<Option<String>, GlossError> {
        if let Some(cached) = self.cache.cached(key) {
            return Ok(cached);
        }
        self.io.read(&self.cache, &self.service, key)
    }

    pub(crate) fn set(&self, key: &str, value: &str) -> Result<(), GlossError> {
        self.io.write(&self.cache, &self.service, key, value)?;
        self.cache
            .cached_map()
            .insert(key.to_owned(), Some(value.to_owned()));
        Ok(())
    }

    pub(crate) fn delete(&self, key: &str) -> Result<(), GlossError> {
        self.io.remove(&self.service, key)?;
        self.cache.cached_map().insert(key.to_owned(), None);
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SecretCache {
    map: Arc<Mutex<HashMap<String, Option<String>>>>,
}

impl SecretCache {
    pub(crate) fn cached(&self, key: &str) -> Option<Option<String>> {
        self.cached_map().get(key).cloned()
    }

    pub(crate) fn settle(&self, key: &str, value: Option<String>) -> Option<String> {
        self.cached_map()
            .entry(key.to_owned())
            .or_insert(value)
            .clone()
    }

    pub(crate) fn cached_map(&self) -> std::sync::MutexGuard<'_, HashMap<String, Option<String>>> {
        self.map
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub(crate) trait SecretIo {
    fn read(
        &self,
        cache: &SecretCache,
        service: &str,
        key: &str,
    ) -> Result<Option<String>, GlossError>;
    fn write(
        &self,
        cache: &SecretCache,
        service: &str,
        key: &str,
        value: &str,
    ) -> Result<(), GlossError>;
    fn remove(&self, service: &str, key: &str) -> Result<(), GlossError>;
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use gloss_core::model::GlossError;

    use super::{CachedSecretStore, SecretCache, SecretIo};

    #[derive(Debug, Clone, Default)]
    struct FakeIo(Arc<Mutex<HashMap<String, Option<String>>>>);

    impl SecretIo for FakeIo {
        fn read(
            &self,
            cache: &SecretCache,
            _service: &str,
            key: &str,
        ) -> Result<Option<String>, GlossError> {
            let value = self.0.lock().expect("poisoned").get(key).cloned().flatten();
            Ok(cache.settle(key, value))
        }

        fn write(
            &self,
            _cache: &SecretCache,
            _service: &str,
            key: &str,
            value: &str,
        ) -> Result<(), GlossError> {
            self.0
                .lock()
                .expect("poisoned")
                .insert(key.to_owned(), Some(value.to_owned()));
            Ok(())
        }

        fn remove(&self, _service: &str, key: &str) -> Result<(), GlossError> {
            self.0.lock().expect("poisoned").remove(key);
            Ok(())
        }
    }

    #[derive(Debug, Clone, Default)]
    struct FailingIo;

    impl SecretIo for FailingIo {
        fn read(
            &self,
            _cache: &SecretCache,
            _service: &str,
            _key: &str,
        ) -> Result<Option<String>, GlossError> {
            Err(GlossError::Config("underlying read failed".into()))
        }

        fn write(
            &self,
            _cache: &SecretCache,
            _service: &str,
            _key: &str,
            _value: &str,
        ) -> Result<(), GlossError> {
            Err(GlossError::Config("underlying write failed".into()))
        }

        fn remove(&self, _service: &str, _key: &str) -> Result<(), GlossError> {
            Err(GlossError::Config("underlying delete failed".into()))
        }
    }

    fn test_store() -> CachedSecretStore<FakeIo> {
        CachedSecretStore::with_io("io.github.losmli010.gloss.test", FakeIo::default())
    }

    #[test]
    fn missing_entry_reads_as_none() {
        let store = test_store();
        let key = "test/missing-entry-reads-as-none";

        store.delete(key).ok();
        assert_eq!(store.get(key).expect("read should succeed"), None);
    }

    #[test]
    fn delete_missing_entry_is_ok() {
        let store = test_store();
        store
            .delete("test/delete-missing-entry-is-ok")
            .expect("delete of missing entry should succeed");
    }

    #[test]
    fn cached_reads_stay_consistent_with_writes() {
        let store = test_store();
        let key = "test/cached-reads-consistency";
        let cloned = store.clone();

        store.delete(key).expect("preset delete should succeed");
        assert_eq!(store.get(key).expect("read should succeed"), None);

        cloned
            .cache
            .cached_map()
            .insert(key.to_owned(), Some("sk-cached".into()));
        assert_eq!(
            store.get(key).expect("read should succeed"),
            Some("sk-cached".into()),
            "clones must share the cache"
        );

        store.delete(key).expect("delete should succeed");
        assert_eq!(
            cloned.get(key).expect("read should succeed"),
            None,
            "delete must invalidate the cached value"
        );
    }

    #[test]
    fn a_failed_underlying_read_stays_uncached_and_reports() {
        let store = CachedSecretStore::with_io("io.github.losmli010.gloss.test", FailingIo);
        let key = "test/failed-underlying-read";

        let err = store.get(key).expect_err("underlying failure must surface");
        assert!(matches!(err, GlossError::Config(_)), "got: {err:?}");
        assert_eq!(
            store.cache.cached(key),
            None,
            "failed reads must not settle into the cache"
        );
    }

    #[test]
    fn settle_keeps_the_first_answer_for_a_key() {
        let cache = SecretCache::default();
        let key = "test/settle-first-wins";

        assert_eq!(
            cache.settle(key, Some("sk-first".into())),
            Some("sk-first".into())
        );
        assert_eq!(
            cache.settle(key, Some("sk-second".into())),
            Some("sk-first".into()),
            "settling must not overwrite an already cached answer"
        );
    }
}
