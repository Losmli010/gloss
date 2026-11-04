//! 触发前场景探针（系统（Windows）占位），即场景探针端口的桩：恒报无事实。
//!
//! 前台应用取自前台窗口的进程映像名、安全输入态恒为关（安全桌面（UAC）
//! 下前台窗口查询会失败，据此拒绝），按移植方案后续阶段补齐；本桩返回
//! 空事实——闸门按“无事实”放行，敏感应用名单的系统（Windows）形态随
//! 后续阶段落地。

use gloss_core::guard::SceneFacts;
use gloss_core::ports::SceneProbe;

#[derive(Debug, Default)]
pub struct SystemSceneProbe;

impl SceneProbe for SystemSceneProbe {
    fn facts(&self) -> SceneFacts {
        SceneFacts::default()
    }
}
