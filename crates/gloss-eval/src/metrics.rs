//! 评分器：把「回复原文加期望」折算成确定性指标。
//!
//! 分类轨走生产校验器（核心层分类解析）；任务轨的完成态判定走
//! 「生产同源」的契约校验器（核心层任务校验）——评测受依赖方向约束
//! 不能消费应用层（gloss-app），核心层的契约模块就是两侧共用的那份
//! 契约。修复轨迹的违约类别与修复轮次同样经生产校验器逐轮还原
//! （违约统计），与生产运行结果的修复记录同口径。本模块是纯函数集，
//! 执行器与单测共用。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use gloss_core::schema::{self, Violation};
use gloss_core::task::{TaskKind, TaskOutcome};

use crate::dataset::TaskCase;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassifyVerdict {
    Correct,
    Wrong(TaskKind),
    InvalidJson,
    Rejected,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViolationTrack {
    pub violations: BTreeMap<Violation, usize>,
    pub repairs: u32,
}

pub fn violation_track(
    validate: impl Fn(&str) -> Result<(), Violation>,
    rounds: &[String],
) -> ViolationTrack {
    let mut track = ViolationTrack {
        repairs: (rounds.len().saturating_sub(1)) as u32,
        violations: BTreeMap::new(),
    };
    for round in rounds {
        if let Err(violation) = validate(round) {
            *track.violations.entry(violation).or_default() += 1;
        }
    }
    track
}

pub fn violation_tag(violation: &Violation) -> &'static str {
    match violation {
        Violation::EmptyReply => "empty",
        Violation::NotJson => "not-json",
        Violation::MissingNote => "missing-note",
        Violation::KindNotAllowed => "kind-not-allowed",
    }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassifyMetrics {
    pub evaluated: usize,
    pub correct: usize,
    pub confusion: BTreeMap<TaskKind, BTreeMap<TaskKind, usize>>,
    pub invalid_json: usize,
    pub rejected: usize,
    pub engine_errors: usize,
    pub violations: BTreeMap<Violation, usize>,
    pub repair_rounds: u32,
}

impl ClassifyMetrics {
    pub fn accuracy(&self) -> f64 {
        ratio(self.correct, self.evaluated)
    }

    pub fn fallback_rate(&self) -> f64 {
        ratio(
            self.invalid_json + self.rejected + self.engine_errors,
            self.evaluated,
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaskVerdict {
    pub json_object: bool,
    pub note_present: bool,
    pub fields_complete: bool,
    pub outcome: TaskOutcome,
    pub violations: BTreeMap<Violation, usize>,
    pub repairs: u32,
}

impl TaskVerdict {
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
                crate::dataset::required_fields(case.kind)
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
            violations: BTreeMap::new(),
            repairs: 0,
        }
    }

    pub fn for_rounds(case: &TaskCase, rounds: &[String]) -> Self {
        let mut verdict = Self::for_reply(case, rounds.last().map(String::as_str).unwrap_or(""));
        let track = violation_track(
            |raw| schema::validate_task(case.kind, raw).map(|_| ()),
            rounds,
        );
        verdict.violations = track.violations;
        verdict.repairs = track.repairs;
        verdict
    }

    pub fn degraded(&self) -> bool {
        self.outcome.degraded
    }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskMetrics {
    pub evaluated: usize,
    pub json_object: usize,
    pub note_present: usize,
    pub fields_complete: usize,
    pub degraded: usize,
    pub violations: BTreeMap<Violation, usize>,
    pub repair_rounds: u32,
}

impl TaskMetrics {
    pub fn record(&mut self, verdict: &TaskVerdict) {
        self.evaluated += 1;
        self.json_object += usize::from(verdict.json_object);
        self.note_present += usize::from(verdict.note_present);
        self.fields_complete += usize::from(verdict.fields_complete);
        self.degraded += usize::from(verdict.degraded());
        for (violation, count) in &verdict.violations {
            *self.violations.entry(*violation).or_default() += count;
        }
        self.repair_rounds += verdict.repairs;
    }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Latency {
    samples_ms: Vec<f64>,
}

impl Latency {
    pub fn record(&mut self, ms: f64) {
        self.samples_ms.push(ms);
    }

    pub fn count(&self) -> usize {
        self.samples_ms.len()
    }

    pub fn percentile(&mut self, p: f64) -> Option<f64> {
        if self.samples_ms.is_empty() {
            return None;
        }
        self.samples_ms.sort_by(|a, b| a.total_cmp(b));
        let index = (p / 100.0) * (self.samples_ms.len() - 1) as f64;
        let lower = index.floor() as usize;
        let upper = index.ceil() as usize;
        let weight = index - lower as f64;
        Some(self.samples_ms[lower] * (1.0 - weight) + self.samples_ms[upper] * weight)
    }
}

pub fn judge_classify_reply(reply: &str, expected: TaskKind) -> ClassifyVerdict {
    if !is_json_like(reply) {
        return ClassifyVerdict::InvalidJson;
    }
    match gloss_core::classify::parse_classify_reply(reply) {
        Ok(actual) if actual == expected => ClassifyVerdict::Correct,
        Ok(actual) => ClassifyVerdict::Wrong(actual),
        Err(_) => ClassifyVerdict::Rejected,
    }
}

fn is_json_like(reply: &str) -> bool {
    let trimmed = reply.trim();
    if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        return true;
    }
    first_fenced_json(trimmed)
        .is_some_and(|json| serde_json::from_str::<serde_json::Value>(json).is_ok())
}

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
    use super::{ClassifyVerdict, Latency, TaskVerdict, judge_classify_reply, violation_track};
    use crate::dataset::{TaskCase, load_task};
    use gloss_core::schema::Violation;
    use gloss_core::task::{OutcomeStructured, TaskKind};

    #[test]
    fn classify_verdicts_cover_the_matrix() {
        assert_eq!(
            judge_classify_reply("{\"kind\":\"TranslateWord\"}", TaskKind::TranslateWord),
            ClassifyVerdict::Correct
        );
        assert_eq!(
            judge_classify_reply("{\"kind\":\"TranslateSentence\"}", TaskKind::TranslateWord),
            ClassifyVerdict::Wrong(TaskKind::TranslateSentence)
        );
        assert_eq!(
            judge_classify_reply("{\"kind\":\"Fallback\"}", TaskKind::General),
            ClassifyVerdict::Correct,
            "the fallback category maps to the general kind before judging"
        );
        assert_eq!(
            judge_classify_reply("我觉得这是一段翻译", TaskKind::TranslateWord),
            ClassifyVerdict::InvalidJson
        );
        assert_eq!(
            judge_classify_reply("{\"kind\":\"Nonsense\"}", TaskKind::TranslateWord),
            ClassifyVerdict::Rejected
        );
        assert_eq!(
            judge_classify_reply("{\"kind\":\"ImageExplain\"}", TaskKind::TranslateWord),
            ClassifyVerdict::Rejected,
            "a kind outside the classify template is a rejection"
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
    fn violation_track_counts_categories_and_rounds_from_the_trajectory() {
        let case = word_case();
        let good = r#"{"note":"义","structured":{"phonetic":null,"examples":[]}}"#.to_owned();
        let prose = "整段散文，不是 JSON".to_owned();

        let track = violation_track(
            |raw| gloss_core::schema::validate_task(case.kind, raw).map(|_| ()),
            &[prose.clone(), prose.clone(), good.clone()],
        );
        assert_eq!(track.repairs, 2);
        assert_eq!(track.violations.get(&Violation::NotJson), Some(&2));

        let repaired = TaskVerdict::for_rounds(&case, &[prose.clone(), good.clone()]);
        assert!(!repaired.degraded(), "a repaired trajectory lands normally");
        assert_eq!(repaired.repairs, 1);
        assert_eq!(repaired.violations.get(&Violation::NotJson), Some(&1));
        assert_eq!(repaired.outcome.note, "义");

        let exhausted =
            TaskVerdict::for_rounds(&case, &[prose.clone(), prose.clone(), prose.clone(), prose]);
        assert!(
            exhausted.degraded(),
            "an exhausted trajectory still degrades"
        );
        assert_eq!(exhausted.repairs, 3);
        assert_eq!(
            exhausted.violations.get(&Violation::NotJson),
            Some(&4),
            "the exhausted final round violates too"
        );
        assert_eq!(exhausted.outcome.note, "");

        let empty = TaskVerdict::for_rounds(&case, &[String::new(), good]);
        assert_eq!(
            empty.violations.get(&Violation::EmptyReply),
            Some(&1),
            "an empty first round is an EmptyReply violation"
        );
        assert!(!empty.degraded());
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
