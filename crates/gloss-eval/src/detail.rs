//! 结算明细：一条精选用例的三层结算产物——契约明细与逐轮轨迹、评审
//! 四维分数、回归五态与差异摘要——外加标注结论词汇。本模块是纯数据
//! 模式（schema），全部结算产物类型只住这里；折算计算住契约、评审与
//! 回归结算模块，聚合计算住报告模块。序列化形状即逐例明细文件
//! （iterate.cases.json）与检视页展开态的取数面。

use std::collections::BTreeMap;

use gloss_core::schema::Violation;
use gloss_core::task::TaskKind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvalKind {
    Classify,
    Task(TaskKind),
}

impl EvalKind {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Classify => "分类",
            Self::Task(TaskKind::TranslateWord) => "词卡",
            Self::Task(TaskKind::TranslateSentence) => "句译",
            Self::Task(TaskKind::ExplainCode) => "代码",
            Self::Task(TaskKind::General) => "通用",
            Self::Task(TaskKind::ImageExplain) => "图像",
        }
    }

    pub fn task_kind(&self) -> Option<TaskKind> {
        match self {
            Self::Classify => None,
            Self::Task(kind) => Some(*kind),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractCheck {
    FieldsComplete,
    KindMatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractOutcome {
    pub json_object: bool,
    pub checks: BTreeMap<ContractCheck, bool>,
    pub degraded: bool,
    pub violations: BTreeMap<Violation, usize>,
    pub repairs: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractState {
    Valid,
    Softgap,
    Degraded,
    EngineError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JudgeScores {
    pub accuracy: u8,
    pub completeness: u8,
    pub fluency: u8,
    pub presentation: u8,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelVerdict {
    Pass,
    Fail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleVerdict {
    Pending,
    Pass,
    Regress,
    Changed,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleOutcome {
    pub verdict: RuleVerdict,
    pub structured_match: Option<bool>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoundDetail {
    pub text: String,
    pub violation: Option<Violation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaseDetail {
    pub id: String,
    pub kind: EvalKind,
    pub text: String,
    pub rounds: Vec<RoundDetail>,
    pub contract: ContractOutcome,
    pub quality: Option<JudgeScores>,
    pub rule: Option<RuleOutcome>,
    pub engine_error: bool,
}

#[cfg(test)]
mod tests {
    use gloss_core::task::TaskKind;

    use super::{
        CaseDetail, ContractCheck, ContractOutcome, EvalKind, JudgeScores, LabelVerdict,
        RoundDetail, RuleOutcome, RuleVerdict,
    };
    use std::collections::BTreeMap;

    fn outcome() -> ContractOutcome {
        ContractOutcome {
            json_object: true,
            checks: BTreeMap::from([(ContractCheck::FieldsComplete, true)]),
            degraded: false,
            violations: BTreeMap::new(),
            repairs: 0,
        }
    }

    fn detail() -> CaseDetail {
        CaseDetail {
            id: "task-word-01".to_owned(),
            kind: EvalKind::Task(TaskKind::TranslateWord),
            text: "一".to_owned(),
            rounds: vec![RoundDetail {
                text: "{\"note\":\"义\",\"structured\":{}}".to_owned(),
                violation: None,
            }],
            contract: outcome(),
            quality: Some(JudgeScores {
                accuracy: 90,
                completeness: 80,
                fluency: 85,
                presentation: 75,
                reason: "术语准确但例证偏少".to_owned(),
            }),
            rule: Some(RuleOutcome {
                verdict: RuleVerdict::Pass,
                structured_match: Some(true),
                summary: None,
            }),
            engine_error: false,
        }
    }

    #[test]
    fn case_detail_round_trips_through_serde() {
        let detail = detail();
        let json = serde_json::to_string(&detail).expect("serialize");
        let back: CaseDetail = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, detail);
        assert!(
            json.contains(r#""kind":{"Task":"TranslateWord"}"#),
            "{json}"
        );
        assert!(json.contains(r#""fields_complete""#), "{json}");
        assert!(json.contains(r#""rule":{"verdict":"pass""#), "{json}");
    }

    #[test]
    fn eval_kind_display_names_cover_the_fixed_groups() {
        assert_eq!(EvalKind::Classify.display_name(), "分类");
        assert_eq!(
            EvalKind::Task(TaskKind::TranslateWord).display_name(),
            "词卡"
        );
        assert_eq!(
            EvalKind::Task(TaskKind::TranslateSentence).display_name(),
            "句译"
        );
        assert_eq!(EvalKind::Task(TaskKind::ExplainCode).display_name(), "代码");
        assert_eq!(EvalKind::Task(TaskKind::General).display_name(), "通用");
        assert_eq!(
            EvalKind::Task(TaskKind::TranslateWord).task_kind(),
            Some(TaskKind::TranslateWord)
        );
        assert_eq!(EvalKind::Classify.task_kind(), None);
    }

    #[test]
    fn label_verdict_wire_names_match_the_labels_file() {
        let pass = serde_json::to_string(&LabelVerdict::Pass).expect("serialize");
        let fail = serde_json::to_string(&LabelVerdict::Fail).expect("serialize");
        assert_eq!(pass, r#""pass""#);
        assert_eq!(fail, r#""fail""#);
        let parsed: LabelVerdict = serde_json::from_str(r#""fail""#).expect("deserialize");
        assert_eq!(parsed, LabelVerdict::Fail);
    }
}
