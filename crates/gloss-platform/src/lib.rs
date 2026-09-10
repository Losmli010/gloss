//! gloss-platform：适配器层。
//!
//! 实现 `gloss-core` 定义的端口（trait），并承载平台事件源。
//! 所有平台差异与危险 API（Accessibility / 剪贴板 / 截图 / keychain）隔离在本 crate。
//! 规划模块（docs/06-系统架构设计.md §3.1）：
//! - `selection`：SelectionReader 实现（CompositeReader：AX 优先 + 剪贴板兜底）——M2
//! - `capture`：RegionCapture 实现（区域截图）——M5
//! - `events`：平台事件源（专用线程：热键 / 鼠标监听）——M2
//! - `engine`：AiEngine 实现（统一 LLM 客户端，不按模态拆分）——M4
//! - `storage`：ConfigStore 实现（文件 + keychain）——M4

/// 冒烟标记：验证 platform → core 依赖接通。
/// M1-T1 临时产物，M2 事件源落地后移除。
pub fn platform_smoke_marker() -> u32 {
    gloss_core::core_smoke_marker() + 1
}

#[cfg(test)]
mod tests {
    /// 冒烟测试：验证对 gloss-core 的依赖已接通
    #[test]
    fn platform_smoke() {
        assert_eq!(crate::platform_smoke_marker(), 2);
    }
}
