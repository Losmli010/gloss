//! gloss-eval：Prompt 评测（确定性重放轨 + LLM judge 轨）。
//!
//! 与生产同源：分类校验器（`gloss_core::classify::parse_classify_reply`）、
//! 结构化解析（生产语义见 gloss-app 的 finalize：JSON 主路径 + 围栏
//! fallback；本 crate 按依赖方向约束以本地镜像对齐同一规则）
//! 与 prompt 渲染（`PromptRegistry`）直接复用，评测数字度量的是生产行为。
//!
//! 隔离红线：本 crate 是评测工具链，**不被任何生产 crate 依赖**——
//! `check-constraints.sh` 的依赖方向断言强制（gloss-eval 只许依赖
//! gloss-core 与 gloss-platform）；`datasets/`、`fixtures/`、`prompts/`
//! 资产也只允许被本 crate 的 `include_str!` 嵌入。
//!
//! 两条轨：
//! - **确定性轨**（`replay`，CI 安全）：数据集条目对上 `fixtures/` 里
//!   录制的真实 SSE 增量，经生产解析函数算出 accuracy / 混淆矩阵 /
//!   无效 JSON 率 / 回退率 / 契约 JSON 率 / note 在场率 / 字段完整率 /
//!   降级率。无网络、无凭据，`cargo test -p gloss-eval` 内置覆盖。
//! - **live 轨**（opt-in，需 `GLOSS_LIVE_*`）：数据集逐条打真实 LLM
//!   （延迟 p50/p95 只在这一轨有意义），可选 `--record` 把增量回写
//!   `fixtures/`，可选 `GLOSS_LIVE_JUDGE=1` 启用 judge 评分。

pub mod dataset;
pub mod judge;
pub mod metrics;
pub mod report;
pub mod runner;

pub use report::EvalReport;

/// 数据集与夹具资产：编译期嵌入，`replay` 模式与测试共用同一份。
pub mod assets {
    /// 分类数据集（≥60 条：中英/代码/长短句/易混边界）。
    pub const CLASSIFY_DATASET: &str = include_str!("../datasets/classify.jsonl");
    /// 词卡任务数据集（带 reference 结构化字段）。
    pub const TASK_WORD_DATASET: &str = include_str!("../datasets/task_translate_word.jsonl");
    /// 句译任务数据集。
    pub const TASK_SENTENCE_DATASET: &str =
        include_str!("../datasets/task_translate_sentence.jsonl");
    /// 代码解释任务数据集。
    pub const TASK_CODE_DATASET: &str = include_str!("../datasets/task_explain_code.jsonl");
    /// 分类重放夹具（录制/维护的真实模型回复增量）。
    pub const CLASSIFY_FIXTURES: &str = include_str!("../fixtures/classify_replay.jsonl");
    /// 任务重放夹具。
    pub const TASK_FIXTURES: &str = include_str!("../fixtures/task_replay.jsonl");
}
