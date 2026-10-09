//! 事件权限的预检与授权引导。
//!
//! 划词链路的取材授权只有**辅助功能**（kTCCServiceAccessibility）一项：
//! AX 选区读取的前提，缺失时选区读取返回
//! `gloss_core::model::GlossError::AccessibilityDenied`；listen-only 事件
//! tap 在辅助功能授权下同样可建立，手势不索取第二项系统授权。有公开查询
//! API（[`preflight_accessibility`]），且支持让系统直接弹授权引导对话框
//! （[`request_accessibility`]）。
//!
//! 授权引导统一走系统级 UI（系统授权对话框），不经过应用内对话框。

/// 当前进程是否已获辅助功能授权（AX 选区读取可用）；纯查询，无副作用。
pub fn preflight_accessibility() -> bool {
    crate::ffi::ax::is_process_trusted()
}

/// 查询辅助功能授权，未授权时顺带弹系统授权引导对话框（内含「打开系统
/// 设置」入口）。返回值仍是查询时刻的授权状态；系统对话框非阻塞，最终
/// 结果以用户在系统设置里的操作为准。调用方应把调用频次控制在启动预检
/// 这类低频节点上。
pub fn request_accessibility() -> bool {
    crate::ffi::ax::is_process_trusted_prompting()
}
