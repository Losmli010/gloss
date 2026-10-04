//! 评分器：把「回复原文 + 期望」折算成确定性指标。
//!
//! 分类轨走生产校验器（[`gloss_core::classify::parse_classify_reply`]）；
//! 任务轨的完成态判定**镜像生产语义**（gloss-app 的 JSON 主路径 + 旧围栏
//! fallback）——eval 受依赖方向约束不能消费 gloss-app，这里按同一条规则
//! 重写并由测试对齐两侧语义。本模块是纯函数集，runner 与单测共用。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use gloss_core::prompt::STRUCTURED_FENCE;
use gloss_core::task::{OutcomeStructured, Sense, TaskKind};

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
    /// 回复是符合现行契约的 JSON 对象（生产完成态解析的主路径命中）。
    pub json_object: bool,
    /// JSON 对象携带 `note`（注；主路径在场的必要项，词卡/句译/讲解）。
    pub note_present: bool,
    /// kind 的契约必需结构化字段全部在场（JSON 对象在场才可能为真）。
    pub fields_complete: bool,
    /// 生产解析结果（与 UI 收到的产物同源，含围栏 fallback 的降级形态）。
    pub outcome: OutcomeStructured,
}

impl TaskVerdict {
    /// 按 [`TaskCase`] 的 kind 与回复原文逐级判定。
    ///
    /// 判定镜像生产完成态解析（gloss-app `finalize::complete`）：整段
    /// 回复解析为 JSON 对象且带 `body` 即主路径；否则退围栏 fallback
    /// （取**最后一个**围栏，镜像生产的 rfind 语义）。
    pub fn for_reply(case: &TaskCase, reply: &str) -> Self {
        let parsed = serde_json::from_str::<serde_json::Value>(reply.trim()).ok();
        let json_object = parsed.is_some();
        let note_present = parsed
            .as_ref()
            .and_then(|value| value.get("note"))
            .and_then(|note| note.as_str())
            .is_some();
        let fields_complete = parsed.as_ref().is_some_and(|value| {
            required_fields(case.kind)
                .iter()
                .all(|key| value.get(*key).is_some())
        });
        let outcome = mirror_complete(case.kind, reply);
        Self {
            json_object,
            note_present,
            fields_complete,
            outcome,
        }
    }

    /// 降级判定：回复不是契约 JSON 对象或缺必需字段。存量围栏夹具（旧
    /// 契约录制）天然落降级轨——生产对它们有 fallback 兜底，指标度量的是
    /// 「模型有没有按现行契约输出」。
    pub fn degraded(&self) -> bool {
        !(self.json_object && self.note_present && self.fields_complete)
    }
}

/// 生产完成态解析的镜像（JSON 主路径 + 围栏 fallback → 结构化字段）。
/// 与 gloss-app `finalize` 同一条规则：`body` 缺失退围栏；坏 sense 条目
/// 跳过；字段缺失按 kind 兜底。
fn mirror_complete(kind: TaskKind, reply: &str) -> OutcomeStructured {
    let note = serde_json::from_str::<serde_json::Value>(reply.trim())
        .ok()
        .and_then(|value| {
            let note = value.get("note").and_then(|note| note.as_str())?.to_owned();
            Some((value, note))
        });
    if let Some((value, note)) = note {
        return match kind {
            TaskKind::TranslateWord => OutcomeStructured::WordCard {
                word: text_field(&value, "word").unwrap_or_default(),
                phonetic: text_field(&value, "phonetic"),
                senses: mirror_senses(value.get("senses")).unwrap_or_default(),
            },
            TaskKind::ImageOcr => OutcomeStructured::Extracted {
                text: value
                    .get("text")
                    .and_then(|text| text.as_str())
                    .map(str::to_owned)
                    .unwrap_or(note),
            },
            _ => OutcomeStructured::Plain {
                title: text_field(&value, "title"),
            },
        };
    }
    let fallback = || match kind {
        TaskKind::ImageOcr => OutcomeStructured::Extracted {
            text: reply.to_owned(),
        },
        _ => OutcomeStructured::Plain { title: None },
    };
    let Some(start) = reply.rfind(STRUCTURED_FENCE) else {
        return fallback();
    };
    let after_marker = &reply[start + STRUCTURED_FENCE.len()..];
    let Some(end_rel) = after_marker.find("```") else {
        return fallback();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(after_marker[..end_rel].trim())
    else {
        return fallback();
    };
    match kind {
        TaskKind::TranslateWord => {
            let Some(senses) = mirror_senses(value.get("senses")) else {
                return fallback();
            };
            OutcomeStructured::WordCard {
                word: text_field(&value, "word").unwrap_or_default(),
                phonetic: text_field(&value, "phonetic"),
                senses,
            }
        }
        TaskKind::ImageOcr => match text_field(&value, "text") {
            Some(text) => OutcomeStructured::Extracted { text },
            None => fallback(),
        },
        _ => OutcomeStructured::Plain {
            title: text_field(&value, "title"),
        },
    }
}

/// 释义数组的镜像解析：meaning 缺失/非字符串的坏条目跳过；字段整体缺失
/// 返回 `None`（与生产同语义：word kind 围栏路径据此落 kind 兜底）。
fn mirror_senses(value: Option<&serde_json::Value>) -> Option<Vec<Sense>> {
    let entries = value?.as_array()?;
    let senses = entries
        .iter()
        .filter_map(|entry| {
            Some(Sense {
                pos: text_field(entry, "pos"),
                meaning: text_field(entry, "meaning")?,
                examples: entry
                    .get("examples")
                    .and_then(|v| v.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|v| v.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect();
    Some(senses)
}

/// 取字符串字段；JSON null 与缺失同样返回 None。
fn text_field(value: &serde_json::Value, key: &str) -> Option<String> {
    value.get(key).and_then(|v| v.as_str()).map(str::to_owned)
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
            r#"{"note":"正文","word":"gloss","phonetic":null,"senses":[]}"#,
        );
        assert!(!good.degraded(), "complete contract is not degraded");
        assert!(good.json_object && good.note_present && good.fields_complete);

        let plain = TaskVerdict::for_reply(&case, "只有正文");
        assert!(plain.degraded());
        assert!(!plain.json_object);
        assert!(
            matches!(plain.outcome, OutcomeStructured::Plain { title: None }),
            "production outcome degrades to Plain for word kind without any contract"
        );

        let bad_json = TaskVerdict::for_reply(&case, r#"{"body":"正文","word":"gloss""#);
        assert!(!bad_json.json_object && bad_json.degraded());
    }

    #[test]
    fn legacy_fence_reply_falls_back_like_production() {
        let case = word_case();
        let fence = TaskVerdict::for_reply(
            &case,
            "正文\n```gloss\n{\"word\":\"gloss\",\"senses\":[]}\n```",
        );
        assert!(fence.degraded(), "the old contract is a degraded reply");
        assert!(
            matches!(&fence.outcome, OutcomeStructured::WordCard { word, .. } if word == "gloss"),
            "production still assembles a complete card via the fence fallback"
        );

        let no_fence = TaskVerdict::for_reply(&case, "只有正文");
        assert!(
            matches!(no_fence.outcome, OutcomeStructured::Plain { title: None }),
            "the kind fallback keeps the whole raw text as the body"
        );
    }

    #[test]
    fn field_completeness_requires_the_contract_keys() {
        let case = word_case();
        let missing_senses = TaskVerdict::for_reply(&case, r#"{"note":"正文","word":"gloss"}"#);
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
