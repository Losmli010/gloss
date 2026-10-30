//! 契约修复轮的共享骨架：任务段、分类段与语言段只在“产物形态与兜底”上
//! 不同——预算判定、违约留痕、纠错指令渲染与对话重拼共用一份。
//!
//! 任务与分类两段此前各写一份循环体，语义已各自漂移过一次（任务段首轮
//! 渲染底座、分类段每轮重渲）。共用后差异只剩调用方传入的预算、底座、
//! 违约原因与指令渲染；语言段复用同一骨架，预算独立（不挤占任务段）。

use crate::log::{thread, warn};
use crate::prompt::{ChatMessage, repair_conversation};

#[derive(Debug, Clone, Copy)]
pub(crate) enum RepairStage {
    Task,
    Classify,
    Lang,
}

impl RepairStage {
    fn warn_exhausted(self, reason: impl std::fmt::Display) {
        match self {
            Self::Task => warn!(
                thread = thread::TOKIO,
                reason = %reason,
                "task repairs exhausted, handing the raw reply to the completion parser"
            ),
            Self::Classify => warn!(
                thread = thread::TOKIO,
                reason = %reason,
                "classify repairs exhausted, falling back to the default kind"
            ),
            Self::Lang => warn!(
                thread = thread::TOKIO,
                reason = %reason,
                "language repairs exhausted, delivering the reply degraded"
            ),
        }
    }

    fn warn_violated(self, attempt: u32, reason: impl std::fmt::Display) {
        match self {
            Self::Task => warn!(
                thread = thread::TOKIO,
                attempt = attempt,
                reason = %reason,
                "task reply violated the contract, preparing a repair round"
            ),
            Self::Classify => warn!(
                thread = thread::TOKIO,
                attempt = attempt,
                reason = %reason,
                "classify reply violated the contract, preparing a repair round"
            ),
            Self::Lang => warn!(
                thread = thread::TOKIO,
                attempt = attempt,
                reason = %reason,
                "reply violates the output language rules, preparing a repair round"
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
        reply: Option<&str>,
        reason: impl std::fmt::Display,
        instruction: impl FnOnce() -> String,
    ) -> Option<Vec<ChatMessage>> {
        if self.spent >= self.cap {
            self.stage.warn_exhausted(reason);
            return None;
        }
        self.spent += 1;
        self.stage.warn_violated(self.spent, reason);
        Some(match reply {
            None => base.to_vec(),
            Some(reply) => repair_conversation(base, reply, instruction()),
        })
    }
}
