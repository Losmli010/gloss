//! Gloss 适配器层：实现 gloss-core 端口，隔离平台差异。

pub mod appearance;
pub mod engine;
pub mod events;
pub mod selection;
pub mod storage;

/// L4 opt-in 真机测试的授权前置检查。
///
/// rdev 注入与 AX 读选区都要求**运行测试的进程**（通常是终端或 IDE）
/// 持有辅助功能授权；缺授权时系统表现为静默忽略，各测试会以不同的失
/// 败形态超时——这里统一前置拦截并给出可操作的授权步骤。
#[cfg(test)]
pub(crate) mod live_test_support {
    /// 前置断言：未授权时以可操作的修复指引快速失败，而不是让各测试以
    /// 「注入被忽略 / tap 未建立」的间接形态超时。
    pub fn require_accessibility(test_name: &str) {
        // AXError.h：当前进程是否已获辅助功能授权（签名须与
        // selection/accessibility.rs 内的声明一致）。
        #[link(name = "ApplicationServices", kind = "framework")]
        unsafe extern "C" {
            fn AXIsProcessTrusted() -> u8;
        }
        // SAFETY: 纯查询型 FFI，无前置条件。
        let trusted = unsafe { AXIsProcessTrusted() } != 0;
        assert!(
            trusted,
            "[{test_name}] 运行测试的进程缺少辅助功能授权。修复：系统设置 → 隐私与安全性 → 辅助功能 → 打开运行测试的终端 App（Terminal/iTerm/VS Code）的开关，然后重跑。"
        );
    }
}
