//! 评测工具包（gloss-eval）：提示词评测的离线评测面——语言门独立
//! 评测、报告渲染与历史趋势。
//!
//! 与生产同源：语言门评测经生产判定入口逐条判定，评测数字度量的
//! 是生产行为；动作收敛轨复用同一判定驱动。报告与历史行是评测的
//! 产物层：报告以（JSON）落盘、以（Markdown）渲染，历史行追加进
//! 历史记录文件并重绘趋势矢量图（SVG）。
//!
//! 隔离红线：本包是评测工具链，“不被任何生产包依赖”——约束检查
//! 脚本的依赖方向断言强制：本包只许依赖核心层，平台侧由评测入口
//! 自行组装；数据集与提示词资产也只允许被本包的编译期嵌入使用。

pub mod contract;
pub mod detail;
pub mod gate;
pub mod history;
pub mod metrics;
pub mod quality;
pub mod regression;
pub mod remedy;
pub mod report;
pub mod svg;

pub use detail::{CaseDetail, EvalKind, JudgeScores, RuleOutcome, RuleVerdict};
pub use report::EvalReport;

pub mod assets {
    pub const LANG_GATE_DATASET: &str = include_str!("../datasets/lang_gate.jsonl");
    pub const LANG_GATE_REMEDY_DATASET: &str = include_str!("../datasets/lang_gate_remedy.jsonl");
}
