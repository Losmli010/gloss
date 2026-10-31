//! 动作收敛轨：门禁判定的第二个口径。违规样本、按修复动作机械落地的
//! 样本与落地后复判三列成表，断言落地样本同字段零判罚——精度召回（gate）
//! 度量该拦的拦没拦，本轨度量改法指的路走不走得通：动作文盲区或判定器
//! 演进让落地不收敛时当场红，今后新增违约类别须同批补夹具行。数据集是
//! 受控工件：违规列的类别集合与判定器输出全等（未来合法新增类别须显式
//! 改判例），加载期校验形状（标识唯一、类别列非空、落地样本与违规样本
//! 不同文），坏行带行号报错；判定消费与生产同一入口。

use gloss_core::lang_gate::{LangCategory, LangViolation};
use gloss_core::model::Lang;
use gloss_core::task::{OutcomeStructured, TaskKind};
use serde::Deserialize;

use crate::gate::violations_for;

#[derive(Debug, Clone, PartialEq)]
pub struct RemedyCase {
    pub id: String,
    pub kind: TaskKind,
    pub target: Lang,
    pub text: String,
    pub note: String,
    pub repaired: String,
    pub structured: OutcomeStructured,
    pub categories: Vec<LangCategory>,
}

#[derive(Deserialize)]
struct RawRemedyCase {
    id: String,
    kind: TaskKind,
    target: Lang,
    text: String,
    note: String,
    repaired: String,
    structured: OutcomeStructured,
    categories: Vec<LangCategory>,
}

pub fn load_remedy_cases(jsonl: &str) -> Result<Vec<RemedyCase>, String> {
    let mut cases = Vec::new();
    for (index, line) in jsonl.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let raw: RawRemedyCase = serde_json::from_str(trimmed)
            .map_err(|error| format!("remedy case line {}: {error}", index + 1))?;
        if raw.categories.is_empty() {
            return Err(format!(
                "remedy case line {}: categories must not be empty",
                index + 1
            ));
        }
        if raw.repaired.trim().is_empty() || raw.repaired == raw.note {
            return Err(format!(
                "remedy case line {}: the repaired sample must differ from the \
                 violating sample",
                index + 1
            ));
        }
        cases.push(RemedyCase {
            id: raw.id,
            kind: raw.kind,
            target: raw.target,
            text: raw.text,
            note: raw.note,
            repaired: raw.repaired,
            structured: raw.structured,
            categories: raw.categories,
        });
    }
    let mut seen = std::collections::BTreeSet::new();
    for case in &cases {
        if !seen.insert(case.id.as_str()) {
            return Err(format!("duplicate remedy case id {}", case.id));
        }
    }
    Ok(cases)
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RemedyRow {
    pub id: String,
    pub categories: Vec<String>,
    pub categories_match: bool,
    pub repaired_clean: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RemedyReport {
    pub cases: usize,
    pub converged: usize,
    pub rows: Vec<RemedyRow>,
}

impl RemedyReport {
    pub fn render_markdown(&self) -> String {
        let mut out = String::from("\n## 动作收敛夹具（违规样本 → 机械落地 → 复判）\n\n");
        out.push_str("| 条目 | 期望类别 | 违规对账 | 落地复判 |\n| --- | --- | --- | --- |\n");
        for row in &self.rows {
            out.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                row.id,
                row.categories.join(", "),
                if row.categories_match {
                    "一致"
                } else {
                    "偏差"
                },
                if row.repaired_clean {
                    "零判罚"
                } else {
                    "未收敛"
                },
            ));
        }
        out.push_str(&format!("\n收敛条目：{}/{}\n", self.converged, self.cases));
        out
    }
}

pub fn run_remedy(cases: &[RemedyCase]) -> RemedyReport {
    let mut rows = Vec::new();
    for case in cases {
        let expected: std::collections::BTreeSet<_> = case.categories.iter().copied().collect();
        let predicted: std::collections::BTreeSet<_> = violations_for(
            case.kind,
            case.target,
            &case.text,
            &case.note,
            &case.structured,
        )
        .into_iter()
        .map(|violation: LangViolation| violation.category)
        .collect();
        let repaired_clean = violations_for(
            case.kind,
            case.target,
            &case.text,
            &case.repaired,
            &case.structured,
        )
        .is_empty();
        rows.push(RemedyRow {
            id: case.id.clone(),
            categories: expected.iter().map(|c| c.rule_id().to_owned()).collect(),
            categories_match: predicted == expected,
            repaired_clean,
        });
    }
    let converged = rows
        .iter()
        .filter(|row| row.categories_match && row.repaired_clean)
        .count();
    RemedyReport {
        cases: cases.len(),
        converged,
        rows,
    }
}

#[cfg(test)]
mod tests {
    use super::{load_remedy_cases, run_remedy};

    #[test]
    fn remedy_fixtures_land_clean_after_mechanical_repair() {
        let cases =
            load_remedy_cases(crate::assets::LANG_GATE_REMEDY_DATASET).expect("shipped dataset");
        let report = run_remedy(&cases);
        assert_eq!(
            report.converged,
            report.cases,
            "every mechanically repaired sample must re-check clean: {}",
            report.render_markdown()
        );
        assert!(
            !report.rows.is_empty(),
            "the fixture table must not be empty"
        );
        let rendered = report.render_markdown();
        assert!(rendered.contains("落地复判"));
        assert!(rendered.contains("零判罚"));
    }

    #[test]
    fn remedy_loader_rejects_duplicates_empty_categories_and_identical_repair() {
        let good = r#"{"id":"a","kind":"TranslateWord","target":"Zh","text":"gloss","note":"中文word相连。","repaired":"中文单词相连。","structured":{"WordCard":{"phonetic":null,"examples":[]}},"categories":["ForeignRun"]}"#;
        assert_eq!(
            load_remedy_cases(good)
                .expect("a well-formed case loads")
                .len(),
            1
        );

        let duplicated = format!("{good}\n{good}");
        assert!(load_remedy_cases(&duplicated).is_err());

        let empty_categories = good.replace(r#""categories":["ForeignRun"]"#, r#""categories":[]"#);
        assert!(load_remedy_cases(&empty_categories).is_err());

        let identical = good.replace("中文单词相连。", "中文word相连。");
        assert!(load_remedy_cases(&identical).is_err());

        let broken = good.replace("repaired", "repair");
        assert!(load_remedy_cases(&broken).is_err());
    }
}
