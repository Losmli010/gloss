//! ConfigStore 的密钥半边：系统安全存储的读写。
//!
//! 走 Security framework 的通用密码（keychain）。密钥永不落明文、不进
//! 日志或错误消息（AGENTS.md 密钥红线）。
//!
//! 带进程内读取缓存（`get` 未命中才访问 keychain，`set`/`delete` 同步更
//! 新缓存）：keychain 的 ACL 按代码签名信任读取方，dev 构建每次重编译签
//! 名都变，不在条目信任列表里——每次读取都会弹「允许访问钥匙串」并阻塞
//! 到用户点击。缓存把弹窗从每任务收敛到每进程一次；设置页改密钥经同一
//! 实例的 `set` 写入缓存，即时生效，无需重启。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use gloss_core::model::GlossError;

/// keychain 条目的服务名：与打包标识一致，避免与其他应用的条目串扰。
const SERVICE: &str = "io.github.losmli010.gloss";

/// errSecItemNotFound：keychain 条目不存在。security-framework crate 未
/// 导出该常量，本地定义——不值得为单个常量把 security-framework-sys 升
/// 成直接依赖。
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

/// 密钥存储：以 `account`（即配置里的 `keychain_id`）定位条目，服务名
/// 固定。仅做存取，不理解 key 语义——provider 与 key 的映射在 `Config`
/// 侧维护。
///
/// `Clone` 出的副本经 `Arc` 共享同一份缓存：组装点只建一个实例、克隆的
/// Arc 分发给引擎与设置页，读写因此天然汇合到同一缓存。
#[derive(Debug, Clone)]
pub struct KeychainSecret {
    /// 以服务名定位条目，与应用条目隔离（测试与自定义部署可换名）。
    service: String,
    /// 读取缓存：值含 `None`（条目不存在也被缓存），写删路径同步更新。
    cache: Arc<Mutex<HashMap<String, Option<String>>>>,
}

impl Default for KeychainSecret {
    fn default() -> Self {
        Self::new()
    }
}

impl KeychainSecret {
    /// 用出厂服务名构造。
    pub fn new() -> Self {
        Self {
            service: SERVICE.to_owned(),
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 用指定服务名构造（测试隔离与自定义部署用）。
    pub fn with_service(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 取缓存；锁中毒（持锁期间 panic）取内值继续——缓存的临界区只做
    /// HashMap 读写，中毒意味着进程已处于未定义状态，不能因此拒读密钥。
    fn cached(&self, key: &str) -> Option<Option<String>> {
        self.cached_map().get(key).cloned()
    }

    fn cached_map(&self) -> std::sync::MutexGuard<'_, HashMap<String, Option<String>>> {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl KeychainSecret {
    /// 读密钥；`None` 表示条目不存在（明确区别于读取失败）。
    ///
    /// API key 约定为 UTF-8 文本；非 UTF-8 字节按 replacement 降级，不
    /// 视为错误——密钥内容只该由本应用写入。
    pub fn get(&self, key: &str) -> Result<Option<String>, GlossError> {
        if let Some(cached) = self.cached(key) {
            return Ok(cached);
        }
        let value = match security_framework::passwords::get_generic_password(&self.service, key) {
            Ok(password) => Some(String::from_utf8_lossy(&password).into_owned()),
            Err(err) if err.code() == ERR_SEC_ITEM_NOT_FOUND => None,
            Err(err) => return Err(GlossError::Config(format!("keychain read failed: {err}"))),
        };
        self.cached_map().insert(key.to_owned(), value.clone());
        Ok(value)
    }

    /// 写入（或覆盖）密钥。写入成功才更新缓存：失败时缓存保持旧值，与
    /// keychain 真实状态一致。
    pub fn set(&self, key: &str, value: &str) -> Result<(), GlossError> {
        security_framework::passwords::set_generic_password(&self.service, key, value.as_bytes())
            .map_err(|err| GlossError::Config(format!("keychain write failed: {err}")))?;
        self.cached_map()
            .insert(key.to_owned(), Some(value.to_owned()));
        Ok(())
    }

    /// 删除密钥；条目本就不存在时视为成功（幂等，方便清理路径直调）。
    pub fn delete(&self, key: &str) -> Result<(), GlossError> {
        match security_framework::passwords::delete_generic_password(&self.service, key) {
            Ok(()) => {}
            Err(err) if err.code() == ERR_SEC_ITEM_NOT_FOUND => {}
            Err(err) => return Err(GlossError::Config(format!("keychain delete failed: {err}"))),
        }
        self.cached_map().insert(key.to_owned(), None);
        Ok(())
    }
}

/// 真机路径：先在测试服务名下验证「未设置 = None」与删除幂等（纯
/// keychain 行为，不写入任何密钥值）。
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
