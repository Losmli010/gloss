//! 语言体例门禁的离线评测轨：门禁判定数据集逐条过生产判定器（核心库
//! 同名顶层模块的判定入口），按条目级与类别级量精确率与召回率；可选
//! 启用大语言模型（LLM）评委抽样复核（判例回流），复核只产出一致率
//! 数字供调参，不参与任何门禁。
//!
//! 数据集是受控工件：违约样本与放行样本各一批，期望类别写在条目里；
//! 加载期一次校验形状（标识唯一、期望类别与期望一致），坏行带行号
//! 报错。判定消费与生产同一入口，评测数字度量的才是生产行为。

use gloss_core::lang_gate::{self, LangCategory, LangViolation};
use gloss_core::model::Lang;
use gloss_core::ports::{AiEngine, EngineRequest};
use gloss_core::prompt::ChatMessage;
use gloss_core::task::{OutcomeStructured, TaskInput, TaskKind, TaskOutcome};
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq)]
pub struct GateCase {
    pub id: String,
    pub kind: TaskKind,
    pub target: Lang,
    pub text: String,
    pub note: String,
    pub structured: OutcomeStructured,
    pub expect_violate: bool,
    pub categories: Vec<LangCategory>,
}

#[derive(Deserialize)]
struct RawGateCase {
    id: String,
    kind: TaskKind,
    target: Lang,
    text: String,
    note: String,
    structured: OutcomeStructured,
    expect: String,
    #[serde(default)]
    categories: Vec<LangCategory>,
}

pub fn load_gate_cases(jsonl: &str) -> Result<Vec<GateCase>, String> {
    let mut cases = Vec::new();
    for (index, line) in jsonl.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let raw: RawGateCase = serde_json::from_str(trimmed)
            .map_err(|error| format!("gate case line {}: {error}", index + 1))?;
        let expect_violate = match raw.expect.as_str() {
            "pass" => false,
            "violate" => true,
            other => {
                return Err(format!(
                    "gate case line {}: expect must be pass or violate, got {other:?}",
                    index + 1
                ));
            }
        };
        cases.push(GateCase {
            id: raw.id,
            kind: raw.kind,
            target: raw.target,
            text: raw.text,
            note: raw.note,
            structured: raw.structured,
            expect_violate,
            categories: raw.categories,
        });
    }
    let mut seen = std::collections::BTreeSet::new();
    for case in &cases {
        if !seen.insert(case.id.as_str()) {
            return Err(format!("duplicate gate case id {}", case.id));
        }
        if case.expect_violate && case.categories.is_empty() {
            return Err(format!(
                "gate case {} expects a violation but lists no categories",
                case.id
            ));
        }
        if !case.expect_violate && !case.categories.is_empty() {
            return Err(format!(
                "gate case {} expects a pass but lists categories",
                case.id
            ));
        }
    }
    Ok(cases)
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CategoryMetrics {
    pub category: String,
    pub expected: usize,
    pub predicted: usize,
    pub true_positives: usize,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GateReport {
    pub cases: usize,
    pub expected_violations: usize,
    pub predicted_violations: usize,
    pub case_true_positives: usize,
    pub category_metrics: Vec<CategoryMetrics>,
}

impl GateReport {
    pub fn case_precision(&self) -> Option<f64> {
        nonzero(self.predicted_violations)
            .map(|denominator| self.case_true_positives as f64 / denominator)
    }

    pub fn case_recall(&self) -> Option<f64> {
        nonzero(self.expected_violations)
            .map(|denominator| self.case_true_positives as f64 / denominator)
    }

    pub fn category_recall(&self, category: &str) -> Option<f64> {
        let metrics = self
            .category_metrics
            .iter()
            .find(|m| m.category == category)?;
        nonzero(metrics.expected).map(|_| metrics.true_positives as f64 / metrics.expected as f64)
    }

    pub fn render_markdown(&self) -> String {
        let mut out = String::from("# 语言体例门禁离线判定\n\n");
        out.push_str("| 条目 | 期望违约 | 判定违约 | 条目命中 |\n| --- | --- | --- | --- |\n");
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n\n",
            self.cases,
            self.expected_violations,
            self.predicted_violations,
            self.case_true_positives
        ));
        out.push_str(&format!(
            "条目级精确率：{}\n\n条目级召回率：{}\n\n## 类别级对账\n\n",
            ratio(self.case_precision()),
            ratio(self.case_recall())
        ));
        out.push_str("| 类别 | 期望 | 判定 | 命中 |\n| --- | --- | --- | --- |\n");
        for metrics in &self.category_metrics {
            out.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                metrics.category, metrics.expected, metrics.predicted, metrics.true_positives
            ));
        }
        out
    }
}

fn nonzero(value: usize) -> Option<f64> {
    (value > 0).then_some(value as f64)
}

fn ratio(value: Option<f64>) -> String {
    match value {
        Some(value) => format!("{value:.3}"),
        None => "—（分母为零）".to_owned(),
    }
}

pub fn run_gate(cases: &[GateCase]) -> GateReport {
    let mut tally = Tally::default();
    for case in cases {
        let predicted_set: std::collections::BTreeSet<_> = predict(case)
            .into_iter()
            .map(|violation| violation.category)
            .collect();
        if case.expect_violate {
            tally.expected_violations += 1;
            for category in &case.categories {
                *tally.expected(category) += 1;
                if predicted_set.contains(category) {
                    *tally.hit(category) += 1;
                }
            }
        }
        if predicted_set.is_empty() {
            continue;
        }
        tally.predicted_violations += 1;
        if case.expect_violate {
            tally.case_true_positives += 1;
        }
        for category in &predicted_set {
            *tally.predicted(category) += 1;
        }
    }
    GateReport {
        cases: cases.len(),
        expected_violations: tally.expected_violations,
        predicted_violations: tally.predicted_violations,
        case_true_positives: tally.case_true_positives,
        category_metrics: tally.category_metrics(),
    }
}

#[derive(Default)]
struct Tally {
    expected_violations: usize,
    predicted_violations: usize,
    case_true_positives: usize,
    expected_by_category: std::collections::BTreeMap<String, usize>,
    predicted_by_category: std::collections::BTreeMap<String, usize>,
    hits_by_category: std::collections::BTreeMap<String, usize>,
}

impl Tally {
    fn expected(&mut self, category: &LangCategory) -> &mut usize {
        self.expected_by_category
            .entry(category.rule_id().to_owned())
            .or_default()
    }

    fn predicted(&mut self, category: &LangCategory) -> &mut usize {
        self.predicted_by_category
            .entry(category.rule_id().to_owned())
            .or_default()
    }

    fn hit(&mut self, category: &LangCategory) -> &mut usize {
        self.hits_by_category
            .entry(category.rule_id().to_owned())
            .or_default()
    }

    fn category_metrics(&self) -> Vec<CategoryMetrics> {
        let mut metrics: Vec<CategoryMetrics> = self
            .expected_by_category
            .iter()
            .map(|(category, expected)| CategoryMetrics {
                category: category.clone(),
                expected: *expected,
                predicted: self
                    .predicted_by_category
                    .get(category)
                    .copied()
                    .unwrap_or(0),
                true_positives: self.hits_by_category.get(category).copied().unwrap_or(0),
            })
            .collect();
        for (category, predicted) in &self.predicted_by_category {
            if !self.expected_by_category.contains_key(category) {
                metrics.push(CategoryMetrics {
                    category: category.clone(),
                    expected: 0,
                    predicted: *predicted,
                    true_positives: 0,
                });
            }
        }
        metrics
    }
}

fn predict(case: &GateCase) -> Vec<LangViolation> {
    violations_for(
        case.kind,
        case.target,
        &case.text,
        &case.note,
        &case.structured,
    )
}

pub(crate) fn violations_for(
    kind: TaskKind,
    target: Lang,
    text: &str,
    note: &str,
    structured: &OutcomeStructured,
) -> Vec<LangViolation> {
    let input = TaskInput::Text {
        text: text.to_owned(),
    };
    let outcome = TaskOutcome {
        kind,
        note: note.to_owned(),
        structured: structured.clone(),
        degraded: false,
        lang_violations: Vec::new(),
    };
    lang_gate::check(kind, &target, &input, &outcome)
}

pub const GATE_JUDGE_PROMPT: &str = include_str!("../prompts/gate_judge.md");

pub const GATE_JUDGE_MAX_TOKENS: u32 = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateVerdict {
    pub violate: bool,
    pub categories: Vec<LangCategory>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GateJudgeReport {
    pub sampled: usize,
    pub judged: usize,
    pub agreed: usize,
    pub disagreements: Vec<String>,
}

impl GateJudgeReport {
    pub fn render_markdown(&self) -> String {
        let mut out = String::from("\n## 评委抽样复核\n\n");
        out.push_str("| 抽样 | 已判 | 与门禁一致 | 分歧条目 |\n| --- | --- | --- | --- |\n");
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            self.sampled,
            self.judged,
            self.agreed,
            self.disagreements.join("、")
        ));
        out
    }
}

pub fn render_gate_judge(input: &str, output: &str) -> Vec<ChatMessage> {
    let system = GATE_JUDGE_PROMPT
        .replace("{{input}}", input)
        .replace("{{output}}", output);
    vec![ChatMessage::system(system), ChatMessage::user(input)]
}

pub fn parse_gate_reply(reply: &str) -> Option<GateVerdict> {
    let value: serde_json::Value = serde_json::from_str(reply.trim()).ok()?;
    let violate = match value.get("verdict")?.as_str()? {
        "pass" => false,
        "violate" => true,
        _ => return None,
    };
    let categories = value
        .get("categories")?
        .as_array()?
        .iter()
        .map(|category| serde_json::from_value::<LangCategory>(category.clone()).ok())
        .collect::<Option<Vec<_>>>()?;
    Some(GateVerdict {
        violate,
        categories,
    })
}

pub async fn run_gate_judge(
    engine: &dyn AiEngine,
    model: &str,
    cases: &[GateCase],
    limit: usize,
) -> GateJudgeReport {
    let half = limit / 2;
    let mut sampled: Vec<&GateCase> = cases
        .iter()
        .filter(|case| case.expect_violate)
        .take(half)
        .chain(
            cases
                .iter()
                .filter(|case| !case.expect_violate)
                .take(limit - half),
        )
        .collect();
    if sampled.len() < limit {
        let rest: Vec<&GateCase> = cases
            .iter()
            .filter(|case| !sampled.iter().any(|picked| picked.id == case.id))
            .take(limit - sampled.len())
            .collect();
        sampled.extend(rest);
    }
    let mut report = GateJudgeReport {
        sampled: sampled.len(),
        judged: 0,
        agreed: 0,
        disagreements: Vec::new(),
    };
    for case in sampled {
        let output = judge_output(case);
        let messages = render_gate_judge(&case.text, &output);
        let request = EngineRequest {
            messages,
            model: model.to_owned(),
            max_tokens: Some(GATE_JUDGE_MAX_TOKENS),
        };
        let Ok(reply) = collect_reply(engine, request).await else {
            continue;
        };
        let Some(verdict) = parse_gate_reply(&reply) else {
            continue;
        };
        report.judged += 1;
        if verdict.violate == case.expect_violate {
            report.agreed += 1;
        } else {
            report.disagreements.push(case.id.clone());
        }
    }
    report
}

fn judge_output(case: &GateCase) -> String {
    let mut output = case.note.clone();
    for example in case.structured.examples() {
        output.push('\n');
        output.push_str(example);
    }
    output
}

async fn collect_reply(engine: &dyn AiEngine, request: EngineRequest) -> Result<String, String> {
    let mut stream = engine
        .execute(request)
        .await
        .map_err(|error| error.to_string())?;
    let mut reply = String::new();
    while let Some(item) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
        match item {
            Ok(gloss_core::ports::TaskChunk::Delta(delta)) => reply.push_str(&delta),
            Ok(gloss_core::ports::TaskChunk::Done { .. }) => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(reply)
}

#[cfg(test)]
mod tests {
    use gloss_core::lang_gate::LangCategory;
    use gloss_core::model::Lang;
    use gloss_core::task::{OutcomeStructured, TaskKind};

    use super::{GateCase, load_gate_cases, parse_gate_reply, run_gate};

    fn case(id: &str, expect_violate: bool, categories: &[LangCategory], note: &str) -> GateCase {
        GateCase {
            id: id.to_owned(),
            kind: TaskKind::TranslateWord,
            target: Lang::Zh,
            text: "gloss".to_owned(),
            note: note.to_owned(),
            structured: OutcomeStructured::WordCard {
                phonetic: None,
                examples: Vec::new(),
            },
            expect_violate,
            categories: categories.to_vec(),
        }
    }

    #[test]
    fn gate_reply_parsing_accepts_json_verdict_only() {
        use super::GateVerdict;

        let verdict =
            parse_gate_reply(r#"{"verdict":"violate","categories":["ForeignRun","MissingSpace"]}"#)
                .expect("a well-formed verdict parses");
        assert!(verdict.violate);
        assert_eq!(verdict.categories.len(), 2);

        assert_eq!(
            parse_gate_reply(r#"{"verdict":"pass","categories":[]}"#),
            Some(GateVerdict {
                violate: false,
                categories: Vec::new(),
            })
        );
        assert_eq!(parse_gate_reply("没有任何分数"), None);
        assert_eq!(
            parse_gate_reply(r#"{"verdict":"maybe","categories":[]}"#),
            None
        );
        assert_eq!(
            parse_gate_reply(r#"{"verdict":"pass","categories":["Nonsense"]}"#),
            None
        );
        assert_eq!(parse_gate_reply(""), None);
    }

    #[test]
    fn gate_metrics_compute_precision_and_recall() {
        use LangCategory::*;

        let cases = [
            case("v-hit", true, &[ForeignRun], "这个 english 混进来了。"),
            case(
                "v-miss",
                true,
                &[TraditionalChar],
                "引文“這是繁體”按体例豁免。",
            ),
            case("p-clean", false, &[], "干净的一段话。"),
        ];
        let report = run_gate(&cases);
        assert_eq!(report.cases, 3);
        assert_eq!(report.expected_violations, 2);
        assert_eq!(report.predicted_violations, 1);
        assert_eq!(report.case_true_positives, 1);
        assert_eq!(report.case_precision(), Some(1.0));
        assert_eq!(report.case_recall(), Some(0.5));
        let traditional = report
            .category_metrics
            .iter()
            .find(|metrics| metrics.category == "TraditionalChar")
            .expect("the missed category is tallied");
        assert_eq!(
            (traditional.expected, traditional.true_positives),
            (1, 0),
            "the exempted quote char counts as expected but not hit"
        );
    }

    #[test]
    fn gate_loader_rejects_bad_expect_duplicate_ids_and_category_mismatch() {
        let good = r#"{"id":"a","kind":"TranslateWord","target":"Zh","text":"gloss","note":"义释。","structured":{"WordCard":{"phonetic":null,"examples":[]}},"expect":"pass","categories":[]}"#;
        assert_eq!(
            load_gate_cases(good)
                .expect("a well-formed case loads")
                .len(),
            1
        );

        let bad_expect = good.replace(r#""expect":"pass""#, r#""expect":"maybe""#);
        assert!(load_gate_cases(&bad_expect).is_err());

        let duplicated = format!("{good}\n{good}");
        assert!(load_gate_cases(&duplicated).is_err());

        let violate_without_categories = good.replace(
            r#""expect":"pass","categories":[]"#,
            r#""expect":"violate","categories":[]"#,
        );
        assert!(load_gate_cases(&violate_without_categories).is_err());

        let pass_with_categories =
            good.replace(r#""categories":[]"#, r#""categories":["ForeignRun"]"#);
        assert!(load_gate_cases(&pass_with_categories).is_err());

        let broken = good.replace("structured", "structure");
        assert!(load_gate_cases(&broken).is_err());
    }

    #[test]
    fn gate_dataset_agrees_with_the_gate_offline() {
        let cases = load_gate_cases(super::super::assets::LANG_GATE_DATASET)
            .expect("the shipped dataset must load");
        let report = run_gate(&cases);
        assert_eq!(
            report.case_precision(),
            Some(1.0),
            "every predicted violation must match the dataset: {}",
            report.render_markdown()
        );
        assert_eq!(
            report.case_recall(),
            Some(1.0),
            "every expected violation must be caught: {}",
            report.render_markdown()
        );
        for metrics in &report.category_metrics {
            assert_eq!(
                metrics.expected, metrics.true_positives,
                "category {} loses hits",
                metrics.category
            );
        }
        let punctuation = report
            .category_metrics
            .iter()
            .find(|metrics| metrics.category == "PunctuationMismatch")
            .expect("the punctuation category is tallied");
        assert_eq!(
            (punctuation.expected, punctuation.predicted),
            (1, 3),
            "known over-report: punctuation is flagged inside entries already \
             failing other categories; moving the count needs an explicit call",
        );
    }
}
