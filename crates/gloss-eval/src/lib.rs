//! 评测工具包（gloss-eval）：提示词评测的闭环评测面——精选集加载、
//! 执行编排、三层结算（契约、评审、回归）、流程装配（迭代实跑、标注
//! 落库、基线固化）、运行时资产仓、检视页与趋势渲染、语言门独立评测、
//! 报告渲染与历史趋势。
//!
//! 与生产同源：执行与契约结算经生产提示词、生产修复循环与生产校验器
//! 取数，评测数字度量的才是生产行为；回归判定消费基线与标注资产。
//! 报告与历史行是评测的产物层：报告以（JSON）落盘、以（Markdown）渲
//! 染，历史行追加进历史记录文件并重绘趋势矢量图（SVG）。
//!
//! 隔离红线：本包是评测工具链，“不被任何生产包依赖”——约束检查
//! 脚本的依赖方向断言强制：本包只许依赖核心层，平台侧由评测入口
//! 自行组装；数据集与提示词资产也只允许被本包的编译期嵌入使用。

pub mod asset;
pub mod contract;
pub mod dataset;
pub mod detail;
pub mod execute;
pub mod flow;
pub mod gate;
pub mod history;
pub mod quality;
pub mod regression;
pub mod remedy;
pub mod report;
pub mod trend;
pub mod view;

pub use detail::{CaseDetail, EvalKind, JudgeScores, RuleOutcome, RuleVerdict};
pub use report::EvalReport;

pub mod assets {
    pub const CURATED_DATASET: &str = include_str!("../datasets/curated.jsonl");
    pub const LANG_GATE_DATASET: &str = include_str!("../datasets/lang_gate.jsonl");
    pub const LANG_GATE_REMEDY_DATASET: &str = include_str!("../datasets/lang_gate_remedy.jsonl");
}
