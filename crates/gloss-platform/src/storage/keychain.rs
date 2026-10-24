//! 配置存储的密钥半边：系统安全存储的读写。
//!
//! 走系统安全框架的通用密码（钥匙串）。密钥永不落明文、不进
//! 日志或错误消息——密钥红线。
//!
//! 带进程内读取缓存（读未命中才访问钥匙串，写删同步更
//! 新缓存）：钥匙串的访问控制按代码签名信任访问方，调试构建每次重编译签
//! 名都变，不在旧条目信任列表里——每次读取都会弹“允许访问钥匙串”并阻塞
//! 到用户点击。缓存把读弹窗收敛到每进程一次，未命中缓存的首读另按条目
//! 键串行（见读入门禁的文档），并发首读不会各自撞
//! 一次授权框；写删路径走“取引用删除加重建”（见写入与底层删除的
//! 文档）免弹窗、也不受旧条目
//! 所有权阻拦。设置页改密钥经同一实例写入缓存，即时生效，无需
//! 重启。

use std::collections::HashMap;
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};

use core_foundation::base::TCFType;
use gloss_core::model::GlossError;
use security_framework::base::Error as SfError;
use security_framework::item::{ItemClass, ItemSearchOptions, Limit, Reference, SearchResult};
use security_framework::passwords;
use security_framework_sys::base::{errSecItemNotFound, errSecSuccess};
use security_framework_sys::keychain_item::SecKeychainItemDelete;

const SERVICE: &str = "io.github.losmli010.gloss";

const ERR_SEC_INVALID_OWNER_EDIT: i32 = -25244;

#[derive(Debug, Clone)]
pub struct KeychainSecret {
    service: String,
    cache: Arc<Mutex<HashMap<String, Option<String>>>>,
    read_gates: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    #[cfg(test)]
    raw_reads: Arc<AtomicUsize>,
}

impl Default for KeychainSecret {
    fn default() -> Self {
        Self::new()
    }
}

impl KeychainSecret {
    pub fn new() -> Self {
        Self {
            service: SERVICE.to_owned(),
            cache: Arc::new(Mutex::new(HashMap::new())),
            read_gates: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(test)]
            raw_reads: Arc::new(AtomicUsize::new(0)),
        }
    }

    #[cfg(test)]
    pub fn with_service(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            cache: Arc::new(Mutex::new(HashMap::new())),
            read_gates: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(test)]
            raw_reads: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn cached(&self, key: &str) -> Option<Option<String>> {
        self.cached_map().get(key).cloned()
    }

    fn cached_map(&self) -> std::sync::MutexGuard<'_, HashMap<String, Option<String>>> {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn read_gate(&self, key: &str) -> Arc<Mutex<()>> {
        Arc::clone(
            self.read_gates
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .entry(key.to_owned())
                .or_default(),
        )
    }
}

impl KeychainSecret {
    pub fn get(&self, key: &str) -> Result<Option<String>, GlossError> {
        if let Some(cached) = self.cached(key) {
            return Ok(cached);
        }
        let gate = self.read_gate(key);
        let _gate = gate.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(cached) = self.cached(key) {
            return Ok(cached);
        }
        #[cfg(test)]
        self.raw_reads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let value = match passwords::get_generic_password(&self.service, key) {
            Ok(password) => Some(String::from_utf8_lossy(&password).into_owned()),
            Err(err) if err.code() == errSecItemNotFound => None,
            Err(err) => return Err(GlossError::Config(format!("keychain read failed: {err}"))),
        };
        let answer = self
            .cached_map()
            .entry(key.to_owned())
            .or_insert(value)
            .clone();
        Ok(answer)
    }

    pub fn set(&self, key: &str, value: &str) -> Result<(), GlossError> {
        self.delete_underlying(key)?;
        if let Err(err) = passwords::set_generic_password(&self.service, key, value.as_bytes()) {
            if let Some(Some(old)) = self.cached(key)
                && let Err(rollback) =
                    passwords::set_generic_password(&self.service, key, old.as_bytes())
            {
                gloss_core::log::warn!(
                    error = %rollback,
                    "keychain rollback of the previous secret failed"
                );
            }
            return Err(GlossError::Config(format!("keychain write failed: {err}")));
        }
        self.cached_map()
            .insert(key.to_owned(), Some(value.to_owned()));
        Ok(())
    }

    pub fn delete(&self, key: &str) -> Result<(), GlossError> {
        self.delete_underlying(key)?;
        self.cached_map().insert(key.to_owned(), None);
        Ok(())
    }

    fn delete_underlying(&self, key: &str) -> Result<(), GlossError> {
        let results = match ItemSearchOptions::new()
            .class(ItemClass::generic_password())
            .service(&self.service)
            .account(key)
            .load_refs(true)
            .limit(Limit::All)
            .search()
        {
            Ok(results) => results,
            Err(err) if err.code() == errSecItemNotFound => return Ok(()),
            Err(err) => return Err(GlossError::Config(format!("keychain search failed: {err}"))),
        };
        for result in results {
            let SearchResult::Ref(Reference::KeychainItem(item)) = result else {
                gloss_core::log::warn!(
                    service = %self.service,
                    "keychain search returned a non-item reference; entry deletion skipped"
                );
                continue;
            };
            // SAFETY: `item` is a live `SecKeychainItem` handed over by
            // `SecItemCopyMatching` (the `TCFType` wrapper guarantees a non-null
            // reference retained via `CFRetain` before `Drop`); the delete call
            // only consumes the reference itself, reads no item data, and keeps no
            // pointer past this statement.
            let status = unsafe { SecKeychainItemDelete(item.as_concrete_TypeRef()) };
            if status == errSecSuccess || status == errSecItemNotFound {
                continue;
            }
            if status == ERR_SEC_INVALID_OWNER_EDIT {
                return Err(GlossError::Config(format!(
                    "keychain delete failed: the entry is owned by another build of gloss; \
                     remove it once in Keychain Access (login keychain, service {}, \
                     account {key}) and save again",
                    self.service
                )));
            }
            let err = SfError::from_code(status);
            return Err(GlossError::Config(format!("keychain delete failed: {err}")));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> KeychainSecret {
        KeychainSecret::with_service("io.github.losmli010.gloss.test")
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
    fn racing_first_reads_settle_on_the_cached_value() {
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
            store.raw_reads.load(std::sync::atomic::Ordering::Relaxed),
            1,
            "racing first reads must collapse to exactly one underlying read"
        );
    }

    #[test]
    #[ignore = "creating a foreign-owned entry writes the real keychain via the security CLI, \
                which needs an unrestricted session; run with --ignored"]
    fn foreign_owned_entry_is_removed_by_reference_delete() {
        let store = test_store();
        let key = "test/foreign-owned-entry-reference-delete";

        store.delete(key).expect("preset delete should succeed");
        let add = std::process::Command::new("security")
            .args([
                "add-generic-password",
                "-s",
                &store.service,
                "-a",
                key,
                "-w",
                "sk-foreign-owner",
            ])
            .output()
            .expect("security cli should launch");
        assert!(add.status.success(), "cli must create the foreign entry");
        let found = std::process::Command::new("security")
            .args(["find-generic-password", "-s", &store.service, "-a", key])
            .output()
            .expect("security cli should launch");
        assert!(
            found.status.success(),
            "foreign entry must exist before delete"
        );

        store
            .delete(key)
            .expect("reference delete should remove the foreign-owned entry");

        let gone = std::process::Command::new("security")
            .args(["find-generic-password", "-s", &store.service, "-a", key])
            .output()
            .expect("security cli should launch");
        assert!(
            !gone.status.success(),
            "foreign entry must be gone after delete"
        );
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    #[test]
    #[ignore = "keychain write needs an unrestricted session; run with --ignored"]
    fn keychain_round_trip_on_real_store() {
        let store = KeychainSecret::with_service("io.github.losmli010.gloss.test");
        let key = "test/keychain-round-trip";

        store.delete(key).ok();
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
