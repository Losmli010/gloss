//! ConfigStore 的密钥半边：系统安全存储的读写。
//!
//! macOS 走 Security framework 的通用密码（keychain）；Windows 的 DPAPI
//! 方案按计划留 stub——调用返回 [`GlossError::Config`]，落地时替换平台
//! 分支的实现即可，端口与调用方不动。密钥永不落明文、不进日志或错误
//! 消息（AGENTS.md 密钥红线）。

use gloss_core::model::GlossError;

/// keychain 条目的服务名：与打包标识一致，避免与其他应用的条目串扰。
const SERVICE: &str = "io.github.losmli010.gloss";

/// errSecItemNotFound：keychain 条目不存在。security-framework crate 未
/// 导出该常量，本地定义——不值得为单个常量把 security-framework-sys 升
/// 成直接依赖。仅 macOS 后端读取：其他平台的 stub 不引用它。
#[cfg(target_os = "macos")]
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

/// 密钥存储：以 `account`（即配置里的 `keychain_id`）定位条目，服务名
/// 固定。仅做存取，不理解 key 语义——provider 与 key 的映射在 `Config`
/// 侧维护。
#[derive(Debug, Clone)]
pub struct KeychainSecret {
    /// macOS 后端以服务名定位条目；stub 平台暂不读取，但保留字段维持
    /// `with_service` 的跨平台 API 与即将落地的 Windows DPAPI 对称性。
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    service: String,
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
        }
    }

    /// 用指定服务名构造（测试隔离与自定义部署用）。
    pub fn with_service(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }
}

/// macOS：Security framework 通用密码真实现。
#[cfg(target_os = "macos")]
impl KeychainSecret {
    /// 读密钥；`None` 表示条目不存在（明确区别于读取失败）。
    ///
    /// API key 约定为 UTF-8 文本；非 UTF-8 字节按 replacement 降级，不
    /// 视为错误——密钥内容只该由本应用写入。
    pub fn get(&self, key: &str) -> Result<Option<String>, GlossError> {
        match security_framework::passwords::get_generic_password(&self.service, key) {
            Ok(password) => Ok(Some(String::from_utf8_lossy(&password).into_owned())),
            Err(err) if err.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
            Err(err) => Err(GlossError::Config(format!("keychain read failed: {err}"))),
        }
    }

    /// 写入（或覆盖）密钥。
    pub fn set(&self, key: &str, value: &str) -> Result<(), GlossError> {
        security_framework::passwords::set_generic_password(&self.service, key, value.as_bytes())
            .map_err(|err| GlossError::Config(format!("keychain write failed: {err}")))
    }

    /// 删除密钥；条目本就不存在时视为成功（幂等，方便清理路径直调）。
    pub fn delete(&self, key: &str) -> Result<(), GlossError> {
        match security_framework::passwords::delete_generic_password(&self.service, key) {
            Ok(()) => Ok(()),
            Err(err) if err.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
            Err(err) => Err(GlossError::Config(format!("keychain delete failed: {err}"))),
        }
    }
}

/// 其他平台：stub。Windows 将换 DPAPI 实现，端口与调用方不变。
#[cfg(not(target_os = "macos"))]
impl KeychainSecret {
    /// 未实现平台的读取：返回明确错误而非 `None`——`None` 语义是「未
    /// 设置」，与「存储不可用」是两回事，混用会静默放走空密钥。
    pub fn get(&self, key: &str) -> Result<Option<String>, GlossError> {
        let _ = key;
        Err(unsupported())
    }

    /// 未实现平台的写入：明确失败，调用方按错误降级。
    pub fn set(&self, key: &str, value: &str) -> Result<(), GlossError> {
        let _ = (key, value);
        Err(unsupported())
    }

    /// 未实现平台的删除：同上明确失败。
    pub fn delete(&self, key: &str) -> Result<(), GlossError> {
        let _ = key;
        Err(unsupported())
    }
}

/// stub 平台的统一错误文本（不含 key 与密钥内容）。
#[cfg(not(target_os = "macos"))]
fn unsupported() -> GlossError {
    GlossError::Config("keychain storage is not implemented on this platform".into())
}

/// stub 平台（Linux CI 等）的错误路径：get/set/delete 全部明确失败。
#[cfg(all(test, not(target_os = "macos")))]
mod stub_tests {
    use super::*;

    #[test]
    fn all_methods_report_unsupported_platform() {
        let store = KeychainSecret::with_service("io.github.losmli010.gloss.test");
        assert!(matches!(store.get("test/k"), Err(GlossError::Config(_))));
        assert!(matches!(
            store.set("test/k", "v"),
            Err(GlossError::Config(_))
        ));
        assert!(matches!(store.delete("test/k"), Err(GlossError::Config(_))));
    }
}

/// macOS 真机路径：先在测试服务名下验证「未设置 = None」与删除幂等
/// （纯 keychain 行为，不写入任何密钥值）。
#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    /// 测试用服务名：与出厂条目隔离，条目随测随清。
    fn test_store() -> KeychainSecret {
        KeychainSecret::with_service("io.github.losmli010.gloss.test")
    }

    /// 未设置时读返回 None 而不是错误（区分「未设置」与「存储不可用」）。
    #[test]
    fn missing_entry_reads_as_none() {
        let store = test_store();
        let key = "test/missing-entry-reads-as-none";

        // 先清一次，保证前置状态与上次运行残留无关。
        store.delete(key).ok();
        assert_eq!(store.get(key).expect("read should succeed"), None);
    }

    /// 未找到条目的删除是幂等成功。
    #[test]
    fn delete_missing_entry_is_ok() {
        let store = test_store();
        store
            .delete("test/delete-missing-entry-is-ok")
            .expect("delete of missing entry should succeed");
    }
}

/// 密钥读写往返真机验证（macOS）：写 → 读一致 → 覆盖 → 删除后 None。
/// 直接操作测试服务名下的 keychain 条目，结束清理，不产生明文落盘。
///
/// opt-in（`cargo test -p gloss-platform -- --ignored`）：keychain 写入
/// 在部分受控环境（沙箱、CI runner）会被系统拒绝或需要授权，不宜作为
/// 无条件门禁；只读行为由上方非忽略测试覆盖。
#[cfg(all(test, target_os = "macos"))]
mod macos_live_tests {
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
