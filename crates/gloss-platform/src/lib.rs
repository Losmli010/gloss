//! 适配器层：实现 gloss-core 端口，把平台差异收在这一层。
//!
//! 端口实现在平台事件线程上被调用（AX 与 AppKit 的线程约束见各模块头）；
//! 要求主线程的调用（如图标安装）由实现自行判定并降级。全部 C 接口声明
//! 与所有权守卫集中在 `ffi`，业务模块不直接声明外部符号。

#[cfg(test)]
#[path = "../tests/stubs/mod.rs"]
pub(crate) mod stubs;

pub mod appearance;
pub mod clipboard;
pub mod events;
mod ffi;
pub mod locale;
pub mod permissions;
pub mod scene;
pub mod selection;
pub mod storage;

#[cfg(test)]
pub(crate) mod live_test_support {
    pub fn require_accessibility(test_name: &str) {
        assert!(
            crate::ffi::ax::is_process_trusted(),
            "[{test_name}] 运行测试的进程缺少辅助功能授权。修复：系统设置 → 隐私与安全性 → 辅助功能 → 打开运行测试的终端 App（Terminal/iTerm/VS Code）的开关，然后重跑。"
        );
    }
}
