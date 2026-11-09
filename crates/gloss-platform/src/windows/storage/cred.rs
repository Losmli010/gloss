//! 配置存储的密钥半边：凭据管理器（凭据应用编程接口）的读写删。
//!
//! 通用凭据按“服务名加键串”寻址，载荷为密钥的原始字节；密钥永不落明文、
//! 不进日志或错误消息——密钥红线。带进程内读取缓存（读未命中才访问凭据
//! 管理器，写删同步更新缓存）：任务链路每次执行都要取一次密钥，缓存把
//! 系统调用收敛到每进程每键一次，设置页改密钥经同一实例写入缓存即时生效，
//! 无需重启。与系统（macOS）钥匙串半边不同，凭据管理器不按访问方签名设
//! 访问控制，读取无弹窗，因此无需读入门禁；并发首读各自撞一次系统调用
//! 无碍（幂等读），缓存以先落者为准。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use gloss_core::model::GlossError;

use crate::windows::ffi;

const SERVICE: &str = "io.github.losmli010.gloss";

fn target_name(service: &str, key: &str) -> String {
    format!("{service}/{key}")
}

#[derive(Debug, Clone)]
pub struct CredentialSecret {
    service: String,
    cache: Arc<Mutex<HashMap<String, Option<String>>>>,
}

impl Default for CredentialSecret {
    fn default() -> Self {
        Self::new()
    }
}

impl CredentialSecret {
    pub fn new() -> Self {
        Self {
            service: SERVICE.to_owned(),
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[cfg(test)]
    pub fn with_service(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn cached_map(&self) -> std::sync::MutexGuard<'_, HashMap<String, Option<String>>> {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn get(&self, key: &str) -> Result<Option<String>, GlossError> {
        if let Some(cached) = self.cached_map().get(key).cloned() {
            return Ok(cached);
        }
        let value = match ffi::cred::read(&target_name(&self.service, key))? {
            Some(blob) => Some(decode(blob)?),
            None => None,
        };
        let answer = self
            .cached_map()
            .entry(key.to_owned())
            .or_insert(value)
            .clone();
        Ok(answer)
    }

    pub(crate) fn set(&self, key: &str, value: &str) -> Result<(), GlossError> {
        ffi::cred::write(&target_name(&self.service, key), value.as_bytes())?;
        self.cached_map()
            .insert(key.to_owned(), Some(value.to_owned()));
        Ok(())
    }

    pub(crate) fn delete(&self, key: &str) -> Result<(), GlossError> {
        ffi::cred::delete(&target_name(&self.service, key))?;
        self.cached_map().insert(key.to_owned(), None);
        Ok(())
    }
}

fn decode(blob: Vec<u8>) -> Result<String, GlossError> {
    String::from_utf8(blob)
        .map_err(|err| GlossError::Config(format!("stored secret is not valid utf-8: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> CredentialSecret {
        CredentialSecret::with_service("io.github.losmli010.gloss.test")
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
    fn racing_first_reads_settle_on_one_answer() {
        let store = test_store();
        let key = "test/racing-first-reads";

        let readers: Vec<_> = (0..4)
            .map(|_| {
                let reader = store.clone();
                let key = key.to_owned();
                std::thread::spawn(move || reader.get(&key))
            })
            .collect();
        for reader in readers {
            assert_eq!(
                reader
                    .join()
                    .expect("reader must not panic")
                    .expect("read should succeed"),
                None,
                "concurrent first reads must all observe the same missing entry"
            );
        }
        assert_eq!(
            store.get(key).expect("read after the race should succeed"),
            None,
            "cache must settle on one answer after racing reads"
        );
    }

    #[test]
    fn credential_round_trip_on_real_store() {
        let store = test_store();
        let key = "test/credential-round-trip";

        store.delete(key).expect("preset delete should succeed");
        assert_eq!(store.get(key).expect("read should succeed"), None);

        store.set(key, "sk-test-value").expect("set should succeed");
        assert_eq!(
            store.get(key).expect("read should succeed"),
            Some("sk-test-value".into())
        );

        store
            .set(key, "sk-test-value-2")
            .expect("overwrite should succeed");
        assert_eq!(
            store.get(key).expect("read should succeed"),
            Some("sk-test-value-2".into())
        );

        store.delete(key).expect("delete should succeed");
        assert_eq!(store.get(key).expect("read should succeed"), None);
    }
}
