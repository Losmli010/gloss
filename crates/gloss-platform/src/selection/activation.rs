//! AX 通道激活：对 Chromium/Electron 宿主按需打开无障碍树。
//!
//! Chromium 系应用（Chrome 与一切 Electron/CEF 宿主）默认不构建无障碍
//! 树，只在检测到辅助技术（VoiceOver 等）时才开树；Gloss 这类自建 AX
//! 客户端不在宿主的检测名单里，AX 读选区在这类应用上恒失败（焦点元素
//! 缺属性或无值），每个手势都落到剪贴板兜底——注入按键、写剪贴板、再
//! 恢复原内容，慢且有剪贴板副作用。通行解法是写入宿主识别的私有开关
//! 属性（`AXManualAccessibility`，老内核认 `AXEnhancedUserInterface`），
//! 宿主收到即开始异步建树，此后 AX 读选区与原生应用无异。本模块负责
//! 识别 Chromium 宿主、注入开关并做会话内去重；注入后的建树是宿主的
//! 异步行为（数百毫秒起），补读节奏由组合通道的激活预算控制（见
//! `composite::activation_delay`）。
//!
//! 识别判据是包内容证据而非 bundle id 名单：`Contents/Frameworks` 下
//! 存在「品牌前缀 + Framework」形态的框架包即视为 Chromium 家族（见
//! [`is_chromium_family`]），免维护名单，WebKit/Gecko 宿主天然不命中。
//! 评估结果按 pid 记忆，整个会话对同一宿主至多注入一次——注入本身
//! 幂等，但重复注入只会反复打扰宿主，不会让建树更快。

use std::collections::HashSet;

/// 会话内评估登记（纯逻辑，单测覆盖）：pid 首次出现返回 `true` 并登记，
/// 之后对该 pid 一律 `false`——对同一宿主的评估与注入整个会话只此一次。
fn first_assessment(seen: &mut HashSet<i32>, pid: i32) -> bool {
    seen.insert(pid)
}

/// Chromium/Electron 家族检测（纯逻辑，单测覆盖）：框架目录条目里存在
/// 名字以「空格 + Framework.framework」结尾的框架包。Chromium 内核框架
/// 按宿主品牌重命名（Chrome 是 `Google Chrome Framework.framework`、
/// Brave 是 `Brave Browser Framework.framework`），Electron 系固定
/// `Electron Framework.framework`，CEF 固定 `Chromium Embedded
/// Framework.framework`，共同形态即「品牌前缀 + 空格 + Framework」。
/// 误报（第三方框架恰好同名）的代价仅是一次无害的属性写入——非
/// Chromium 宿主对未知 AX 属性返回属性不支持错误并忽略；WebKit/Gecko
/// 宿主没有这一形态，不会被误伤。
fn is_chromium_family(names: impl IntoIterator<Item = impl AsRef<str>>) -> bool {
    names
        .into_iter()
        .any(|name| name.as_ref().ends_with(" Framework.framework"))
}

mod imp {
    use std::collections::HashSet;
    use std::path::Path;

    use gloss_core::log::{debug, info, thread, warn};

    use objc2_app_kit::NSRunningApplication;

    use crate::ffi::ax::{self, AXUIElementRef};
    use crate::ffi::cf::{self, CFTypeRef, CfGuard};

    use super::{first_assessment, is_chromium_family};

    /// AXError.h 的 kAXErrorSuccess。
    const AX_ERROR_SUCCESS: i32 = 0;

    /// AX 通道激活器：会话内按 pid 去重的 Chromium 宿主唤醒。只在平台
    /// 事件线程上使用（由组合读取器持有），无锁不共享。
    #[derive(Debug, Default)]
    pub struct AxActivator {
        assessed: HashSet<i32>,
    }

    impl AxActivator {
        /// 创建激活器。
        pub fn new() -> Self {
            Self::default()
        }

        /// 唤醒前台应用的 AX 通道（评估与注入各只此一次）。
        ///
        /// 返回 `true` 表示刚对 Chromium 宿主成功注入开关——宿主开始异步
        /// 建树，调用方应以激活预算补读 AX；`false` 表示无需补读（该
        /// pid 已评估过、不是 Chromium 宿主、前台应用不可识别或注入
        /// 失败）。调用方保证：在平台事件线程上调用。
        pub fn wake(&mut self) -> bool {
            let Some((app, pid)) = front_app() else {
                return false;
            };
            if !first_assessment(&mut self.assessed, pid) {
                return false;
            }
            if !is_chromium_host(pid) {
                debug!(
                    thread = thread::EVENT,
                    pid, "front app is not a chromium host, activation skipped"
                );
                return false;
            }
            if enable_manual_accessibility(app.raw() as AXUIElementRef) {
                info!(
                    thread = thread::EVENT,
                    pid, "manual accessibility enabled for the chromium front app"
                );
                true
            } else {
                warn!(
                    thread = thread::EVENT,
                    pid, "manual accessibility write failed, activation declined"
                );
                false
            }
        }
    }

    /// 问系统要前台应用元素与其 pid：`AXFocusedApplication` 由系统 AX
    /// 运行时供给，宿主建不建树都能拿到（这正是唤醒路径可用的前提）。
    /// 任一步失败返回 `None`（调用方跳过本轮激活）。消息超时沿用读取
    /// 路径对 systemwide 设置的全局 1s 上界，不会无限阻塞事件线程。
    fn front_app() -> Option<(CfGuard, i32)> {
        // SAFETY: 无前置条件；返回的 +1 引用可能为 NULL（系统异常），随后判空。
        let system_wide = unsafe { ax::create_system_wide() };
        if system_wide.is_null() {
            return None;
        }
        let _system_wide = CfGuard::new(system_wide as CFTypeRef);
        let attribute = cf::cf_string(ax::FOCUSED_APPLICATION_ATTRIBUTE)?;
        let mut app: CFTypeRef = std::ptr::null();
        // SAFETY: 元素与属性名均为有效 +1 引用，出参指向刚声明的栈上变量。
        let err = unsafe { ax::copy_attribute(system_wide, attribute.string_ref(), &mut app) };
        if err != AX_ERROR_SUCCESS {
            return None;
        }
        let app = CfGuard::new(app);
        let mut pid: i32 = 0;
        // SAFETY: `app` 是上一步成功拷贝的应用元素引用，出参指向栈上变量。
        let err = unsafe { ax::pid_of(app.raw() as AXUIElementRef, &mut pid) };
        if err != AX_ERROR_SUCCESS {
            return None;
        }
        Some((app, pid))
    }

    /// 按 pid 找运行中的应用并取其包路径，查内核框架形态（判定见
    /// [`is_chromium_family`]）。应用无包（命令行进程等）或框架目录读
    /// 不出，都按「不是 Chromium 宿主」收口。
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

    /// 对宿主应用元素写入两代开关属性：现行内核认
    /// `AXManualAccessibility`，老内核认 `AXEnhancedUserInterface`，一起
    /// 写入覆盖两代；任一写入成功即视为注入生效。逐属性的写入失败只
    /// 记 debug（唤醒失败另有 warn 收口）。
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
            // SAFETY: `element` 是刚从系统拷贝的有效应用元素引用，`name`
            // 是刚构造的有效 CFString；写入结果按 AXError 返回。
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
    use std::collections::HashSet;

    use super::{first_assessment, is_chromium_family};

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
    fn each_pid_is_assessed_exactly_once() {
        let mut seen = HashSet::new();
        assert!(first_assessment(&mut seen, 42));
        assert!(!first_assessment(&mut seen, 42));
        assert!(first_assessment(&mut seen, 43));
    }
}
