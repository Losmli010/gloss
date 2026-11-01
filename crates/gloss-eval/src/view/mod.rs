//! 检视页渲染：零外部资源单文件（HTML）——顶栏、指标总览、历史曲线、
//! 测试用例四区，中文文案，原生（details）与（summary）加约四十行内
//! 联脚本。本模块只渲染不重算：指标总览读报告分组聚合，历史曲线经曲
//! 线渲染唯一出口（trend）嵌入，测试用例区读结算明细、用例定义、基线
//! 与标注四源；外来文本一律转义，导出标注与标注落库共用同一结构。

pub mod cases;
pub mod overview;
pub mod style;

use crate::asset::{GoldenMap, LabelMap};
use crate::dataset::EvalCase;
use crate::detail::CaseDetail;
use crate::report::EvalReport;
use crate::trend;

pub struct CasePair<'a> {
    pub case: &'a EvalCase,
    pub detail: &'a CaseDetail,
}

pub struct ViewInput<'a> {
    pub report: &'a EvalReport,
    pub cases: &'a [CasePair<'a>],
    pub history: &'a [EvalReport],
    pub labels: &'a LabelMap,
    pub golden: &'a GoldenMap,
}

pub fn render_html(input: &ViewInput<'_>) -> String {
    let mut out = String::from(
        "<!doctype html>\n<html lang=\"zh-CN\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>Gloss 提示词评估工作台</title>\n<style>\n",
    );
    out.push_str(&style::stylesheet());
    out.push_str("</style>\n</head>\n<body>\n\n<header>\n  <h1>Gloss 提示词评估工作台</h1>\n</header>\n\n<h2>指标总览</h2>\n");
    out.push_str(&overview::render(input));
    out.push_str("\n<h2>历史曲线</h2>\n<section class=\"dash\">\n");
    out.push_str(&trend::render_svg(input.history));
    out.push_str("</section>\n\n");
    out.push_str(&cases::render(input));
    out.push_str("\n<script>\n");
    out.push_str(SCRIPT);
    out.push_str("</script>\n</body>\n</html>\n");
    out
}

pub(crate) fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub(crate) fn fmt(value: f64) -> String {
    let hundredth = (value * 100.0).round() as i64;
    if hundredth % 100 == 0 {
        format!("{}", hundredth / 100)
    } else if hundredth % 10 == 0 {
        format!("{value:.1}")
    } else {
        format!("{value:.2}")
    }
}

const SCRIPT: &str = r#"let group='all',contract='all',reg='all';
const box=document.getElementById('cases');
function apply(){
  let visible=0;
  box.querySelectorAll('details.case').forEach(el=>{
    const g=group==='all'||el.dataset.group===group;
    const c=contract==='all'||el.dataset.contract===contract;
    const r=reg==='all'||el.dataset.regression===reg;
    const show=g&&c&&r; el.style.display=show?'':'none'; if(show)visible++;
  });
  document.getElementById('empty').hidden=visible>0;
}
function pick(cls,v){document.querySelectorAll('button.'+cls).forEach(b=>b.classList.toggle('on',b.dataset.v===v));}
document.getElementById('filters').addEventListener('click',e=>{
  const b=e.target.closest('button'); if(!b)return;
  if(b.classList.contains('group')){group=b.dataset.v;pick('group',group);}
  else if(b.classList.contains('contract')){contract=b.dataset.v;pick('contract',contract);}
  else if(b.classList.contains('reg')){reg=b.dataset.v;pick('reg',reg);}
  apply();
});
box.addEventListener('click',e=>{
  const m=e.target.closest('.markbtn'); if(!m)return;
  m.parentElement.querySelectorAll('.markbtn').forEach(x=>x.classList.remove('on'));
  m.classList.add('on');
  document.getElementById('exportCount').textContent=box.querySelectorAll('.markbtn.on').length;
});
document.getElementById('exportBtn').addEventListener('click',()=>{
  const labels=[];
  box.querySelectorAll('details.case').forEach(el=>{
    const on=el.querySelector('.markbtn.on'); if(!on)return;
    labels.push({id:el.dataset.id,verdict:on.dataset.v,note:el.querySelector('input').value.trim()});
  });
  const blob=new Blob([JSON.stringify({labels},null,2)],{type:'application/json'});
  const a=document.createElement('a');
  a.href=URL.createObjectURL(blob); a.download='labels.json'; a.click();
  URL.revokeObjectURL(a.href);
});
"#;

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::BTreeMap;

    use gloss_core::schema::Violation;
    use gloss_core::task::{OutcomeStructured, TaskKind};

    use super::{CasePair, ViewInput};
    use crate::asset::{GoldenRow, LabelRow};
    use crate::dataset::{EvalCase, Reference};
    use crate::detail::{
        CaseDetail, ContractCheck, ContractOutcome, ContractState, EvalKind, JudgeScores,
        LabelVerdict, RoundDetail, RuleOutcome, RuleVerdict,
    };
    use crate::report::assemble;

    pub fn outcome(state: ContractState) -> ContractOutcome {
        ContractOutcome {
            json_object: state != ContractState::Degraded,
            checks: BTreeMap::from([(
                ContractCheck::FieldsComplete,
                state == ContractState::Valid,
            )]),
            degraded: state == ContractState::Degraded,
            violations: BTreeMap::from([(Violation::NotJson, 1)]),
            repairs: 0,
        }
    }

    pub fn case(id: &str, kind: &EvalKind) -> EvalCase {
        EvalCase {
            id: id.to_owned(),
            kind: kind.clone(),
            text: format!("text-of-{id}"),
            target: gloss_core::model::Lang::Zh,
            reference: match kind {
                EvalKind::Classify => Reference::Classify {
                    expected: TaskKind::TranslateWord,
                },
                _ => Reference::Task {
                    note: "义".to_owned(),
                    structured: OutcomeStructured::Plain { examples: vec![] },
                },
            },
        }
    }

    pub fn detail(id: &str, kind: &EvalKind) -> CaseDetail {
        CaseDetail {
            id: id.to_owned(),
            kind: kind.clone(),
            text: format!("text-of-{id}"),
            rounds: vec![RoundDetail {
                text: "{\"note\":\"义\"}".to_owned(),
                violation: None,
            }],
            contract: outcome(ContractState::Valid),
            quality: None,
            rule: Some(RuleOutcome {
                verdict: RuleVerdict::Pending,
                structured_match: None,
                summary: None,
            }),
            engine_error: false,
        }
    }

    pub fn pair<'a>(case: &'a EvalCase, detail: &'a CaseDetail) -> CasePair<'a> {
        CasePair { case, detail }
    }

    pub fn scored(detail: &mut CaseDetail, four: [u8; 4]) {
        detail.quality = Some(JudgeScores {
            accuracy: four[0],
            completeness: four[1],
            fluency: four[2],
            presentation: four[3],
            reason: "锚点理由".to_owned(),
        });
    }

    pub fn rule(verdict: RuleVerdict, summary: Option<&str>) -> RuleOutcome {
        RuleOutcome {
            verdict,
            structured_match: None,
            summary: summary.map(str::to_owned),
        }
    }

    pub fn labeled(id: &str, verdict: LabelVerdict, note: Option<&str>) -> (String, LabelRow) {
        (
            id.to_owned(),
            LabelRow {
                id: id.to_owned(),
                verdict,
                note: note.map(str::to_owned),
            },
        )
    }

    pub fn golden_row(id: &str) -> (String, GoldenRow) {
        (
            id.to_owned(),
            GoldenRow {
                id: id.to_owned(),
                rounds: vec![vec!["{\"note\":\"义\"}".to_owned()]],
                accepted_at: "2026-11-01T10:00:00Z".to_owned(),
            },
        )
    }

    pub fn input<'a>(
        report: &'a crate::report::EvalReport,
        pairs: &'a [CasePair<'a>],
        labels: &'a BTreeMap<String, LabelRow>,
        golden: &'a BTreeMap<String, GoldenRow>,
    ) -> ViewInput<'a> {
        ViewInput {
            report,
            cases: pairs,
            history: std::slice::from_ref(report),
            labels,
            golden,
        }
    }

    #[test]
    fn page_assembles_the_four_regions() {
        let kind = EvalKind::Task(TaskKind::TranslateWord);
        let eval_case = case("task-word-01", &kind);
        let detail = detail("task-word-01", &kind);
        let pairs = vec![pair(&eval_case, &detail)];
        let report = assemble(
            std::slice::from_ref(&detail),
            &crate::asset::GoldenMap::new(),
            &crate::asset::LabelMap::new(),
            None,
        );
        let page = super::render_html(&input(
            &report,
            &pairs,
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeMap::new(),
        ));
        assert!(page.starts_with("<!doctype html>"));
        assert!(page.contains("<h1>Gloss 提示词评估工作台</h1>"));
        assert!(page.contains("<h2>指标总览</h2>"));
        assert!(page.contains("<h2>历史曲线</h2>") && page.contains("<svg"));
        assert!(page.contains("<h2>测试用例</h2>"));
        assert!(page.contains("id=\"exportBtn\"") && page.contains("id=\"filters\""));
        assert!(
            !page.contains("<link")
                && !page.contains("@import")
                && !page.contains("url(")
                && !page.contains(" src=\"http"),
            "zero external resources beyond the svg namespace: {page}"
        );
    }

    #[test]
    fn foreign_text_is_escaped_everywhere() {
        let kind = EvalKind::Task(TaskKind::TranslateWord);
        let mut hostile = detail("task-word-02", &kind);
        hostile.text = "<script>alert(1)</script>".to_owned();
        hostile.rounds[0].text = "</details><img src=x onerror=alert(2)>".to_owned();
        let eval_case = case("task-word-02", &kind);
        let pairs = vec![pair(&eval_case, &hostile)];
        let report = assemble(
            &[hostile.clone()],
            &crate::asset::GoldenMap::new(),
            &crate::asset::LabelMap::new(),
            None,
        );
        let page = super::render_html(&input(
            &report,
            &pairs,
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeMap::new(),
        ));
        assert!(
            !page.contains("<script>alert"),
            "script payloads never land raw: {page}"
        );
        assert!(
            page.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
            "case text escapes to entities"
        );
        assert!(
            !page.contains("</details><img"),
            "round text never breaks out of the markup"
        );
        assert!(page.contains("&lt;img src=x"));
    }

    #[test]
    fn export_document_matches_the_import_schema() {
        let row_json = serde_json::to_string(&LabelRow {
            id: "task-word-01".to_owned(),
            verdict: LabelVerdict::Pass,
            note: Some("例句全".to_owned()),
        })
        .expect("label row json");
        let document = format!(r#"{{"labels":[{row_json}]}}"#);
        let parsed: BTreeMap<String, Vec<LabelRow>> = serde_json::from_str(&document)
            .expect("the export document parses as the import schema");
        assert_eq!(parsed["labels"][0].id, "task-word-01");
        assert_eq!(parsed["labels"][0].verdict, LabelVerdict::Pass);
        assert_eq!(parsed["labels"][0].note.as_deref(), Some("例句全"));

        let kind = EvalKind::Task(TaskKind::TranslateWord);
        let eval_case = case("task-word-01", &kind);
        let detail = detail("task-word-01", &kind);
        let pairs = vec![pair(&eval_case, &detail)];
        let report = assemble(
            std::slice::from_ref(&detail),
            &crate::asset::GoldenMap::new(),
            &crate::asset::LabelMap::new(),
            None,
        );
        let page = super::render_html(&input(
            &report,
            &pairs,
            &std::collections::BTreeMap::new(),
            &std::collections::BTreeMap::new(),
        ));
        assert!(
            page.contains("labels.push({id:el.dataset.id,verdict:on.dataset.v,note:"),
            "the page script emits rows with the same three fields: {page}"
        );
        assert!(
            page.contains("JSON.stringify({labels}"),
            "the export wraps rows under the labels key"
        );
    }
}
