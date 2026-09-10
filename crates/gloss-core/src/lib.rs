//! gloss-core：领域层 + 端口（Ports）。
//!
//! 纯逻辑 crate，零平台/UI 依赖，位于依赖图最底层。
//! 规划模块（docs/06-系统架构设计.md §3.1）：
//! - `model`：通用模型（语言/坐标/错误）——M3-T1
//! - `task`：任务模型（TaskKind / TaskInput / Task / TaskOutcome）——M3-T2
//! - `ports`：全部端口 trait（SelectionReader / RegionCapture / AiEngine / ConfigStore / Cache）——M3-T3
//! - `prompt`：Prompt 模板注册表——M3-T4
//! - `cache`：Cache 端口内存实现（moka）——M3-T5
//! - `engine`：AiTaskService 任务编排——M3-T6
//! - `config`：配置模型 + ConfigStore 端口——M4-T1
//! - `pipeline`：触发→取材→任务化→执行→出卡 编排管道——M3

/// 冒烟标记：验证 workspace 依赖方向接通。
/// M1-T1 临时产物，M3 真实模块落地后移除。
pub fn core_smoke_marker() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    /// 冒烟测试：保证 workspace 测试管线对每个 crate 都有覆盖
    #[test]
    fn core_smoke() {
        assert_eq!(2 + 2, 4);
    }
}
