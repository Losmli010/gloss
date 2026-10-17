//! 评分器：把「回复原文 + 期望」折算成确定性指标。
//!
//! 分类轨走生产校验器（[`gloss_core::classify::parse_classify_reply`]）；
//! 任务轨的完成态判定走**生产同源**的契约校验器
//! （[`gloss_core::schema::validate_task`]）——eval 受依赖方向约束不能消费
//! gloss-app，core 的 schema 模块就是两侧共用的那份契约。修复轨迹的违约
//! 类别与修复轮次同样经生产校验器逐轮还原（[`violation_track`]），与生产
//! `RunOutput.repairs` 同口径。本模块是纯函数集，runner 与单测共用。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use gloss_core::schema::{self, Violation};
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

/// 修复轨迹的违约统计：每一违约轮记一次类别，修复轮次 = 轮数 − 1（与
/// 生产 `RunOutput.repairs` / `ClassifyDecision.repairs` 同口径）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViolationTrack {
    /// 违约类别 → 违约轮次数（含末轮违约——预算耗尽的收尾）。
    pub violations: BTreeMap<Violation, usize>,
    /// 修复轮次总数。
    pub repairs: u32,
}

impl ViolationTrack {
    /// 并入另一条轨迹的计数（逐条目汇总用）。
    pub fn merge(&mut self, other: &Self) {
        for (violation, count) in &other.violations {
            *self.violations.entry(*violation).or_default() += count;
        }
        self.repairs += other.repairs;
    }
}

/// 从修复轨迹（每轮完整回复）折算违约统计：逐轮跑调用方给定的生产
/// 校验器（分类/任务同一份 `schema` 模块）。重放轨与 live 轨共用——
/// live 的轮次来自生产循环本身，折算口径与重放完全一致。
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

/// 违约类别的报表短名（`Display` 措辞是句子，表格单元格里用短名）。
pub fn violation_tag(violation: &Violation) -> &'static str {
    match violation {
        Violation::EmptyReply => "empty",
        Violation::NotJson => "not-json",
        Violation::MissingNote => "missing-note",
        Violation::KindNotAllowed => "kind-not-allowed",
    }
}

/// 分类指标：accuracy / 混淆矩阵 / 无效 JSON 率 / 回退率 / 违约与修复轮次。
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
    /// 违约类别计数（每一违约轮记一次，含修复成功与耗尽两类轨迹）。
    pub violations: BTreeMap<Violation, usize>,
    /// 修复轮次总数（生产 `ClassifyDecision.repairs` 同口径）。
    pub repair_rounds: u32,
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

/// 任务回复的契约达成情况（比「解析成功与否」细：四个指标各自独立，
/// 修复轨迹随行）。
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
    /// 违约类别计数（每一违约轮记一次；单轮判定最多一条）。
    pub violations: BTreeMap<Violation, usize>,
    /// 修复轮次（生产 `RunOutput.repairs` 同口径）。
    pub repairs: u32,
}

impl TaskVerdict {
    /// 按 [`TaskCase`] 的 kind 与回复原文逐级判定（单轮口径：该回复即
    /// 末轮，无修复轨迹）。
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
            violations: BTreeMap::new(),
            repairs: 0,
        }
    }

    /// 按 [`TaskCase`] 的 kind 与**修复轨迹**（每轮完整回复）判定：末轮
    /// 定完成态，全程逐轮经生产校验器折算违约类别与修复轮次——与生产
    /// `RunOutput.repairs` 同口径（重放轨自夹具轮次、live 轨自生产循环
    /// 的轮记录，共用本判定）。
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

    /// 降级判定：契约校验器判违约（非 JSON / 空回复 / 缺 note）。
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

/// 任务指标：契约 JSON 率 / note 在场率 / 字段完整率 / 降级率 / 违约与
/// 修复轮次。
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
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
    /// 违约类别计数（每一违约轮记一次，含修复成功与耗尽两类轨迹）。
    pub violations: BTreeMap<Violation, usize>,
    /// 修复轮次总数（生产 `RunOutput.repairs` 同口径）。
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
    pub fn record(&mut self, ms: f64) {
        self.samples_ms.push(ms);
    }

    pub fn count(&self) -> usize {
        self.samples_ms.len()
    }

    /// 百分位（线性插值）；样本为空返回 `None`。就地按值排序（百分位与
    /// 顺序无关，重复调用幂等），不再为每次查询复制整份样本。
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
/// 刻意比生产校验（严格裸 JSON，无围栏容错）宽一档：生产对两者同样
/// 拒绝，本判定只把「非 JSON 形态」与「形态像但被拒」分列统计。只判
/// 形态，不判 kind 合法性——那归生产校验器。
fn is_json_like(reply: &str) -> bool {
    let trimmed = reply.trim();
    if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        return true;
    }
    first_fenced_json(trimmed)
        .is_some_and(|json| serde_json::from_str::<serde_json::Value>(json).is_ok())
}

/// 首个 ``` 围栏内 JSON（语言标识行跳过）：「长得像 JSON」的形态判定
/// 专用——生产已无围栏容错，本提取不再镜像任何生产路径。
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
