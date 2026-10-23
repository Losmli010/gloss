//! 触发前场景探针（`SceneProbe` 的 `macOS` 实现）：安全输入态 + 前台应用。
//!
//! 两者都是纯查询：不索取权限、不改状态、读不到选区内容——探针只回答
//! 「现在该不该触发」，不碰用户数据。前台应用取不到（无图形会话、
//! `LaunchServices` 未就绪）时返回空事实，闸门按「无事实」放行。

use gloss_core::guard::{FrontApp, SceneFacts};
use gloss_core::ports::SceneProbe;
use objc2_app_kit::NSWorkspace;

#[derive(Debug, Default)]
pub struct SystemSceneProbe;

impl SceneProbe for SystemSceneProbe {
    fn facts(&self) -> SceneFacts {
        SceneFacts {
            secure_input: crate::ffi::carbon::is_secure_event_input_enabled(),
            front_app: frontmost_app(),
        }
    }
}

fn frontmost_app() -> Option<FrontApp> {
    let workspace = NSWorkspace::sharedWorkspace();
    let app = workspace.frontmostApplication()?;
    let identity = FrontApp {
        bundle_id: app.bundleIdentifier().map(|value| value.to_string()),
        name: app.localizedName().map(|value| value.to_string()),
        is_self: app.processIdentifier() == std::process::id() as i32,
    };
    (identity.bundle_id.is_some() || identity.name.is_some()).then_some(identity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_probe_answers_with_a_coherent_snapshot() {
        let facts = SystemSceneProbe.facts();
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

#[cfg(test)]
mod live_tests {
    use gloss_core::ports::SceneProbe;

    use super::SystemSceneProbe;

    #[test]
    #[ignore = "需图形会话：没有前台应用的环境（无窗口服务）会失败"]
    fn scene_probe_reports_the_frontmost_app() {
        let facts = SystemSceneProbe.facts();
        let app = facts
            .front_app
            .expect("a session with a frontmost application must report one");
        assert!(
            app.bundle_id
                .as_deref()
                .is_some_and(|value| !value.is_empty())
                || app.name.as_deref().is_some_and(|value| !value.is_empty()),
            "the frontmost app must carry at least one usable identity"
        );
    }
}
