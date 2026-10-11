//! 评分器：把「回复原文 + 期望」折算成确定性指标。
//!
//! 分类轨走生产校验器（[`gloss_core::classify::parse_classify_reply`]）；
//! 任务轨的完成态判定走**生产同源**的契约校验器
//! （[`gloss_core::schema::validate_task`]）——eval 受依赖方向约束不能消费
//! gloss-app，core 的 schema 模块就是两侧共用的那份契约。本模块是纯函数
//! 集，runner 与单测共用。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use gloss_core::schema::{self};
use gloss_core::task::{TaskKind, TaskOutcome};

use crate::dataset::TaskCase;

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
    /// 回复不是合法 JSON 的次数（模型输出格式质量问题）。
    pub invalid_json: usize,
    /// JSON 合法但被生产校验器拒绝的次数（契约问题）。
    pub rejected: usize,
    /// 分类请求本身失败（网络/限流）的次数——与模型行为无关，不计入
    /// 格式类指标，但计入回退率（生产编排对这类条目同样落兜底）。
    pub engine_errors: usize,
}

impl ClassifyMetrics {
    /// 判定正确率（分母 = 进入统计的条数）。
    pub fn accuracy(&self) -> f64 {
        ratio(self.correct, self.evaluated)
    }

    /// 回退率：生产编排会对这些条目落兜底 kind（无效 JSON + 被拒绝 +
    /// 引擎失败）。
    pub fn fallback_rate(&self) -> f64 {
        ratio(
            self.invalid_json + self.rejected + self.engine_errors,
            self.evaluated,
        )
    }
}

/// 任务回复的契约达成情况（比「解析成功与否」细：四个指标各自独立）。
#[derive(Debug, Clone, PartialEq)]
pub struct TaskVerdict {
    /// 回复是合法 JSON 对象（契约硬约束之一）。
    pub json_object: bool,
    /// JSON 对象携带非空 `note`（注；硬约束之二，词卡/句译/讲解/图像）。
    pub note_present: bool,
    /// `structured` 对象在场且携带 kind 契约必需的疏证键（软约束项，
    /// 缺失只降丰富度不降产物；JSON 对象在场才可能为真）。
    pub fields_complete: bool,
    /// 生产同源的解析产物（与 UI 收到的 [`TaskOutcome`] 同构，含降级
    /// 标记——违约即空 note 空变体 `degraded = true`）。
    pub outcome: TaskOutcome,
}

impl TaskVerdict {
    /// 按 [`TaskCase`] 的 kind 与回复原文逐级判定。
    ///
    /// 判定与生产完成态解析同源（gloss-app `finalize::complete` 同样走
    /// [`gloss_core::schema::validate_task`]）；`json_object` /
    /// `note_present` / `fields_complete` 从违约类别与 `structured` 在场
    /// 情况折算，供分档指标。
    pub fn for_reply(case: &TaskCase, reply: &str) -> Self {
        let parsed = serde_json::from_str::<serde_json::Value>(reply.trim())
            .ok()
            .filter(|value| value.is_object());
        let json_object = parsed.is_some();
        let note_present = parsed.as_ref().is_some_and(|value| {
            value
                .get("note")
                .and_then(|n| n.as_str())
                .is_some_and(|note| !note.trim().is_empty())
        });
        let fields_complete = parsed
            .as_ref()
            .and_then(|value| value.get("structured"))
            .filter(|value| value.is_object())
            .is_some_and(|structured| {
                required_structured_fields(case.kind)
                    .iter()
                    .all(|key| structured.get(*key).is_some())
            });
        let outcome = match schema::validate_task(case.kind, reply) {
            Ok((note, structured)) => TaskOutcome {
                kind: case.kind,
                note,
                structured,
                degraded: false,
            },
            Err(_) => TaskOutcome {
                kind: case.kind,
                note: String::new(),
                structured: schema::empty_structured(case.kind),
                degraded: true,
            },
        };
        Self {
            json_object,
            note_present,
            fields_complete,
            outcome,
        }
    }

    /// 降级判定：契约校验器判违约（非 JSON / 空回复 / 缺 note）。存量
    /// 旧契约夹具（扁平录制）天然落降级轨——生产对它们出降级卡，指标
    /// 度量的是「模型有没有按现行嵌套契约输出」。
    pub fn degraded(&self) -> bool {
        self.outcome.degraded
    }
}

/// kind 的 `structured` 契约必需键：三行同构表里只有 `examples` 恒在
/// （phonetic / code_language 允许缺省或 null；`note` 单独由
/// `note_present` 计）。
fn required_structured_fields(_kind: TaskKind) -> &'static [&'static str] {
    &["examples"]
}

/// 任务指标：契约 JSON 率 / note 在场率 / 字段完整率 / 降级率。
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TaskMetrics {
    /// 进入统计的条数。
    pub evaluated: usize,
    /// 回复是契约 JSON 对象的条数。
    pub json_object: usize,
    /// JSON 对象带 `note`（注）的条数。
    pub note_present: usize,
    /// 必需字段完整数。
    pub fields_complete: usize,
    /// 降级数（见 [`TaskVerdict::degraded`]）。live 轨的任务请求失败也
    /// 记入此处——降级率因此混入传输失败（与分类轨单列
    /// engine_errors 不同），读数时注意。
    pub degraded: usize,
}

impl TaskMetrics {
    /// 记入一个判定。
    pub fn record(&mut self, verdict: &TaskVerdict) {
        self.evaluated += 1;
        self.json_object += usize::from(verdict.json_object);
        self.note_present += usize::from(verdict.note_present);
        self.fields_complete += usize::from(verdict.fields_complete);
        self.degraded += usize::from(verdict.degraded());
    }

    /// 契约 JSON 率。
    pub fn json_rate(&self) -> f64 {
        ratio(self.json_object, self.evaluated)
    }

    /// note 在场率。
    pub fn note_rate(&self) -> f64 {
        ratio(self.note_present, self.evaluated)
    }

    /// 字段完整率。
    pub fn completeness_rate(&self) -> f64 {
        ratio(self.fields_complete, self.evaluated)
    }

    /// 降级率（模型没按现行契约输出的比例）。
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

/// 回复是否「长得像 JSON」：裸 JSON 或**首个** ``` 围栏内是 JSON——
/// 镜像生产 `parse_classify_reply` 的裸先行、首围栏兜底提取（与任务轨
/// 的 rfind 提取不同，多围栏的退化场景下两侧各自与生产对齐）。只判
/// 形态，不判 kind 合法性——那归生产校验器。
fn is_json_like(reply: &str) -> bool {
    let trimmed = reply.trim();
    if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        return true;
    }
    first_fenced_json(trimmed)
        .is_some_and(|json| serde_json::from_str::<serde_json::Value>(json).is_ok())
}

/// 首个 ``` 围栏内 JSON（语言标识行跳过）；镜像生产分类回复的容错提取。
fn first_fenced_json(reply: &str) -> Option<&str> {
    let pos = reply.find("```")?;
    let after_marker = &reply[pos + 3..];
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
            judge_classify_reply("{\"kind\":\"ImageExplain\"}", TaskKind::TranslateWord, &a),
            ClassifyVerdict::Rejected,
            "kind outside the allowed list is a rejection"
        );
    }

    #[test]
    fn task_verdict_reads_the_four_levels() {
        let case = word_case();
        let good = TaskVerdict::for_reply(
            &case,
            r#"{"note":"正文","structured":{"phonetic":null,"examples":[]}}"#,
        );
        assert!(!good.degraded(), "complete contract is not degraded");
        assert!(good.json_object && good.note_present && good.fields_complete);

        let plain = TaskVerdict::for_reply(&case, "只有正文");
        assert!(plain.degraded());
        assert!(!plain.json_object);
        assert_eq!(plain.outcome.note, "", "a degraded verdict carries no note");
        assert!(plain.outcome.degraded);

        let bad_json = TaskVerdict::for_reply(&case, r#"{"note":"正文","structured""#);
        assert!(!bad_json.json_object && bad_json.degraded());

        let bare_note = TaskVerdict::for_reply(&case, r#"{"note":"只有义"}"#);
        assert!(
            bare_note.json_object
                && bare_note.note_present
                && !bare_note.fields_complete
                && !bare_note.degraded(),
            "missing structured is a soft gap: the note keeps the product alive"
        );
    }

    #[test]
    fn legacy_flat_and_fenced_replies_degrade_like_production() {
        let case = word_case();
        // 旧扁平契约（无 structured 包裹）：note 在场保产物，疏证字段
        // 按软规则回落，判定不降级。
        let flat =
            TaskVerdict::for_reply(&case, r#"{"note":"正文","phonetic":null,"examples":[]}"#);
        assert!(!flat.degraded() && !flat.fields_complete);
        assert_eq!(
            flat.outcome.structured,
            OutcomeStructured::WordCard {
                phonetic: None,
                examples: Vec::new(),
            }
        );

        // 围栏输出整体违约：生产已无围栏容错，落降级产物。
        let fence = TaskVerdict::for_reply(&case, "正文\n```gloss\n{\"note\":\"旧契约\"}\n```");
        assert!(fence.degraded());
        assert_eq!(fence.outcome.note, "");
    }

    #[test]
    fn field_completeness_requires_the_nested_contract_keys() {
        let case = word_case();
        let missing_examples =
            TaskVerdict::for_reply(&case, r#"{"note":"正文","structured":{"phonetic":null}}"#);
        assert!(!missing_examples.fields_complete && !missing_examples.degraded());

        let code_case = code_case();
        let code = TaskVerdict::for_reply(
            &code_case,
            r#"{"note":"讲解","structured":{"examples":[],"code_language":"rust"}}"#,
        );
        assert!(code.fields_complete && !code.degraded());
        assert!(
            matches!(
                &code.outcome.structured,
                OutcomeStructured::Code { code_language, .. }
                    if code_language.as_deref() == Some("rust")
            ),
            "the language verdict rides the Code variant"
        );
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

    fn case(jsonl: &str) -> TaskCase {
        load_task(jsonl)
            .expect("case")
            .into_iter()
            .next()
            .expect("one case")
    }

    fn word_case() -> TaskCase {
        case(
            "{\"id\":\"w1\",\"kind\":\"TranslateWord\",\"text\":\"gloss\",\"reference\":{\"phonetic\":null,\"examples\":[]}}\n",
        )
    }

    fn code_case() -> TaskCase {
        case(
            "{\"id\":\"c1\",\"kind\":\"ExplainCode\",\"text\":\"fn main() {}\",\"reference\":{\"examples\":[]}}\n",
        )
    }

    fn image_case() -> TaskCase {
        case(
            "{\"id\":\"i1\",\"kind\":\"ImageExplain\",\"text\":\"chart.png\",\"reference\":{\"examples\":[]}}\n",
        )
    }

    #[test]
    fn image_explain_lands_on_the_plain_variant() {
        let case = image_case();
        let good = TaskVerdict::for_reply(
            &case,
            r#"{"note":"描述","structured":{"examples":["要点一",42,"要点二"]}}"#,
        );
        assert!(!good.degraded(), "complete contract is not degraded");
        assert_eq!(
            good.outcome.structured,
            OutcomeStructured::Plain {
                examples: vec!["要点一".to_owned(), "要点二".to_owned()],
            },
            "soft rules skip bad entries; image explain shares the plain variant"
        );

        let bare = TaskVerdict::for_reply(&case, r#"{"note":"只有描述"}"#);
        assert!(!bare.degraded(), "missing structured is a soft gap");
        assert_eq!(
            bare.outcome.structured,
            OutcomeStructured::Plain {
                examples: Vec::new(),
            }
        );

        let prose = TaskVerdict::for_reply(&case, "只有正文");
        assert!(prose.degraded());
        assert_eq!(prose.outcome.note, "");
    }
}
