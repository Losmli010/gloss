//! 适配器层：实现核心层（gloss-core）的端口，把平台差异收在这一层。
//!
//! 平台实现按编译目标分目录：系统（macOS）的现有实现、系统（Windows）
//! 的同名镜像占位与其余目标的空占位各占一个平台目录。平台无关的事件线程
//! 骨架（event_thread）不随目标变化，由两侧平台目录共用。占位实现未实现
//! 的端口一律返回平台未支持（PlatformUnsupported）。本入口按目标声明平台
//! 目录，再把子模块重新导出为平台无关路径——消费方的引用面不随编译
//! 目标变化，平台专属依赖也只在对应目标编译。装配面收敛在装配契约模块
//! （bootstrap）：入口与应用层只经平台句柄（Platform）取平台能力，不
//! 引用平台具体类型。端口实现在平台事件线程上被调用，线程约束见各平台
//! 模块头；要求主线程的调用（如图标安装）由实现自行判定并降级。全部
//! 系统接口声明与所有权守卫集中在各平台的外部函数接口（FFI）模块，
//! 业务模块不直接声明外部符号。

#[cfg(test)]
#[path = "../tests/stubs/mod.rs"]
pub(crate) mod stubs;

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod bootstrap;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) mod event_thread;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
pub(crate) use macos::ffi;
#[cfg(target_os = "macos")]
pub use macos::{
    appearance, clipboard, events, locale, menu, permissions, scene, selection, storage,
};

#[cfg(target_os = "windows")]
pub(crate) use windows::ffi;

#[cfg(target_os = "windows")]
pub use windows::{
    appearance, clipboard, events, locale, menu, permissions, scene, selection, storage,
};

#[cfg(all(test, target_os = "macos"))]
pub(crate) mod live_test_support {
    pub fn require_accessibility(test_name: &str) {
        assert!(
            crate::ffi::ax::is_process_trusted(),
            "[{test_name}] 运行测试的进程缺少辅助功能授权。修复：系统设置 → 隐私与安全性 → 辅助功能 → 打开运行测试的终端 App（Terminal/iTerm/VS Code）的开关，然后重跑。" // i18n:allow 测试专用的授权指引，仅终端输出不进界面文案表
        );
    }
}
