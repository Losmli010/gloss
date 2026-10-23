//! 数据集与夹具的加载、结构校验：`JSON Lines` → 强类型条目。
//!
//! 校验在此一次做全（id 唯一、expected/reference 形状合法），runner 与
//! 测试都拿到被验证过的数据；解析失败带行号报错，评测资产是受控工件，
//! 坏行不该被静默跳过。

use gloss_core::task::TaskKind;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifyCase {
    pub id: String,
    pub text: String,
    pub expected: TaskKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaskCase {
    pub id: String,
    pub kind: TaskKind,
    pub text: String,
    pub reference: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Fixture {
    pub id: String,
    pub rounds: Vec<Vec<String>>,
}

#[derive(Deserialize)]
struct RawClassify {
    id: String,
    text: String,
    expected: TaskKind,
}

#[derive(Deserialize)]
struct RawTask {
    id: String,
    kind: TaskKind,
    text: String,
    reference: serde_json::Value,
}

#[derive(Deserialize)]
struct RawFixture {
    id: String,
    rounds: Vec<Vec<String>>,
}

pub fn load_classify(jsonl: &str) -> Result<Vec<ClassifyCase>, String> {
    let raw: Vec<RawClassify> = parse_jsonl(jsonl)?;
    let cases: Vec<ClassifyCase> = raw
        .into_iter()
        .map(|raw| ClassifyCase {
            id: raw.id,
            text: raw.text,
            expected: raw.expected,
        })
        .collect();
    require_unique_ids(cases.iter().map(|case| &case.id))?;
    Ok(cases)
}

pub fn load_task(jsonl: &str) -> Result<Vec<TaskCase>, String> {
    let raw: Vec<RawTask> = parse_jsonl(jsonl)?;
    let cases: Vec<TaskCase> = raw
        .into_iter()
        .map(|raw| TaskCase {
            id: raw.id,
            kind: raw.kind,
            text: raw.text,
            reference: raw.reference,
        })
        .collect();
    require_unique_ids(cases.iter().map(|case| &case.id))?;
    for case in &cases {
        for key in required_fields(case.kind) {
            if case.reference.get(key).is_none() {
                return Err(format!(
                    "task case {}: reference for {kind:?} lacks required field {key:?}",
                    case.id,
                    kind = case.kind
                ));
            }
        }
    }
    Ok(cases)
}

pub fn load_fixtures(jsonl: &str) -> Result<Vec<Fixture>, String> {
    let raw: Vec<RawFixture> = parse_jsonl(jsonl)?;
    let fixtures: Vec<Fixture> = raw
        .into_iter()
        .map(|raw| Fixture {
            id: raw.id,
            rounds: raw.rounds,
        })
        .collect();
    require_unique_ids(fixtures.iter().map(|fixture| &fixture.id))?;
    for fixture in &fixtures {
        if fixture.rounds.is_empty() {
            return Err(format!("fixture {}: zero-round trajectory", fixture.id));
        }
    }
    Ok(fixtures)
}

pub fn required_fields(_kind: TaskKind) -> &'static [&'static str] {
    &["examples"]
}

fn parse_jsonl<'a, T: Deserialize<'a>>(jsonl: &'a str) -> Result<Vec<T>, String> {
    let mut out = Vec::new();
    for (index, line) in jsonl.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        serde_json::from_str::<T>(trimmed)
            .map(|item| out.push(item))
            .map_err(|err| format!("line {}: {err}", index + 1))?;
    }
    if out.is_empty() {
        return Err("dataset is empty".into());
    }
    Ok(out)
}

fn require_unique_ids<'a>(ids: impl Iterator<Item = &'a String>) -> Result<(), String> {
    let mut seen = std::collections::BTreeSet::new();
    for id in ids {
        if !seen.insert(id.as_str()) {
            return Err(format!("duplicate case id {id:?}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{load_classify, load_fixtures, load_task, required_fields};
    use gloss_core::task::TaskKind;

    #[test]
    fn classify_dataset_loads_with_unique_ids() {
        let cases = load_classify(crate::assets::CLASSIFY_DATASET).expect("dataset must load");
        assert!(cases.len() >= 60, "the plan pins >=60 classify cases");
        assert!(
            cases.iter().all(|case| matches!(
                case.expected,
                TaskKind::TranslateWord
                    | TaskKind::TranslateSentence
                    | TaskKind::ExplainCode
                    | TaskKind::General
            )),
            "the classify dataset covers the text task kinds plus the general \
             landing of the Fallback category"
        );
    }

    #[test]
    fn task_datasets_load_and_match_required_fields() {
        for (raw, kind) in [
            (crate::assets::TASK_WORD_DATASET, TaskKind::TranslateWord),
            (
                crate::assets::TASK_SENTENCE_DATASET,
                TaskKind::TranslateSentence,
            ),
            (crate::assets::TASK_CODE_DATASET, TaskKind::ExplainCode),
        ] {
            let cases = load_task(raw).expect("task dataset must load");
            assert!(!cases.is_empty());
            for case in &cases {
                assert_eq!(case.kind, kind, "dataset rows must match their file kind");
            }
        }
    }

    #[test]
    fn fixtures_align_with_dataset_ids() {
        let classify_ids: std::collections::BTreeSet<String> =
            load_classify(crate::assets::CLASSIFY_DATASET)
                .expect("dataset")
                .into_iter()
                .map(|case| case.id)
                .collect();
        let classify_fixtures = load_fixtures(crate::assets::CLASSIFY_FIXTURES).expect("fixtures");
        assert!(!classify_fixtures.is_empty());
        for fixture in &classify_fixtures {
            assert!(
                classify_ids.contains(&fixture.id),
                "{}: fixture id must exist in the classify dataset",
                fixture.id
            );
        }

        let task_ids: std::collections::BTreeSet<String> = [
            crate::assets::TASK_WORD_DATASET,
            crate::assets::TASK_SENTENCE_DATASET,
            crate::assets::TASK_CODE_DATASET,
        ]
        .iter()
        .flat_map(|raw| load_task(raw).expect("task dataset"))
        .map(|case| case.id)
        .collect();
        let task_fixtures = load_fixtures(crate::assets::TASK_FIXTURES).expect("task fixtures");
        assert!(!task_fixtures.is_empty());
        for fixture in &task_fixtures {
            assert!(
                task_ids.contains(&fixture.id),
                "{}: fixture id must exist in a task dataset",
                fixture.id
            );
        }
    }

    #[test]
    fn zero_round_fixtures_are_rejected() {
        assert!(
            load_fixtures("{\"id\":\"a\",\"rounds\":[]}\n").is_err(),
            "an empty trajectory is a malformed fixture, not an empty reply"
        );
        assert!(
            load_fixtures("{\"id\":\"a\",\"rounds\":[[]]}\n").is_ok(),
            "a round with no deltas is an empty reply, which is a legal round"
        );
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let jsonl = "{\"id\":\"a\",\"text\":\"x\",\"expected\":\"TranslateWord\"}\n{\"id\":\"a\",\"text\":\"y\",\"expected\":\"TranslateSentence\"}\n";
        assert!(load_classify(jsonl).is_err());
    }

    #[test]
    fn bad_reference_is_rejected() {
        let jsonl = "{\"id\":\"a\",\"kind\":\"TranslateWord\",\"text\":\"x\",\"reference\":{\"title\":null}}\n";
        assert!(
            load_task(jsonl).is_err(),
            "word reference needs word+senses"
        );
    }

    #[test]
    fn required_fields_follow_the_contract() {
        for kind in [
            TaskKind::TranslateWord,
            TaskKind::TranslateSentence,
            TaskKind::ExplainCode,
            TaskKind::ImageExplain,
        ] {
            assert_eq!(
                required_fields(kind),
                &["examples"],
                "{kind:?}: examples is the only ever-present structured key"
            );
        }
    }
}
