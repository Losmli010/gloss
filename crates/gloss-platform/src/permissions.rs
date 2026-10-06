//! 事件权限的查询与授权引导。
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
//!   有公开查询 API（[`preflight_accessibility`]）。
//!
//! 授权引导统一为「确认对话框后打开对应系统设置面板」，由启动向导
//! （gloss-app 的向导流程）在主线程按序执行；启动期缺失与运行期失效的
//! 引导都汇到这两个入口。

use std::process::Command;

/// 系统设置「输入监控」面板（kTCCServiceListenEvent 的授权入口）。
const INPUT_MONITORING_PANE: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent";
/// 系统设置「辅助功能」面板（kTCCServiceAccessibility 的授权入口）。
const ACCESSIBILITY_PANE: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

/// 当前进程是否已获辅助功能授权（AX 选区读取可用）；纯查询，无副作用。
pub fn preflight_accessibility() -> bool {
    crate::ffi::ax::is_process_trusted()
}

/// 打开系统设置的「输入监控」面板：向导第二步的引导动作。该项无公开预检
/// API，tap 建立失败即视为缺失，把设置面板送到用户面前。只发起不等待
/// （`open` 自身秒退，等它退出没有价值）；失败只降级记日志，不影响主流程。
pub fn open_input_monitoring_pane() -> bool {
    open_privacy_pane(INPUT_MONITORING_PANE, "input monitoring")
}

/// 打开系统设置的「辅助功能」面板：向导第一步的引导动作（该项有公开预检
/// API，缺失与否启动即知）。只发起不等待；失败只降级记日志，不影响主流程。
pub fn open_accessibility_pane() -> bool {
    open_privacy_pane(ACCESSIBILITY_PANE, "accessibility")
}

/// 打开系统设置的「隐私与安全性」子面板；两个授权引导的共用实现。
fn open_privacy_pane(pane: &str, label: &str) -> bool {
    match Command::new("open").arg(pane).spawn() {
        // spawn 的 Child 有意即时 drop：调用点一次性触发，至多一个短命
        // 僵尸表项、有界不累积（open 自身秒退）；多次触发需先补收尸。
        Ok(_) => true,
        Err(err) => {
            // 打不开设置面板是引导路径的降级，不影响功能主流程。
            gloss_core::log::warn!(
                thread = gloss_core::log::thread::UI,
                pane = label,
                error = %err,
                "failed to open the privacy settings pane"
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_guides_target_distinct_panes() {
        assert_ne!(
            INPUT_MONITORING_PANE, ACCESSIBILITY_PANE,
            "the two grants live in different panes; one URL must not stand in for the other"
        );
        assert_eq!(
            INPUT_MONITORING_PANE,
            "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
        );
        assert_eq!(
            ACCESSIBILITY_PANE,
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
        );
    }
}
