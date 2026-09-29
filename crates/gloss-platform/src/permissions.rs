//! 事件权限的预检与授权引导。
//!
//! 划词链路依赖两项**互相独立**的系统授权，且系统没有把二者放在同一个
//! 开关下，缺失任一都会让划词在对应环节失效：
//!
//! - **输入监控**（kTCCServiceListenEvent）：CGEventTap 的建立前提，缺失
//!   时 tap 创建失败、手势整体降级（见 `events::mouse` 的降级标志）。
//!   此项**没有公开的预检 API**，状态只能从 tap 成败推断，引导方式是
//!   打开系统设置的「输入监控」面板（[`open_input_monitoring_pane`]）。
//! - **辅助功能**（kTCCServiceAccessibility）：AX 选区读取的前提，缺失时
//!   选区读取返回 [`GlossError::AccessibilityDenied`]。有公开查询 API，
//!   且支持让系统直接弹授权引导对话框（[`request_accessibility`]）。
//!
//! 启动时预检辅助功能并触发引导、tap 降级时打开输入监控面板，把「划词
//! 失败后摸着失败卡找原因」变成「启动即指路」。

use std::process::Command;

use gloss_core::model::GlossError;

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

/// 打开系统设置的「输入监控」面板；用于 CGEventTap 创建失败的引导（该
/// 授权无公开预检 API，tap 失败即视为缺失）。只发起不等待：调用方可能
/// 在延迟关键的事件线程上（见 main.rs 的降级提示），阻塞等 `open` 退出
/// 没有价值。启动失败只记日志不拦启动。
pub fn open_input_monitoring_pane() -> bool {
    let pane = "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent";
    match Command::new("open").arg(pane).spawn() {
        // spawn 的 Child 有意即时 drop：调用点有一次性标志，至多一个
        // 短命僵尸表项、有界不累积（open 自身秒退）；多次触发需先补收尸。
        Ok(_) => true,
        Err(_) => {
            // 打不开设置面板是引导路径的降级，不影响功能主流程。
            gloss_core::log::warn!(
                thread = gloss_core::log::thread::EVENT,
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
