//! 逐条明细：一条数据集样例的输入、期望与逐轮修复轨迹，供检视页
//! （[`crate::inspect`]）逐条渲染。重放轨与 live 轨共用同一形状——检视体验
//! 不随回复来源切换。
//!
//! 结算函数（[`classify_parts`] / [`task_parts`]）一次校验同时产出明细与
//! 指标侧产物（[`ClassifyVerdict`] / [`TaskVerdict`] / [`ViolationTrack`]）：
//! 轨迹逐轮只跑一遍生产校验器，明细与报告指标天然同源；两者的计数一致
//! 由 runner 的汇总测试锁定。

use std::collections::BTreeMap;

use serde::Serialize;

use gloss_core::schema::{self, Violation};
use gloss_core::task::TaskKind;

use crate::dataset::{ClassifyCase, TaskCase};
use crate::metrics::{
    ClassifyVerdict, TaskVerdict, ViolationTrack, judge_classify_reply, violation_tag,
};

/// 一轮的明细：完整回复原文 + 该轮违约短名（契约轮为 `None`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RoundDetail {
    pub text: String,
    pub violation: Option<String>,
}

/// 分类判定的可序列化视图（与 [`ClassifyVerdict`] 同矩阵，供检视页徽标）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum ClassifyOutcome {
    Correct,
    Wrong { actual: TaskKind },
    InvalidJson,
    Rejected,
}

/// 任务完成态的可序列化视图（[`TaskVerdict`] 的四级标志 + 末轮 note）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskOutcome {
    pub json_object: bool,
    pub note_present: bool,
    pub fields_complete: bool,
    pub degraded: bool,
    /// 末轮 note 正文（降级为空串）——检视时直接读产出，不必自行解析轮文本。
    pub note: String,
}

/// 一条样例的完整明细。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CaseDetail {
    pub id: String,
    /// 分组名：分类轨为「分类」，任务轨为数据集名（task_translate_word 等）。
    pub group: String,
    /// 输入文本（选区原文 / 代码等）。
    pub text: String,
    /// 期望判定（分类）或条目 kind（任务）。
    pub expected: TaskKind,
    /// 修复轮次（生产 `RunOutput.repairs` / `ClassifyDecision.repairs` 同口径）。
    pub repairs: u32,
    pub rounds: Vec<RoundDetail>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub classify: Option<ClassifyOutcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<TaskOutcome>,
    /// live 轨的请求本身失败（网络/限流）：与模型行为分账，检视页按
    /// 「引擎失败」而不是模型错误展示。
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub engine_error: bool,
}

/// 分类样例的结算：明细 + 判定 + 违约轨迹，一次校验三方消费（指标入账、
/// 明细落行、报告聚合）。
pub fn classify_parts(
    case: &ClassifyCase,
    rounds: &[String],
) -> (CaseDetail, ClassifyVerdict, ViolationTrack) {
    let (rounds_detail, track) =
        rounds_with_track(|raw| schema::validate_classify(raw).map(|_| ()), rounds);
    let reply = rounds.last().map(String::as_str).unwrap_or("");
    let verdict = judge_classify_reply(reply, case.expected);
    let classify = match &verdict {
        ClassifyVerdict::Correct => ClassifyOutcome::Correct,
        ClassifyVerdict::Wrong(actual) => ClassifyOutcome::Wrong { actual: *actual },
        ClassifyVerdict::InvalidJson => ClassifyOutcome::InvalidJson,
        ClassifyVerdict::Rejected => ClassifyOutcome::Rejected,
    };
    (
        CaseDetail {
            id: case.id.clone(),
            group: CLASSIFY_GROUP.to_owned(),
            text: case.text.clone(),
            expected: case.expected,
            repairs: track.repairs,
            rounds: rounds_detail,
            classify: Some(classify),
            task: None,
            engine_error: false,
        },
        verdict,
        track,
    )
}

/// 任务样例的结算：明细 + 判定（完成态与轨迹一次折算）。`group` 由调用方
/// 给出（数据集名在 runner 的数据集清单里，条目自身不携带）。
pub fn task_parts(group: &str, case: &TaskCase, rounds: &[String]) -> (CaseDetail, TaskVerdict) {
    let (rounds_detail, track) = rounds_with_track(
        |raw| schema::validate_task(case.kind, raw).map(|_| ()),
        rounds,
    );
    let mut verdict = TaskVerdict::for_reply(case, rounds.last().map(String::as_str).unwrap_or(""));
    verdict.violations = track.violations.clone();
    verdict.repairs = track.repairs;
    (
        CaseDetail {
            id: case.id.clone(),
            group: group.to_owned(),
            text: case.text.clone(),
            expected: case.kind,
            repairs: track.repairs,
            rounds: rounds_detail,
            classify: None,
            task: Some(TaskOutcome {
                json_object: verdict.json_object,
                note_present: verdict.note_present,
                fields_complete: verdict.fields_complete,
                degraded: verdict.degraded(),
                note: verdict.outcome.note.clone(),
            }),
            engine_error: false,
        },
        verdict,
    )
}

/// live 轨的引擎失败明细：无轮次无判定，只留输入与「请求失败」标记
/// （指标侧分类轨单列 engine_errors、任务轨并入降级，明细统一按本标记展示）。
pub fn engine_error_detail(
    group: &str,
    id: &str,
    text: &str,
    expected: TaskKind,
    task_track: bool,
) -> CaseDetail {
    CaseDetail {
        id: id.to_owned(),
        group: group.to_owned(),
        text: text.to_owned(),
        expected,
        repairs: 0,
        rounds: Vec::new(),
        classify: None,
        task: task_track.then(|| TaskOutcome {
            json_object: false,
            note_present: false,
            fields_complete: false,
            degraded: true,
            note: String::new(),
        }),
        engine_error: true,
    }
}

/// 分类轨的分组名（检视页与 runner 共用同一常量，过滤键不漂移）。
pub const CLASSIFY_GROUP: &str = "分类";

/// 轨迹的单遍折算：逐轮跑生产校验器，同时产出逐轮标签与聚合违约统计
/// （[`ViolationTrack`] 同口径：修复轮次 = 轮数 − 1，末轮违约也计入）。
fn rounds_with_track(
    validate: impl Fn(&str) -> Result<(), Violation>,
    rounds: &[String],
) -> (Vec<RoundDetail>, ViolationTrack) {
    let mut violations = BTreeMap::new();
    let rounds_detail = rounds
        .iter()
        .map(|text| {
            let violation = validate(text).err();
            if let Some(violation) = &violation {
                *violations.entry(*violation).or_default() += 1;
            }
            RoundDetail {
                text: text.clone(),
                violation: violation.map(|violation| violation_tag(&violation).to_owned()),
            }
        })
        .collect();
    (
        rounds_detail,
        ViolationTrack {
            repairs: rounds.len().saturating_sub(1) as u32,
            violations,
        },
    )
}

#[cfg(test)]
mod tests {
    use gloss_core::schema::Violation;
    use gloss_core::task::TaskKind;

    use super::{CLASSIFY_GROUP, classify_parts, engine_error_detail, task_parts};

    fn classify_case(jsonl: &str) -> crate::dataset::ClassifyCase {
        crate::dataset::load_classify(jsonl)
            .expect("case")
            .into_iter()
            .next()
            .expect("one case")
    }

    fn task_case(jsonl: &str) -> crate::dataset::TaskCase {
        crate::dataset::load_task(jsonl)
            .expect("case")
            .into_iter()
            .next()
            .expect("one case")
    }

    #[test]
    fn classify_parts_maps_verdicts_and_tags_rounds() {
        let case = classify_case(
            "{\"id\":\"zh-word-01\",\"text\":\"光泽\",\"expected\":\"TranslateWord\"}\n",
        );
        let good = "{\"kind\":\"TranslateWord\"}".to_owned();
        let prose = "整段散文".to_owned();

        let (detail, verdict, track) = classify_parts(&case, &[prose.clone(), good]);
        assert_eq!(verdict, crate::metrics::ClassifyVerdict::Correct);
        assert_eq!(detail.repairs, 1);
        assert_eq!(detail.rounds[0].violation.as_deref(), Some("not-json"));
        assert_eq!(detail.rounds[1].violation, None);
        assert_eq!(track.violations.get(&Violation::NotJson), Some(&1));
        assert_eq!(detail.classify, Some(super::ClassifyOutcome::Correct));
        assert_eq!(detail.group, CLASSIFY_GROUP);

        let (detail, verdict, _) =
            classify_parts(&case, &["{\"kind\":\"TranslateSentence\"}".to_owned()]);
        assert_eq!(
            verdict,
            crate::metrics::ClassifyVerdict::Wrong(TaskKind::TranslateSentence)
        );
        assert_eq!(
            detail.classify,
            Some(super::ClassifyOutcome::Wrong {
                actual: TaskKind::TranslateSentence
            })
        );

        let (detail, verdict, _) = classify_parts(&case, &["不是 JSON".to_owned()]);
        assert_eq!(verdict, crate::metrics::ClassifyVerdict::InvalidJson);
        assert_eq!(detail.classify, Some(super::ClassifyOutcome::InvalidJson));

        let (detail, verdict, _) = classify_parts(&case, &["{\"kind\":\"Nonsense\"}".to_owned()]);
        assert_eq!(verdict, crate::metrics::ClassifyVerdict::Rejected);
        assert_eq!(detail.classify, Some(super::ClassifyOutcome::Rejected));
    }

    #[test]
    fn task_parts_reads_completion_state_and_round_tags() {
        let case = task_case(
            "{\"id\":\"task-word-01\",\"kind\":\"TranslateWord\",\"text\":\"gloss\",\"reference\":{\"phonetic\":null,\"examples\":[]}}\n",
        );
        let good = r#"{"note":"幂等的","structured":{"phonetic":null,"examples":[]}}"#.to_owned();
        let prose = "整段散文".to_owned();

        let (detail, verdict) = task_parts("task_translate_word", &case, &[prose.clone(), good]);
        assert_eq!(verdict.repairs, 1);
        assert!(!verdict.degraded());
        assert_eq!(detail.repairs, 1);
        assert_eq!(detail.group, "task_translate_word");
        let outcome = detail.task.as_ref().expect("task outcome");
        assert!(outcome.json_object && outcome.note_present && outcome.fields_complete);
        assert!(!outcome.degraded);
        assert_eq!(outcome.note, "幂等的");
        assert_eq!(detail.rounds[0].violation.as_deref(), Some("not-json"));

        let (detail, verdict) = task_parts("task_translate_word", &case, &[prose]);
        assert!(verdict.degraded());
        let outcome = detail.task.as_ref().expect("task outcome");
        assert!(outcome.degraded);
        assert_eq!(outcome.note, "", "a degraded verdict carries no note");
        assert_eq!(detail.rounds[0].violation.as_deref(), Some("not-json"));
    }

    #[test]
    fn engine_error_details_carry_the_marker_per_track() {
        let classify = engine_error_detail(
            CLASSIFY_GROUP,
            "zh-word-01",
            "光泽",
            TaskKind::TranslateWord,
            false,
        );
        assert!(classify.engine_error);
        assert!(classify.rounds.is_empty());
        assert!(classify.classify.is_none() && classify.task.is_none());

        let task = engine_error_detail(
            "task_translate_word",
            "task-word-01",
            "gloss",
            TaskKind::TranslateWord,
            true,
        );
        assert!(task.engine_error);
        let outcome = task.task.as_ref().expect("task outcome");
        assert!(
            outcome.degraded,
            "the task track folds engine failures into degraded"
        );
        assert!(task.classify.is_none());
    }
}
