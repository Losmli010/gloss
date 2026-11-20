//! 配置存储的密钥半边（系统（Windows）绑定）：凭据管理器密钥句柄的系统
//! 调用面。
//!
//! 通用凭据按“服务名加键串”拼目标名寻址，载荷为密钥的原始字节，字符串
//! 编解码在本调用面完成。语义骨架——进程内读取缓存、读写删编排、缺条目
//! 读空与错误降级——住平台中立共享层（common::secret），本模块只留凭据
//! 应用编程接口的系统调用面。密钥永不落明文、不进日志或错误消息——密钥
//! 红线。与系统（macOS）钥匙串半边不同，凭据管理器不按访问方签名设访问
//! 控制，读取无弹窗，因此无需读入门禁；并发首读各自撞一次系统调用无碍
//! （幂等读），缓存以先落者为准。任务链路每次执行都要取一次密钥，共享缓
//! 存把系统调用收敛到每进程每键一次，设置页改密钥经同一实例写入缓存即时
//! 生效，无需重启。

use gloss_core::model::GlossError;

use crate::common::secret::{CachedSecretStore, SERVICE, SecretCache, SecretIo};
use crate::ffi;

#[derive(Debug, Clone)]
pub struct CredentialSecret {
    store: CachedSecretStore<CredIo>,
}

impl Default for CredentialSecret {
    fn default() -> Self {
        Self::new()
    }
}

impl CredentialSecret {
    pub fn new() -> Self {
        Self::assemble(SERVICE)
    }

    #[cfg(test)]
    pub fn with_service(service: impl Into<String>) -> Self {
        Self::assemble(service)
    }

    fn assemble(service: impl Into<String>) -> Self {
        Self {
            store: CachedSecretStore::with_io(service, CredIo),
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
struct CredIo;

impl SecretIo for CredIo {
    fn read(
        &self,
        _cache: &SecretCache,
        service: &str,
        key: &str,
    ) -> Result<Option<String>, GlossError> {
        match ffi::cred::read(&target_name(service, key))? {
            Some(blob) => Ok(Some(decode(blob)?)),
            None => Ok(None),
        }
    }

    fn write(
        &self,
        _cache: &SecretCache,
        service: &str,
        key: &str,
        value: &str,
    ) -> Result<(), GlossError> {
        ffi::cred::write(&target_name(service, key), value.as_bytes())
    }

    fn remove(&self, service: &str, key: &str) -> Result<(), GlossError> {
        ffi::cred::delete(&target_name(service, key))
    }
}

fn target_name(service: &str, key: &str) -> String {
    format!("{service}/{key}")
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
    #[ignore = "credential write touches the real credential store and needs an \
                unrestricted session; run with --ignored"]
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
