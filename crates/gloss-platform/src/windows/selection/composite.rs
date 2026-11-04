//! 组合读取器（系统（Windows）占位）：读一次选区文本的桩。
//!
//! 系统实现组合辅助功能读取与剪贴板兜底（含读后还原（settle）语义）；
//! 系统（Windows）的对应实现（用户界面自动化（UIA）主路加按键注入
//! （SendInput）模拟复制兜底）按移植方案后续阶段补齐，本桩读取一律
//! 报告未支持、无还原收尾。

use gloss_core::model::GlossError;

use crate::clipboard::WatchRoundFlag;

pub struct SelectionRead {
    pub outcome: Result<String, GlossError>,
}

impl SelectionRead {
    pub fn into_parts(self) -> (Result<String, GlossError>, SelectionSettle) {
        (self.outcome, SelectionSettle)
    }
}

pub struct SelectionSettle;

impl SelectionSettle {
    pub fn finish(self) {}
}

pub struct CompositeReader;

impl CompositeReader {
    pub fn new(_round: WatchRoundFlag) -> Self {
        Self
    }

    pub fn read(&mut self) -> SelectionRead {
        SelectionRead {
            outcome: Err(GlossError::PlatformUnsupported),
        }
    }
}
