//! 静态用例集：精选集（curated.jsonl）的强类型条目与加载校验。用例集
//! 编译期嵌入为只读资产，加载时一次校验形状（标识唯一非空、类型合法、
//! 参考答案按子类结构、目标语言为已知语名），坏行带行号报错——评测
//! 资产是受控工件，坏行不该被静默跳过。执行器与测试都拿到被验证过的
//! 数据。

use std::collections::BTreeMap;

use gloss_core::model::Lang;
use gloss_core::schema;
use gloss_core::task::{OutcomeStructured, TaskKind};
use serde::Deserialize;

use crate::detail::EvalKind;

const EXPECTED_KINDS: [TaskKind; 4] = [
    TaskKind::TranslateWord,
    TaskKind::TranslateSentence,
    TaskKind::ExplainCode,
    TaskKind::General,
];

#[derive(Debug, Clone, PartialEq)]
pub struct EvalCase {
    pub id: String,
    pub kind: EvalKind,
    pub text: String,
    pub target: Lang,
    pub reference: Reference,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Reference {
    Classify {
        expected: TaskKind,
    },
    Task {
        note: String,
        structured: OutcomeStructured,
    },
}

impl Reference {
    pub fn reply_json(&self) -> String {
        match self {
            Self::Classify { expected } => {
                serde_json::json!({ "kind": classify_category(*expected) }).to_string()
            }
            Self::Task { note, structured } => serde_json::json!({
                "note": note,
                "structured": structured_value(structured),
            })
            .to_string(),
        }
    }
}

fn classify_category(expected: TaskKind) -> &'static str {
    match expected {
        TaskKind::TranslateWord => "TranslateWord",
        TaskKind::TranslateSentence => "TranslateSentence",
        TaskKind::ExplainCode => "ExplainCode",
        TaskKind::General | TaskKind::ImageExplain => "Fallback",
    }
}

fn structured_value(structured: &OutcomeStructured) -> serde_json::Value {
    match structured {
        OutcomeStructured::WordCard { phonetic, examples } => {
            serde_json::json!({ "phonetic": phonetic, "examples": examples })
        }
        OutcomeStructured::Plain { examples } => {
            serde_json::json!({ "examples": examples })
        }
        OutcomeStructured::Code {
            examples,
            code_language,
        } => serde_json::json!({
            "examples": examples,
            "code_language": code_language,
        }),
    }
}

#[derive(Deserialize)]
struct RawCase {
    id: String,
    kind: String,
    text: String,
    target: String,
    reference: serde_json::Value,
}

pub fn load_curated() -> Result<Vec<EvalCase>, String> {
    parse_curated(crate::assets::CURATED_DATASET)
}

pub(crate) fn parse_curated(jsonl: &str) -> Result<Vec<EvalCase>, String> {
    let mut cases = Vec::new();
    let mut id_lines: BTreeMap<String, usize> = BTreeMap::new();
    for (index, line) in jsonl.lines().enumerate() {
        let line_no = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let raw: RawCase =
            serde_json::from_str(trimmed).map_err(|error| format!("line {line_no}: {error}"))?;
        if raw.id.trim().is_empty() {
            return Err(format!("line {line_no}: case id must be non-empty"));
        }
        if let Some(first) = id_lines.insert(raw.id.clone(), line_no) {
            return Err(format!(
                "line {line_no}: duplicate case id {:?} (first seen at line {first})",
                raw.id
            ));
        }
        let case = build_case(raw).map_err(|error| format!("line {line_no}: {error}"))?;
        cases.push(case);
    }
    if cases.is_empty() {
        return Err("curated dataset is empty".into());
    }
    Ok(cases)
}

fn build_case(raw: RawCase) -> Result<EvalCase, String> {
    let kind = parse_kind(&raw.kind)?;
    let target = parse_target(&raw.target)?;
    let reference = parse_reference(&kind, &raw.reference)?;
    Ok(EvalCase {
        id: raw.id,
        kind,
        text: raw.text,
        target,
        reference,
    })
}

fn parse_kind(kind: &str) -> Result<EvalKind, String> {
    match kind {
        "Classify" => Ok(EvalKind::Classify),
        "TranslateWord" => Ok(EvalKind::Task(TaskKind::TranslateWord)),
        "TranslateSentence" => Ok(EvalKind::Task(TaskKind::TranslateSentence)),
        "ExplainCode" => Ok(EvalKind::Task(TaskKind::ExplainCode)),
        "General" => Ok(EvalKind::Task(TaskKind::General)),
        other => Err(format!(
            "unknown kind {other:?} (curated kinds: Classify, TranslateWord, \
             TranslateSentence, ExplainCode, General)"
        )),
    }
}

fn parse_target(target: &str) -> Result<Lang, String> {
    match target {
        "中文" => Ok(Lang::Zh),
        other => Err(format!(
            "unknown target language {other:?} (known names: 中文)"
        )),
    }
}

fn parse_reference(kind: &EvalKind, value: &serde_json::Value) -> Result<Reference, String> {
    match kind {
        EvalKind::Classify => {
            let expected: TaskKind = serde_json::from_value(
                value
                    .get("expected")
                    .cloned()
                    .ok_or_else(|| "classify reference lacks \"expected\"".to_owned())?,
            )
            .map_err(|error| format!("classify reference: {error}"))?;
            if !EXPECTED_KINDS.contains(&expected) {
                return Err(format!(
                    "classify expected kind {expected:?} is not one of the four text tasks"
                ));
            }
            Ok(Reference::Classify { expected })
        }
        EvalKind::Task(task) => {
            let raw = serde_json::to_string(value).map_err(|error| error.to_string())?;
            let (note, structured) = schema::validate_task(*task, &raw).map_err(|violation| {
                format!("task reference fails the production validator: {violation}")
            })?;
            require_reference_fields(*task, value)?;
            Ok(Reference::Task { note, structured })
        }
    }
}

fn require_reference_fields(task: TaskKind, value: &serde_json::Value) -> Result<(), String> {
    let required: &[&str] = match task {
        TaskKind::TranslateWord => &["phonetic", "examples"],
        TaskKind::ExplainCode => &["code_language", "examples"],
        TaskKind::TranslateSentence | TaskKind::ImageExplain | TaskKind::General => &["examples"],
    };
    let structured = value
        .get("structured")
        .ok_or_else(|| "task reference lacks \"structured\"".to_owned())?;
    for key in required {
        if structured.get(*key).is_none() {
            return Err(format!(
                "task reference for {task:?} lacks structured field {key:?}"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use gloss_core::task::TaskKind;

    use super::{Reference, load_curated, parse_curated};
    use crate::contract::settle_case;
    use crate::detail::{ContractCheck, EvalKind};

    fn case(id: &str, kind: EvalKind, text: &str, target: &str, reference: &str) -> String {
        format!(
            r#"{{"id":"{id}","kind":"{kind_word}","text":"{text}","target":"{target}","reference":{reference}}}"#,
            kind_word = match &kind {
                EvalKind::Classify => "Classify".to_owned(),
                EvalKind::Task(task) => format!("{task:?}"),
            },
        )
    }

    fn word_reference() -> &'static str {
        r#"{"note":"义","structured":{"phonetic":"yī","examples":["例"]}}"#
    }

    fn classify_reference(expected: &str) -> String {
        format!(r#"{{"expected":"{expected}"}}"#)
    }

    #[test]
    fn curated_loads_with_the_finalized_distribution() {
        let cases = load_curated().expect("the finalized dataset must load");
        let mut counts: Vec<(EvalKind, usize)> = Vec::new();
        for kind in [
            EvalKind::Classify,
            EvalKind::Task(TaskKind::TranslateWord),
            EvalKind::Task(TaskKind::TranslateSentence),
            EvalKind::Task(TaskKind::ExplainCode),
            EvalKind::Task(TaskKind::General),
        ] {
            let count = cases.iter().filter(|case| case.kind == kind).count();
            counts.push((kind, count));
        }
        assert_eq!(
            counts,
            vec![
                (EvalKind::Classify, 22),
                (EvalKind::Task(TaskKind::TranslateWord), 10),
                (EvalKind::Task(TaskKind::TranslateSentence), 6),
                (EvalKind::Task(TaskKind::ExplainCode), 3),
                (EvalKind::Task(TaskKind::General), 3),
            ],
            "the finalized composition is 22 classify + 10 word + 6 sentence + \
             3 code + 3 general; adding or removing entries is a reviewed dataset \
             change that updates this guard"
        );
        assert_eq!(cases.len(), 44);
        assert!(
            cases
                .iter()
                .all(|case| case.target == gloss_core::model::Lang::Zh),
            "the curated target is always Chinese"
        );
    }

    #[test]
    fn every_embedded_reference_settles_green_against_its_own_case() {
        let cases = load_curated().expect("the finalized dataset must load");
        for case in &cases {
            let reply = case.reference.reply_json();
            let (outcome, _) = settle_case(case, &[reply]);
            assert!(
                !outcome.degraded,
                "{}: the reference must be a valid product",
                case.id
            );
            let key = match case.kind {
                EvalKind::Classify => ContractCheck::KindMatch,
                EvalKind::Task(_) => ContractCheck::FieldsComplete,
            };
            assert_eq!(
                outcome.checks.get(&key),
                Some(&true),
                "{}: the reference must pass its own check",
                case.id
            );
        }
    }

    #[test]
    fn reference_renders_as_the_product_wire_shape() {
        let reference = Reference::Task {
            note: "义".to_owned(),
            structured: gloss_core::task::OutcomeStructured::WordCard {
                phonetic: Some("yī".to_owned()),
                examples: vec!["例".to_owned()],
            },
        };
        let raw = reference.reply_json();
        let (note, _) = gloss_core::schema::validate_task(TaskKind::TranslateWord, &raw)
            .expect("the rendered reference must parse as a product");
        assert_eq!(note, "义");
        assert!(raw.contains(r#""phonetic":"yī""#), "{raw}");

        let classify = Reference::Classify {
            expected: TaskKind::ExplainCode,
        };
        assert_eq!(
            classify.reply_json(),
            r#"{"kind":"ExplainCode"}"#,
            "the classify reference rides the bare category reply shape"
        );
    }

    #[test]
    fn reference_rejects_wrong_subtype_shapes_with_line_numbers() {
        let classify_row = case("cls-01", EvalKind::Classify, "一", "中文", word_reference());
        let error = parse_curated(&classify_row).expect_err("classify takes no task reference");
        assert!(error.starts_with("line 1: "), "{error}");

        let task_row = case(
            "task-01",
            EvalKind::Task(TaskKind::TranslateWord),
            "一",
            "中文",
            &classify_reference("TranslateWord"),
        );
        let error = parse_curated(&task_row).expect_err("task takes no classify reference");
        assert!(error.starts_with("line 1: "), "{error}");

        let empty_note = case(
            "task-02",
            EvalKind::Task(TaskKind::TranslateWord),
            "一",
            "中文",
            r#"{"note":"","structured":{"phonetic":"yī","examples":[]}}"#,
        );
        let error = parse_curated(&empty_note).expect_err("an empty note degrades");
        assert!(error.contains("non-empty note"), "{error}");

        let missing_phonetic = case(
            "task-03",
            EvalKind::Task(TaskKind::TranslateWord),
            "一",
            "中文",
            r#"{"note":"义","structured":{"examples":["例"]}}"#,
        );
        let error =
            parse_curated(&missing_phonetic).expect_err("word references carry a phonetic key");
        assert!(error.contains("phonetic"), "{error}");

        let null_phonetic = case(
            "task-04",
            EvalKind::Task(TaskKind::TranslateWord),
            "一",
            "中文",
            r#"{"note":"义","structured":{"phonetic":null,"examples":[]}}"#,
        );
        let parsed = parse_curated(&null_phonetic).expect("null phonetic counts as present");
        assert!(matches!(parsed[0].reference, Reference::Task { .. }));
    }

    #[test]
    fn loader_rejects_unknown_kinds_targets_and_expected() {
        let image_row = case(
            "cls-01",
            EvalKind::Classify,
            "一",
            "中文",
            &classify_reference("TranslateWord"),
        )
        .replace(r#""kind":"Classify""#, r#""kind":"ImageExplain""#);
        let error = parse_curated(&image_row).expect_err("image cases stay out of the curated set");
        assert!(error.contains("unknown kind"), "{error}");

        let row = case(
            "cls-01",
            EvalKind::Classify,
            "一",
            "英文",
            &classify_reference("TranslateWord"),
        );
        let error = parse_curated(&row).expect_err("only Chinese is a known target");
        assert!(error.contains("unknown target language"), "{error}");

        let row = case(
            "cls-01",
            EvalKind::Classify,
            "一",
            "中文",
            &classify_reference("ImageExplain"),
        );
        let error =
            parse_curated(&row).expect_err("expected kinds stay within the four text tasks");
        assert!(error.contains("not one of the four text tasks"), "{error}");
    }

    #[test]
    fn loader_rejects_empty_or_duplicate_ids_and_bad_lines() {
        let empty_id = case(
            "",
            EvalKind::Classify,
            "一",
            "中文",
            &classify_reference("TranslateWord"),
        );
        let error = parse_curated(&empty_id).expect_err("ids are non-empty");
        assert!(error.contains("non-empty"), "{error}");

        let good = case(
            "cls-01",
            EvalKind::Classify,
            "一",
            "中文",
            &classify_reference("TranslateWord"),
        );
        let duplicate = format!("{good}\n{good}");
        let error = parse_curated(&duplicate).expect_err("ids are unique");
        assert!(error.contains("line 2: duplicate"), "{error}");

        let broken = format!("{{\"id\": \"cls-01\"\n{good}");
        let error = parse_curated(&broken).expect_err("bad lines carry their number");
        assert!(error.starts_with("line 1: "), "{error}");

        let blank = format!("\n{good}\n");
        let parsed = parse_curated(&blank).expect("blank lines are skipped");
        assert_eq!(parsed.len(), 1);

        let error = parse_curated("").expect_err("an empty dataset is a hard error");
        assert!(error.contains("empty"), "{error}");
    }
}
