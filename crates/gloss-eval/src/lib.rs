//! `gloss-eval`：`Prompt` 评测（确定性重放轨 + `LLM judge` 轨）。
//!
//! 与生产同源：分类校验器（`gloss_core::classify::parse_classify_reply`，
//! 严格裸 `JSON`，内部即 `schema::validate_classify`）、任务轨完成态判定
//! （`gloss_core::schema::validate_task`——生产 `finalize` 消费同一校验器，
//! 两侧共用 `core` 的 `schema` 模块：无本地镜像，也无围栏 `fallback`）、修复
//! 轨迹（`live` 轨消费生产的 `classify_traced` / `run_task_rounds` 循环，
//! 重放轨自夹具轮次逐轮还原）与提示词渲染（`PromptRegistry`）直接
//! 复用，评测数字度量的是生产行为。
//!
//! 隔离红线：本 `crate` 是评测工具链，**不被任何生产 `crate` 依赖**——
//! `check-constraints.sh` 的依赖方向断言强制（`gloss-eval` 只许依赖
//! `gloss-core`，平台侧由评测入口自行组装）；`datasets/`、`fixtures/`、`prompts/`
//! 资产也只允许被本 `crate` 的 `include_str!` 嵌入。
//!
//! 两条轨：
//! - **确定性轨**（`replay`，`CI` 安全）：数据集条目对上 `fixtures/` 里
//!   维护的修复轨迹（每轮增量；正常回复来自录制，异常轨迹——空回复/
//!   散文/缺 `note`/`kind` 越类别清单——是按生产修复形态构造的受控样例），经
//!   生产解析函数逐轮算出准确率 / 混淆矩阵 / 无效 `JSON` 率 / 回退率 /
//!   契约 `JSON` 率 / `note` 在场率 / 字段完整率 / 降级率 / 违约类别计数 /
//!   修复轮次。无网络、无凭据，`cargo test -p gloss-eval` 内置覆盖。
//! - **`live` 轨**（`opt-in`，需 `GLOSS_LIVE_*`）：数据集逐条打真实 `LLM`
//!   （延迟 `p50`/`p95` 只在这一轨有意义），可选 `--record` 把修复轨迹回写
//!   `fixtures/`，可选 `GLOSS_LIVE_JUDGE=1` 启用 `judge` 评分。
//!
//! 两条轨都产出逐条明细（[`detail`]）并渲染单文件检视页（[`crate::inspect`]，
//! `target/eval/inspect.html`）：准确率头条、逐条期望与实际、逐轮原始回复
//! 与内嵌趋势图，供提示词改动后人工检视。

pub mod dataset;
pub mod detail;
pub mod history;
pub mod inspect;
pub mod judge;
pub mod metrics;
pub mod report;
pub mod runner;
pub mod svg;

pub use report::EvalReport;

pub mod assets {
    pub const CLASSIFY_DATASET: &str = include_str!("../datasets/classify.jsonl");
    pub const TASK_WORD_DATASET: &str = include_str!("../datasets/task_translate_word.jsonl");
    pub const TASK_SENTENCE_DATASET: &str =
        include_str!("../datasets/task_translate_sentence.jsonl");
    pub const TASK_CODE_DATASET: &str = include_str!("../datasets/task_explain_code.jsonl");
    pub const CLASSIFY_FIXTURES: &str = include_str!("../fixtures/classify_replay.jsonl");
    pub const TASK_FIXTURES: &str = include_str!("../fixtures/task_replay.jsonl");
}
