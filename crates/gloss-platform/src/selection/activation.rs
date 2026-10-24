//! 辅助功能（AX）通道激活：对浏览器内核家族（Chromium）宿主按需打开无障碍树。
//!
//! 浏览器内核家族（Chromium）系应用——品牌浏览器（Chrome）与一切桌面壳（Electron）、
//! 内嵌框架（CEF）宿主——默认不构建无障碍
//! 树，只在检测到读屏器（VoiceOver）这类辅助技术时才开树；本应用（Gloss）
//! 这类自建辅助功能客户端不在宿主的检测名单里，辅助功能读选区在这类应用上
//! 恒失败（焦点元素缺属性或无值），每个手势都落到剪贴板兜底——注入按键、
//! 写剪贴板、再恢复原内容，慢且有剪贴板副作用。通行解法是写入宿主识别的
//! 私有开关属性（无障碍开关属性，老内核认增强界面属性），
//! 宿主收到即开始异步建树，此后辅助功能读选区与原生应用无异。本模块负责
//! 识别该家族宿主、注入开关并做会话内去重；注入后的建树是宿主的
//! 异步行为（数百毫秒起），补读节奏由组合通道的激活预算
//! 控制（见组合通道的激活延时）。
//!
//! 识别判据是包内容证据而非应用包标识名单：应用包内框架目录下
//! 存在“品牌前缀加框架”形态的框架包即视为该家族（见宿主识别
//! 判定），免维护名单，浏览器引擎（WebKit/Gecko）宿主天然不命中。
//! 评估结论按进程标识记账（判定账本）：确定性结论（非浏览器内核家族、
//! 注入成功）整个会话只此一次——注入本身幂等，重复注入只会反复打扰
//! 宿主；注入瞬时失败（如消息超时）不记账，下一手势自然重试，不把
//! 宿主锁在激活门外。注入成功的进程另记“唤醒常客”（唤醒常客记账）
//! ——这类宿主的无障碍树会缺席（宿主自行收回或尚未建好），组合通道对
//! 常客的空读跳过让渡直接来唤醒，常客的重注入也放行（幂等、是唯一
//! 自救通道）。

use std::collections::HashSet;

#[derive(Debug, Default)]
struct Ledger {
    settled: HashSet<i32>,
    rescued: HashSet<i32>,
}

impl Ledger {
    fn is_pending(&self, pid: i32) -> bool {
        !self.settled.contains(&pid)
    }

    fn settle(&mut self, pid: i32) {
        self.settled.insert(pid);
    }

    fn mark_rescued(&mut self, pid: i32) {
        self.rescued.insert(pid);
    }

    fn is_rescued(&self, pid: i32) -> bool {
        self.rescued.contains(&pid)
    }
}

fn is_chromium_family(names: impl IntoIterator<Item = impl AsRef<str>>) -> bool {
    names
        .into_iter()
        .any(|name| name.as_ref().ends_with(" Framework.framework"))
}

mod imp {
    use std::path::Path;

    use gloss_core::log::{debug, info, thread, warn};

    use objc2_app_kit::NSRunningApplication;

    use crate::ffi::ax::{self, AXUIElementRef};
    use crate::ffi::cf::{self, CFTypeRef, CfGuard};

    use super::{Ledger, is_chromium_family};

    const AX_ERROR_SUCCESS: i32 = 0;

    #[derive(Debug, Default)]
    pub struct AxActivator {
        ledger: Ledger,
    }

    impl AxActivator {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn responds_to_wake(&self) -> bool {
            front_app().is_some_and(|(_, pid)| self.ledger.is_rescued(pid))
        }

        pub fn wake(&mut self) -> bool {
            let Some((app, pid)) = front_app() else {
                return false;
            };
            if !self.ledger.is_pending(pid) {
                if !self.ledger.is_rescued(pid) {
                    return false;
                }
                return enable_manual_accessibility(app.raw() as AXUIElementRef);
            }
            if !is_chromium_host(pid) {
                debug!(
                    thread = thread::EVENT,
                    pid, "front app is not a chromium host, activation skipped"
                );
                self.ledger.settle(pid);
                return false;
            }
            if enable_manual_accessibility(app.raw() as AXUIElementRef) {
                info!(
                    thread = thread::EVENT,
                    pid, "manual accessibility enabled for the chromium front app"
                );
                self.ledger.settle(pid);
                self.ledger.mark_rescued(pid);
                true
            } else {
                warn!(
                    thread = thread::EVENT,
                    pid,
                    "manual accessibility write failed, activation deferred to the next gesture"
                );
                false
            }
        }
    }

    fn front_app() -> Option<(CfGuard, i32)> {
        // SAFETY: no preconditions; the returned +1 reference may be NULL (a
        // system anomaly) and is null-checked right after.
        let system_wide = unsafe { ax::create_system_wide() };
        if system_wide.is_null() {
            return None;
        }
        let _system_wide = CfGuard::new(system_wide as CFTypeRef);
        let attribute = cf::cf_string(ax::FOCUSED_APPLICATION_ATTRIBUTE)?;
        let mut app: CFTypeRef = std::ptr::null();
        // SAFETY: both the element and the attribute name are valid +1
        // references, and the out parameter points to the stack variable just
        // declared.
        let err = unsafe { ax::copy_attribute(system_wide, attribute.string_ref(), &mut app) };
        if err != AX_ERROR_SUCCESS {
            return None;
        }
        let app = CfGuard::new(app);
        let mut pid: i32 = 0;
        // SAFETY: `app` is the application element reference successfully copied
        // in the previous step, and the out parameter points to a stack variable.
        let err = unsafe { ax::pid_of(app.raw() as AXUIElementRef, &mut pid) };
        if err != AX_ERROR_SUCCESS {
            return None;
        }
        Some((app, pid))
    }

    fn is_chromium_host(pid: i32) -> bool {
        let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) else {
            return false;
        };
        let Some(url) = app.bundleURL() else {
            return false;
        };
        let Some(path) = url.path() else {
            return false;
        };
        let frameworks = Path::new(path.to_string().as_str()).join("Contents/Frameworks");
        let Ok(entries) = std::fs::read_dir(frameworks) else {
            return false;
        };
        let names: Vec<String> = entries
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        is_chromium_family(&names)
    }

    fn enable_manual_accessibility(element: AXUIElementRef) -> bool {
        let attributes = [
            ax::MANUAL_ACCESSIBILITY_ATTRIBUTE,
            ax::ENHANCED_USER_INTERFACE_ATTRIBUTE,
        ];
        let mut enabled = false;
        for attribute in attributes {
            let Some(name) = cf::cf_string(attribute) else {
                continue;
            };
            // SAFETY: `element` is a valid application element reference just
            // copied from the system, and `name` is a valid CFString just
            // constructed; the write result is returned as an `AXError`.
            let err = unsafe { ax::set_bool_attribute(element, name.string_ref()) };
            if err == AX_ERROR_SUCCESS {
                enabled = true;
            } else {
                debug!(
                    thread = thread::EVENT,
                    code = err,
                    "manual accessibility attribute write failed"
                );
            }
        }
        enabled
    }
}

pub use imp::AxActivator;

#[cfg(test)]
mod tests {
    use super::{Ledger, is_chromium_family};

    #[test]
    fn chromium_family_frameworks_are_recognized() {
        for name in [
            "Electron Framework.framework",
            "Google Chrome Framework.framework",
            "Brave Browser Framework.framework",
            "Chromium Embedded Framework.framework",
        ] {
            assert!(
                is_chromium_family([name]),
                "{name} must be recognized as a chromium host"
            );
        }
    }

    #[test]
    fn unrelated_frameworks_are_not_chromium() {
        for names in [
            Vec::<&str>::new(),
            vec![
                "Mantle.framework",
                "Squirrel.framework",
                "ReactiveObjC.framework",
            ],
            vec!["Electron Framework"],
            vec!["Framework.framework"],
        ] {
            assert!(
                !is_chromium_family(&names),
                "{names:?} must not be mistaken for a chromium host"
            );
        }
    }

    #[test]
    fn the_ledger_settles_definitive_verdicts_exactly_once() {
        let mut ledger = Ledger::default();
        assert!(ledger.is_pending(42));
        ledger.settle(42);
        assert!(
            !ledger.is_pending(42),
            "a settled pid must not be assessed again this session"
        );
    }

    #[test]
    fn an_unsettled_pid_stays_pending_for_the_next_gesture() {
        let ledger = Ledger::default();
        assert!(
            ledger.is_pending(42) && ledger.is_pending(42),
            "transient failures leave the pid pending, every gesture may retry"
        );
    }

    #[test]
    fn a_rescued_pid_is_marked_for_rewake() {
        let mut ledger = Ledger::default();
        assert!(!ledger.is_rescued(42), "a fresh pid is no regular");
        ledger.settle(42);
        ledger.mark_rescued(42);
        assert!(
            !ledger.is_pending(42) && ledger.is_rescued(42),
            "an injected pid becomes a wake regular: settled for assessment, rewake allowed"
        );
        assert!(
            !ledger.is_rescued(43),
            "the mark is per pid, other hosts are unaffected"
        );
    }
}
