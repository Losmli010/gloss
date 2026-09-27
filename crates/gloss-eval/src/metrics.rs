//! 评分器：把「回复原文 + 期望」折算成确定性指标。
//!
//! 全部走生产解析函数（[`gloss_core::classify::parse_classify_reply`] /
//! [`gloss_core::engine::finalize_outcome`]），评测数字度量的是生产行为
//! 而不是评测自己的另一套解析。本模块是纯函数集，runner 与单测共用。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use gloss_core::engine::finalize_outcome;
use gloss_core::prompt::STRUCTURED_FENCE;
use gloss_core::task::{OutcomeStructured, TaskKind};

use crate::dataset::{TaskCase, required_fields};

/// 分类判定的结果归类（比 Ok/Err 细一档：指标要区分「回复不是 JSON」
/// 与「JSON 合法但被生产校验器拒绝」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassifyVerdict {
    /// 判定正确。
    Correct,
    /// 判定成了别的（合法 kind，但在允许清单内且 ≠ 期望）。
    Wrong(TaskKind),
    /// 回复不是合法 JSON（含围栏提取失败）。
    InvalidJson,
    /// JSON 合法但被生产校验器拒绝（未知 kind、清单外 kind）。
    Rejected,
}

/// 分类指标：accuracy / 混淆矩阵 / 无效 JSON 率 / 回退率。
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassifyMetrics {
    /// 进入统计的条数（无夹具的条目跳过不计）。
    pub evaluated: usize,
    /// 判定正确数。
    pub correct: usize,
    /// 混淆矩阵：期望 kind → 实际 kind → 次数（判定错误路径）。
    pub confusion: BTreeMap<TaskKind, BTreeMap<TaskKind, usize>>,
    /// 回复不是合法 JSON 的次数。
    pub invalid_json: usize,
    /// JSON 合法但被生产校验器拒绝的次数。
    pub rejected: usize,
}

impl ClassifyMetrics {
    /// 判定正确率（分母 = 进入统计的条数）。
    pub fn accuracy(&self) -> f64 {
        ratio(self.correct, self.evaluated)
    }

    /// 回退率：生产编排会对这些条目落兜底 kind（无效 JSON + 被拒绝）。
    pub fn fallback_rate(&self) -> f64 {
        ratio(self.invalid_json + self.rejected, self.evaluated)
    }
}

/// 任务回复的契约达成情况（比「解析成功与否」细：四个指标各自独立）。
#[derive(Debug, Clone, PartialEq)]
pub struct TaskVerdict {
    /// 回复里出现了结构化围栏标记。
    pub fence_present: bool,
    /// 围栏内的 JSON 可解析（围栏在场才可能为真）。
    pub json_parseable: bool,
    /// 契约必需字段全部在场（JSON 可解析才可能为真）。
    pub fields_complete: bool,
    /// 生产解析结果（与 UI 收到的产物同源）。
    pub outcome: OutcomeStructured,
}

impl TaskVerdict {
    /// 按 [`TaskCase`] 的 kind 与回复原文逐级判定。
    pub fn for_reply(case: &TaskCase, reply: &str) -> Self {
        let fence_present = reply.contains(STRUCTURED_FENCE);
        let parsed = fenced_json(reply)
            .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok());
        let json_parseable = parsed.is_some();
        let fields_complete = parsed.as_ref().is_some_and(|value| {
            required_fields(case.kind)
                .iter()
                .all(|key| value.get(*key).is_some())
        });
        let outcome = finalize_outcome(case.kind, reply).structured;
        Self {
            fence_present,
            json_parseable,
            fields_complete,
            outcome,
        }
    }

    /// 降级判定：围栏缺失 / JSON 不可解析 / 必需字段不全，三者任一即视为
    /// 降级（生产侧会以无结构化的 Plain/全文兜底呈现）。
    pub fn degraded(&self) -> bool {
        !(self.fence_present && self.json_parseable && self.fields_complete)
    }
}

/// 任务指标：围栏在场率 / JSON 可解析率 / 字段完整率 / 降 Plain 率。
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TaskMetrics {
    /// 进入统计的条数。
    pub evaluated: usize,
    /// 围栏在场数。
    pub fence_present: usize,
    /// 围栏 JSON 可解析数。
    pub json_parseable: usize,
    /// 必需字段完整数。
    pub fields_complete: usize,
    /// 降级数（见 [`TaskVerdict::degraded`]）。
    pub degraded: usize,
}

impl TaskMetrics {
    /// 记入一个判定。
    pub fn record(&mut self, verdict: &TaskVerdict) {
        self.evaluated += 1;
        self.fence_present += usize::from(verdict.fence_present);
        self.json_parseable += usize::from(verdict.json_parseable);
        self.fields_complete += usize::from(verdict.fields_complete);
        self.degraded += usize::from(verdict.degraded());
    }

    /// 围栏在场率。
    pub fn fence_rate(&self) -> f64 {
        ratio(self.fence_present, self.evaluated)
    }

    /// JSON 可解析率。
    pub fn parse_rate(&self) -> f64 {
        ratio(self.json_parseable, self.evaluated)
    }

    /// 字段完整率。
    pub fn completeness_rate(&self) -> f64 {
        ratio(self.fields_complete, self.evaluated)
    }

    /// 降级率（模型没按结构化契约输出的比例）。
    pub fn degraded_rate(&self) -> f64 {
        ratio(self.degraded, self.evaluated)
    }
}

/// 延迟样本（毫秒）：live 轨的逐条耗时，报告取 p50/p95。
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Latency {
    samples_ms: Vec<f64>,
}

impl Latency {
    /// 记一个样本。
    pub fn record(&mut self, ms: f64) {
        self.samples_ms.push(ms);
    }

    /// 样本数。
    pub fn count(&self) -> usize {
        self.samples_ms.len()
    }

    /// 百分位（线性插值）；样本为空返回 `None`。
    pub fn percentile(&self, p: f64) -> Option<f64> {
        let mut samples = self.samples_ms.clone();
        if samples.is_empty() {
            return None;
        }
        samples.sort_by(|a, b| a.total_cmp(b));
        let index = (p / 100.0) * (samples.len() - 1) as f64;
        let lower = index.floor() as usize;
        let upper = index.ceil() as usize;
        let weight = index - lower as f64;
        Some(samples[lower] * (1.0 - weight) + samples[upper] * weight)
    }
}

/// 对一条分类回复判定：生产校验器为主，无效 JSON 由本模块单独识别
/// （它度量模型输出格式质量，校验器对两者同样拒绝，但指标要分列）。
pub fn judge_classify_reply(
    reply: &str,
    expected: TaskKind,
    allowed: &[TaskKind],
) -> ClassifyVerdict {
    if !is_json_like(reply) {
        return ClassifyVerdict::InvalidJson;
    }
    match gloss_core::classify::parse_classify_reply(reply, allowed) {
        Ok(actual) if actual == expected => ClassifyVerdict::Correct,
        Ok(actual) => ClassifyVerdict::Wrong(actual),
        Err(_) => ClassifyVerdict::Rejected,
    }
}

/// 回复是否「长得像 JSON」：裸 JSON 或任一围栏内是 JSON。只判形态，
/// 不判 kind 合法性——那归生产校验器。
fn is_json_like(reply: &str) -> bool {
    let trimmed = reply.trim();
    if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        return true;
    }
    fenced_json(trimmed).is_some_and(|json| serde_json::from_str::<serde_json::Value>(json).is_ok())
}

/// 提取围栏内 JSON：```gloss 围栏优先，退到任意 ``` 围栏（语言标识行
/// 跳过）。无围栏返回 `None`。
fn fenced_json(reply: &str) -> Option<&str> {
    let start = reply
        .find(STRUCTURED_FENCE)
        .map(|pos| pos + STRUCTURED_FENCE.len());
    let after_marker = match start {
        Some(pos) => &reply[pos..],
        None => {
            let pos = reply.find("```")?;
            &reply[pos + 3..]
        }
    };
    let content = match after_marker.find('\n') {
        Some(line_end) => &after_marker[line_end + 1..],
        None => after_marker,
    };
    let end = content.find("```")?;
    Some(content[..end].trim())
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

#[cfg(test)]
mod tests {
    use super::{ClassifyVerdict, Latency, TaskVerdict, judge_classify_reply};
    use crate::dataset::{TaskCase, load_task};
    use gloss_core::task::{OutcomeStructured, TaskKind};

    fn allowed() -> Vec<TaskKind> {
        vec![
            TaskKind::TranslateWord,
            TaskKind::TranslateSentence,
            TaskKind::ExplainCode,
        ]
    }

    #[test]
    fn classify_verdicts_cover_the_matrix() {
        let a = allowed();
        assert_eq!(
            judge_classify_reply("{\"kind\":\"TranslateWord\"}", TaskKind::TranslateWord, &a),
            ClassifyVerdict::Correct
        );
        assert_eq!(
            judge_classify_reply(
                "{\"kind\":\"TranslateSentence\"}",
                TaskKind::TranslateWord,
                &a
            ),
            ClassifyVerdict::Wrong(TaskKind::TranslateSentence)
        );
        assert_eq!(
            judge_classify_reply("我觉得这是一段翻译", TaskKind::TranslateWord, &a),
            ClassifyVerdict::InvalidJson
        );
        assert_eq!(
            judge_classify_reply("{\"kind\":\"Nonsense\"}", TaskKind::TranslateWord, &a),
            ClassifyVerdict::Rejected
        );
        assert_eq!(
            judge_classify_reply("{\"kind\":\"ImageOcr\"}", TaskKind::TranslateWord, &a),
            ClassifyVerdict::Rejected,
            "kind outside the allowed list is a rejection"
        );
    }

    #[test]
    fn task_verdict_reads_the_four_levels() {
        let case = word_case();
        let good = TaskVerdict::for_reply(
            &case,
            "正文\n```gloss\n{\"word\":\"gloss\",\"phonetic\":null,\"senses\":[]}\n```",
        );
        assert!(!good.degraded(), "complete contract is not degraded");
        assert!(good.fields_complete);

        let no_fence = TaskVerdict::for_reply(&case, "只有正文");
        assert!(no_fence.degraded());
        assert!(!no_fence.fence_present);
        assert!(
            matches!(no_fence.outcome, OutcomeStructured::Plain { title: None }),
            "production outcome degrades to Plain for word kind without the fence"
        );

        let bad_json = TaskVerdict::for_reply(&case, "正文\n```gloss\n{broken\n```");
        assert!(bad_json.fence_present && !bad_json.json_parseable && bad_json.degraded());
    }

    #[test]
    fn field_completeness_requires_the_contract_keys() {
        let case = word_case();
        let missing_senses = TaskVerdict::for_reply(&case, "```gloss\n{\"word\":\"gloss\"}\n```");
        assert!(!missing_senses.fields_complete && missing_senses.degraded());
    }

    #[test]
    fn latency_percentiles_interpolate() {
        let mut latency = Latency::default();
        assert_eq!(latency.percentile(50.0), None);
        for ms in [10.0, 20.0, 30.0, 40.0] {
            latency.record(ms);
        }
        assert_eq!(latency.percentile(50.0), Some(25.0));
        assert_eq!(latency.percentile(95.0), Some(38.5));
        assert_eq!(latency.percentile(0.0), Some(10.0));
        assert_eq!(latency.percentile(100.0), Some(40.0));
    }

    fn word_case() -> TaskCase {
        let jsonl = "{\"id\":\"w1\",\"kind\":\"TranslateWord\",\"text\":\"gloss\",\"reference\":{\"word\":\"gloss\",\"phonetic\":null,\"senses\":[]}}\n";
        load_task(jsonl)
            .expect("case")
            .into_iter()
            .next()
            .expect("one case")
    }
}
