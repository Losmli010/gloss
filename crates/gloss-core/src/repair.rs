//! 契约修复轮的共享骨架：任务段与分类段只在「产物形态与兜底」上不同——
//! 预算判定、违约留痕、纠错指令渲染与对话重拼共用一份。
//!
//! 两段此前各写一份循环体，语义已各自漂移过一次（任务段首轮渲染底座、
//! 分类段每轮重渲）。共用后差异只剩调用方传入的预算、底座与指令渲染。

use crate::log::{thread, warn};
use crate::prompt::{ChatMessage, repair_conversation};
use crate::schema::Violation;

/// 修复轮所属的编排段：只决定留痕的固定措辞（日志一律英文）。
#[derive(Debug, Clone, Copy)]
pub(crate) enum RepairStage {
    /// 任务段：耗尽后把原始回复交给完成态解析（降级产物）。
    Task,
    /// 分类段：耗尽后落常量兜底 kind。
    Classify,
}

impl RepairStage {
    /// 预算耗尽的留痕：调用方随即按各自语义上交产物。
    fn warn_exhausted(self, violation: Violation) {
        match self {
            Self::Task => warn!(
                thread = thread::TOKIO,
                reason = %violation,
                "task repairs exhausted, handing the raw reply to the completion parser"
            ),
            Self::Classify => warn!(
                thread = thread::TOKIO,
                reason = %violation,
                "classify repairs exhausted, falling back to the default kind"
            ),
        }
    }

    /// 一次违约的留痕：只记类别与轮次，无选区与回复原文（安全红线）。
    fn warn_violated(self, attempt: u32, violation: Violation) {
        match self {
            Self::Task => warn!(
                thread = thread::TOKIO,
                attempt = attempt,
                reason = %violation,
                "task reply violated the contract, preparing a repair round"
            ),
            Self::Classify => warn!(
                thread = thread::TOKIO,
                attempt = attempt,
                reason = %violation,
                "classify reply violated the contract, preparing a repair round"
            ),
        }
    }
}

/// 违约后的下一轮对话：预算内留痕并返回修复对话；耗尽返回 `None`，由
/// 调用方按本段语义上交产物。EmptyReply 无内容可纠：原样重发底座，不拼
/// assistant 轮也不渲染纠错指令（部分端点拒收空 assistant content，而
/// 首轮请求本身已含完整契约）——`instruction` 只在真正要发修复轮时求值。
pub(crate) fn next_round(
    stage: RepairStage,
    base: &[ChatMessage],
    reply: &str,
    violation: Violation,
    repairs: &mut u32,
    budget: u32,
    instruction: impl FnOnce(Violation) -> String,
) -> Option<Vec<ChatMessage>> {
    if *repairs >= budget {
        stage.warn_exhausted(violation);
        return None;
    }
    *repairs += 1;
    stage.warn_violated(*repairs, violation);
    match violation {
        Violation::EmptyReply => Some(base.to_vec()),
        _ => Some(repair_conversation(base, reply, instruction(violation))),
    }
}
