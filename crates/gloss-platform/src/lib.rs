//! Gloss 适配器层：实现 gloss-core 端口，隔离平台差异。

pub mod events;
pub mod selection;

/// L4 opt-in 真机测试的授权前置检查（macOS/Windows 的测试编译均可用）。
///
/// macOS：rdev 监听/注入与 AX 读选区都要求**运行测试的进程**（通常是
/// 终端或 IDE）持有辅助功能授权；缺授权时系统表现为静默忽略，各测试
/// 会以不同的失败形态超时——这里统一前置拦截并给出可操作的授权步骤。
/// Windows：无 macOS 式授权模型，交互会话内直接可用，直通。
#[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
pub(crate) mod live_test_support {
    /// 前置断言：macOS 未授权时以可操作的修复指引快速失败，而不是让
    /// 各测试以「注入被忽略 / tap 未建立」的间接形态超时。
    #[cfg(target_os = "macos")]
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

    /// Windows 无对应授权模型：直通。
    #[cfg(not(target_os = "macos"))]
    pub fn require_accessibility(test_name: &str) {
        let _ = test_name;
    }
}
