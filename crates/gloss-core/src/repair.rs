//! 契约修复轮的共享骨架：任务段与分类段只在“产物形态与兜底”上不同——
//! 预算判定、违约留痕、纠错指令渲染与对话重拼共用一份。
//!
//! 两段此前各写一份循环体，语义已各自漂移过一次（任务段首轮渲染底座、
//! 分类段每轮重渲）。共用后差异只剩调用方传入的预算、底座与指令渲染。

use crate::log::{thread, warn};
use crate::prompt::{ChatMessage, repair_conversation};
use crate::schema::Violation;

#[derive(Debug, Clone, Copy)]
pub(crate) enum RepairStage {
    Task,
    Classify,
}

impl RepairStage {
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

pub(crate) struct RepairLoop {
    stage: RepairStage,
    spent: u32,
    cap: u32,
}

impl RepairLoop {
    pub(crate) fn new(stage: RepairStage, cap: u32) -> Self {
        Self {
            stage,
            spent: 0,
            cap,
        }
    }

    pub(crate) fn spent(&self) -> u32 {
        self.spent
    }

    pub(crate) fn next_round(
        &mut self,
        base: &[ChatMessage],
        reply: &str,
        violation: Violation,
        instruction: impl FnOnce(Violation) -> String,
    ) -> Option<Vec<ChatMessage>> {
        if self.spent >= self.cap {
            self.stage.warn_exhausted(violation);
            return None;
        }
        self.spent += 1;
        self.stage.warn_violated(self.spent, violation);
        match violation {
            Violation::EmptyReply => Some(base.to_vec()),
            _ => Some(repair_conversation(base, reply, instruction(violation))),
        }
    }
}
