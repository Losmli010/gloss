//! 触发前场景探针，即场景探针端口的系统（Windows）实现：前台应用加安全输入态。
//!
//! 前台应用取自前台窗口的进程映像名：套标识字段（bundle_id）填可执行文件
//! 名（含扩展名），名称字段（name）填映像名去扩展名的进程名——与系统
//! （macOS）的套标识语义不同
//! （移植方案写明的语义漂移，敏感应用名单两平台各持一份，匹配对象见防护
//! 模块）。安全输入态恒为关：系统（Windows）没有可查询的安全输入开关，
//! 密码框防护由敏感应用名单承担；安全桌面（UAC）下前台窗口查询失败返回
//! 空事实，闸门按“无事实”放行（探针读不到不该升级成拦截）。两者都是纯
//! 查询：不索取权限、不改状态、读不到选区内容。

use std::process;

use gloss_core::guard::{FrontApp, SceneFacts};
use gloss_core::ports::SceneProbe;

use crate::ffi::foreground;

#[derive(Debug, Default)]
pub struct SystemSceneProbe;

impl SceneProbe for SystemSceneProbe {
    fn facts(&self) -> SceneFacts {
        SceneFacts {
            secure_input: false,
            front_app: frontmost_app(),
        }
    }
}

fn frontmost_app() -> Option<FrontApp> {
    let (image_path, pid) = foreground::foreground_image_name()?;
    let (file, stem) = image_name(&image_path)?;
    let identity = FrontApp {
        bundle_id: Some(file.to_owned()),
        name: Some(stem.to_owned()),
        is_self: pid == process::id(),
    };
    Some(identity)
}

fn image_name(path: &str) -> Option<(&str, &str)> {
    let file = path.rsplit(['\\', '/']).next()?;
    if file.is_empty() {
        return None;
    }
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    Some((file, stem))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_name_survives_full_path_extraction() {
        assert_eq!(
            image_name(r"C:\Apps\1Password\1password.exe"),
            Some(("1password.exe", "1password"))
        );
        assert_eq!(
            image_name("C:/Apps/KeePass/KeePass.exe"),
            Some(("KeePass.exe", "KeePass"))
        );
        assert_eq!(image_name("bare.exe"), Some(("bare.exe", "bare")));
        assert_eq!(
            image_name(r"C:\dir\multi.dot.name.exe"),
            Some(("multi.dot.name.exe", "multi.dot.name"))
        );
        assert_eq!(
            image_name(r"C:\"),
            None,
            "a bare directory path names no app"
        );
    }

    #[test]
    fn front_app_probe_reports_a_usable_identity() {
        let facts = SystemSceneProbe.facts();
        assert!(
            !facts.secure_input,
            "the secure-input flag has no Windows query and stays off"
        );
        if let Some(app) = facts.front_app {
            assert!(
                app.bundle_id
                    .as_deref()
                    .is_some_and(|value| !value.is_empty())
                    || app.name.as_deref().is_some_and(|value| !value.is_empty()),
                "a reported front app must carry at least one usable identity"
            );
        }
    }
}
