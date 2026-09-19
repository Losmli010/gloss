//! Gloss 适配器层：实现 gloss-core 端口，隔离平台差异。

// 测试桩的唯一源在 tests/stubs/，这里把同一份源并入库内单测编译。
#[cfg(test)]
#[path = "../tests/stubs/mod.rs"]
pub(crate) mod stubs;

pub mod appearance;
pub mod engine;
pub mod events;
pub mod selection;
pub mod storage;

#[cfg(test)]
pub(crate) mod live_test_support {
    pub fn require_accessibility(test_name: &str) {
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
