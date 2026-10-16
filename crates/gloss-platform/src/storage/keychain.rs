//! ConfigStore 的密钥半边：系统安全存储的读写。
//!
//! 走 Security framework 的通用密码（keychain）。密钥永不落明文、不进
//! 日志或错误消息（AGENTS.md 密钥红线）。
//!
//! 带进程内读取缓存（`get` 未命中才访问 keychain，`set`/`delete` 同步更
//! 新缓存）：keychain 的 ACL 按代码签名信任访问方，dev 构建每次重编译签
//! 名都变，不在旧条目信任列表里——每次读取都会弹「允许访问钥匙串」并阻塞
//! 到用户点击。缓存把读弹窗收敛到每进程一次，未命中缓存的首读另按 key
//! 串行（见 [`KeychainSecret.read_gates`] 字段文档），并发首读不会各自撞
//! 一次授权框；写删路径走「取引用删除 + 重建」（见 [`KeychainSecret::set`]
//! 与 [`KeychainSecret::delete_underlying`] 文档）免弹窗、也不受旧条目
//! 所有权阻拦。设置页改密钥经同一实例的 `set` 写入缓存，即时生效，无需
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

/// keychain 条目的服务名：与打包标识一致，避免与其他应用的条目串扰。
const SERVICE: &str = "io.github.losmli010.gloss";

/// errSecInvalidOwnerEdit：条目所有权按创建二进制的代码签名归属，查询式
/// `SecItemDelete` 对非本二进制创建的条目以此拒绝删除。security-framework
/// 系 crate 未导出该常量，本地定义。
const ERR_SEC_INVALID_OWNER_EDIT: i32 = -25244;

/// 密钥存储：以 `account`（即配置里的 `keychain_id`）定位条目，服务名
/// 固定。仅做存取，不理解 key 语义——provider 与 key 的映射在 `Config`
/// 侧维护。
///
/// `Clone` 出的副本经 `Arc` 共享同一份缓存：组装点只建一个实例、克隆的
/// Arc 分发给引擎与设置页，读写因此天然汇合到同一缓存。
#[derive(Debug, Clone)]
pub struct KeychainSecret {
    /// 以服务名定位条目，与应用条目隔离（换名入口 `with_service` 仅测试可用）。
    service: String,
    /// 读取缓存：值含 `None`（条目不存在也被缓存），写删路径同步更新。
    cache: Arc<Mutex<HashMap<String, Option<String>>>>,
    /// 未命中缓存首读的按 key 闸门：ACL 授权框阻塞首读期间（预热与任务
    /// 请求可能并发读同一条目），后来的读者在闸门上等首读回填缓存，而不是
    /// 各自撞一次系统授权框——「读弹窗每进程至多一次」因此对并发读取也
    /// 成立。闸门只包住 keychain 读本身；写删路径不经它（键数由配置里的
    /// keychain_id 决定，个位数级别，map 进程内只增不减不构成无界）。
    read_gates: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    /// 底层读计数：闸门的载荷契约（并发首读只发生一次底层读）在真
    /// keychain 上不可观测，仅测试构建累计，测试据此断言。
    #[cfg(test)]
    raw_reads: Arc<AtomicUsize>,
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
            read_gates: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(test)]
            raw_reads: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// 用指定服务名构造（测试隔离用）。
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

    /// 取某 key 的首读闸门（无则建，克隆 Arc 出去再锁，避免持 map 锁等
    /// 闸门）；锁中毒口径同缓存。
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
    /// 读密钥；`None` 表示条目不存在（明确区别于读取失败）。
    ///
    /// API key 约定为 UTF-8 文本；非 UTF-8 字节按 replacement 降级，不
    /// 视为错误——密钥内容只该由本应用写入。
    ///
    /// 未命中缓存的首读可能被 ACL 授权框阻塞到用户应答；在闸门上等待的
    /// 并发读者是同步阻塞（不在 await 点上，途中的取消不生效，见模块内
    /// 闸门文档）。回填让并发写入胜出：读期间发生的 set/delete 已把缓存
    /// 更新到更新的状态，按缓存值作答、本读的旧值作废。
    pub fn get(&self, key: &str) -> Result<Option<String>, GlossError> {
        if let Some(cached) = self.cached(key) {
            return Ok(cached);
        }
        // 先取闸门再上锁，锁内双检：等闸门期间并发的首读可能已回填缓存。
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

    /// 写入（或覆盖）密钥；成功才更新缓存：失败时缓存保持旧值。
    ///
    /// 走「取引用删除 + 重建」：条目 ACL 按创建时的代码签名信任访问方
    /// （dev 构建每次重编译都换签名，旧条目不认新二进制），原地修改会触发
    /// 系统授权弹窗；取引用删除不经过条目 ACL 与所有权（见
    /// [`Self::delete_underlying`] 文档），重建出的新条目归当前二进制所
    /// 有，本构建内的后续读写免弹窗。删除成功而重建失败时，尽力用缓存里
    /// 的旧值重建原条目（重建路径不再需要授权，失败多为环境性）；连重建
    /// 也失败则如实上报，由设置页的既有失败通知兜底。残余风险：本进程从
    /// 未成功读过该条目（缓存为空）时回滚无源，旧值不可恢复——不删除前
    /// 补读是为了不把读路径的授权弹窗重新引进写路径。
    pub fn set(&self, key: &str, value: &str) -> Result<(), GlossError> {
        self.delete_underlying(key)?;
        if let Err(err) = passwords::set_generic_password(&self.service, key, value.as_bytes()) {
            // 尽力回滚旧条目（值取自进程内缓存，与写入前状态一致）；回滚
            // 自身的失败只留痕——上报以最初的写入失败为准。
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

    /// 删除密钥；条目本就不存在时视为成功（幂等，方便清理路径直调）。
    pub fn delete(&self, key: &str) -> Result<(), GlossError> {
        self.delete_underlying(key)?;
        self.cached_map().insert(key.to_owned(), None);
        Ok(())
    }

    /// 删除底层 keychain 条目（含同名重复项）；条目不存在视为成功。
    ///
    /// 走「取引用 + `SecKeychainItemDelete`」而不是查询式 `SecItemDelete`：
    /// keychain 条目所有权按创建二进制的代码签名归属，查询式删除对非本
    /// 二进制创建的条目返回 errSecInvalidOwnerEdit 拒删——dev 构建每次重
    /// 编译都换签名，旧条目对当前二进制必然是外部所有者；引用式删除不受
    /// 条目所有权限制，同名重复项也一并清掉。取引用只按 service/account
    /// 定位条目、不解密，不触发授权弹窗。
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
            // 该查询形态（generic_password + load_refs）上游只会交回
            // keychain item 引用，其它形态按上游契约不可达；真发生说明
            // 系统行为越出预期，留痕后跳过，不静默吞掉。
            let SearchResult::Ref(Reference::KeychainItem(item)) = result else {
                gloss_core::log::warn!(
                    service = %self.service,
                    "keychain search returned a non-item reference; entry deletion skipped"
                );
                continue;
            };
            // SAFETY: item 是 SecItemCopyMatching 交出的存活 SecKeychainItem
            // （TCFType 包装保证引用非空、Drop 前 CFRetain 持有），delete
            // 只消费该引用本身，不读条目数据、不留存越过本语句的指针。
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
