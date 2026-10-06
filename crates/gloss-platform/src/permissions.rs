//! 事件权限的预检与授权引导。
//!
//! 划词链路依赖两项**互相独立**的系统授权，且系统没有把二者放在同一个
//! 开关下，缺失任一都会让划词在对应环节失效：
//!
//! - **输入监控**（kTCCServiceListenEvent）：CGEventTap 的建立前提。此项
//!   **没有公开的预检 API**，状态只能从 tap 建立成败推断（见
//!   `events::mouse` 的同步启动结果），引导方式是打开系统设置的
//!   「输入监控」面板（[`open_input_monitoring_pane`]）。
//! - **辅助功能**（kTCCServiceAccessibility）：AX 选区读取的前提，缺失时
//!   选区读取返回 `gloss_core::model::GlossError::AccessibilityDenied`。
//!   有公开查询 API（[`preflight_accessibility`]），且支持让系统直接弹
//!   授权引导对话框（[`request_accessibility`]）。
//!
//! 授权引导统一走系统级 UI（系统授权对话框或系统设置面板），不经过应用
//! 内对话框。

use std::process::Command;

use gloss_core::model::GlossError;

/// 系统设置「输入监控」面板（kTCCServiceListenEvent 的授权入口）。
const INPUT_MONITORING_PANE: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent";

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

/// 打开系统设置的「输入监控」面板：该项无公开预检 API，tap 建立失败即视
/// 为缺失，把设置面板送到用户面前。只发起不等待（`open` 自身秒退，等它
/// 退出没有价值）；失败只降级记日志，不影响主流程。
pub fn open_input_monitoring_pane() -> bool {
    match Command::new("open").arg(INPUT_MONITORING_PANE).spawn() {
        // spawn 的 Child 有意即时 drop：调用点一次性触发，至多一个短命
        // 僵尸表项、有界不累积（open 自身秒退）；多次触发需先补收尸。
        Ok(_) => true,
        Err(err) => {
            // 打不开设置面板是引导路径的降级，不影响功能主流程。
            gloss_core::log::warn!(
                error = %err,
                "failed to open the Input Monitoring settings pane"
            );
            false
        }
    }
}

/// 判断错误是否属于辅助功能权限语义（引导提示与其它失败区分开）。
pub fn is_accessibility_denied(error: &GlossError) -> bool {
    matches!(error, GlossError::AccessibilityDenied)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessibility_denied_is_recognized() {
        assert!(is_accessibility_denied(&GlossError::AccessibilityDenied));
        assert!(!is_accessibility_denied(&GlossError::SelectionUnavailable));
    }
}
