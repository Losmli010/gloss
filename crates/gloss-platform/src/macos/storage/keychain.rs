//! 配置存储的密钥半边（系统（macOS）绑定）：钥匙串密钥句柄的系统调用面。
//!
//! 走系统安全框架的通用密码（钥匙串）。语义骨架——进程内读取缓存、读写删
//! 编排、缺条目读空与错误降级——住平台中立共享层（common::secret），本模块
//! 只留系统安全框架调用面与钥匙串特有的两段编排：未命中缓存首读的按条目
//! 闸门，与写删路径的“取引用删除加重建”。密钥永不落明文、不进日志或错误
//! 消息——密钥红线。
//!
//! 钥匙串的访问控制按代码签名信任访问方，调试构建每次重编译签名都变，不在
//! 旧条目信任列表里——未命中缓存的底层读会弹“允许访问钥匙串”并阻塞到用户
//! 点击，闸门把读弹窗收敛到每进程一次；写删路径重建出的新条目归当前二进制
//! 所有，本构建内的后续读写免弹窗、也不受旧条目所有权阻拦。设置页改密钥经
//! 同一实例写入缓存，即时生效，无需重启。

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

use crate::common::secret::{CachedSecretStore, SERVICE, SecretCache, SecretIo};

const ERR_SEC_INVALID_OWNER_EDIT: i32 = -25244;

#[derive(Debug, Clone)]
pub struct KeychainSecret {
    store: CachedSecretStore<KeychainIo>,
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
        Self::assemble(SERVICE)
    }

    #[cfg(test)]
    pub fn with_service(service: impl Into<String>) -> Self {
        Self::assemble(service)
    }

    fn assemble(service: impl Into<String>) -> Self {
        let io = KeychainIo::new();
        #[cfg(test)]
        let raw_reads = io.raw_reads_handle();
        Self {
            store: CachedSecretStore::with_io(service, io),
            #[cfg(test)]
            raw_reads,
        }
    }

    pub(crate) fn get(&self, key: &str) -> Result<Option<String>, GlossError> {
        self.store.get(key)
    }

    pub(crate) fn set(&self, key: &str, value: &str) -> Result<(), GlossError> {
        self.store.set(key, value)
    }

    pub(crate) fn delete(&self, key: &str) -> Result<(), GlossError> {
        self.store.delete(key)
    }
}

#[derive(Debug, Clone)]
struct KeychainIo {
    read_gates: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    #[cfg(test)]
    raw_reads: Arc<AtomicUsize>,
}

impl KeychainIo {
    fn new() -> Self {
        Self {
            read_gates: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(test)]
            raw_reads: Arc::new(AtomicUsize::new(0)),
        }
    }

    #[cfg(test)]
    fn raw_reads_handle(&self) -> Arc<AtomicUsize> {
        Arc::clone(&self.raw_reads)
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

    fn delete_underlying(&self, service: &str, key: &str) -> Result<(), GlossError> {
        let results = match ItemSearchOptions::new()
            .class(ItemClass::generic_password())
            .service(service)
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
                    service = %service,
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
                     remove it once in Keychain Access (login keychain, service {service}, \
                     account {key}) and save again"
                )));
            }
            let err = SfError::from_code(status);
            return Err(GlossError::Config(format!("keychain delete failed: {err}")));
        }
        Ok(())
    }
}

impl SecretIo for KeychainIo {
    fn read(
        &self,
        cache: &SecretCache,
        service: &str,
        key: &str,
    ) -> Result<Option<String>, GlossError> {
        let gate = self.read_gate(key);
        let _gate = gate.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(cached) = cache.cached(key) {
            return Ok(cached);
        }
        #[cfg(test)]
        self.raw_reads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let value = match passwords::get_generic_password(service, key) {
            Ok(password) => Some(String::from_utf8_lossy(&password).into_owned()),
            Err(err) if err.code() == errSecItemNotFound => None,
            Err(err) => return Err(GlossError::Config(format!("keychain read failed: {err}"))),
        };
        Ok(cache.settle(key, value))
    }

    fn write(
        &self,
        cache: &SecretCache,
        service: &str,
        key: &str,
        value: &str,
    ) -> Result<(), GlossError> {
        self.delete_underlying(service, key)?;
        if let Err(err) = passwords::set_generic_password(service, key, value.as_bytes()) {
            if let Some(Some(old)) = cache.cached(key)
                && let Err(rollback) = passwords::set_generic_password(service, key, old.as_bytes())
            {
                gloss_core::log::warn!(
                    error = %rollback,
                    "keychain rollback of the previous secret failed"
                );
            }
            return Err(GlossError::Config(format!("keychain write failed: {err}")));
        }
        Ok(())
    }

    fn remove(&self, service: &str, key: &str) -> Result<(), GlossError> {
        self.delete_underlying(service, key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> KeychainSecret {
        KeychainSecret::with_service("io.github.losmli010.gloss.test")
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
        let service = "io.github.losmli010.gloss.test";
        let store = KeychainSecret::with_service(service);
        let key = "test/foreign-owned-entry-reference-delete";

        store.delete(key).expect("preset delete should succeed");
        let add = std::process::Command::new("security")
            .args([
                "add-generic-password",
                "-s",
                service,
                "-a",
                key,
                "-w",
                "sk-foreign-owner",
            ])
            .output()
            .expect("security cli should launch");
        assert!(add.status.success(), "cli must create the foreign entry");
        let found = std::process::Command::new("security")
            .args(["find-generic-password", "-s", service, "-a", key])
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
            .args(["find-generic-password", "-s", service, "-a", key])
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
