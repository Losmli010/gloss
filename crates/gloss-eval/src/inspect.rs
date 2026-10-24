//! 检视页渲染：报告、逐条明细与历史 → 单文件检视页（HTML）。
//!
//! 渲染入口是纯函数，不读环境、不落盘；数据集文本与模型回复等外来
//! 内容一律经（HTML）转义落进静态标记，交互只用内联的十几行原生
//! 脚本（JavaScript）——产物零外部资源，离线双击可开，也可直接分发
//! 给无终端的评审者。
//!
//! 口径脚注必须随页走：重放页的准确率是生产校验器对「固化夹具」的
//! 判定，改提示词并不会动它，需重录夹具；改提示词后的真实准确率看
//! 实跑（live）页。这条不写清，检视页会把人引向错误结论。

use gloss_core::task::TaskKind;

use crate::detail::{CLASSIFY_GROUP, CaseDetail, ClassifyOutcome, TaskOutcome};
use crate::report::{ClassifyStats, EvalReport, format_utc};
use crate::svg;

pub fn render_html(report: &EvalReport, cases: &[CaseDetail], history: &[EvalReport]) -> String {
    let mut out = String::new();
    out.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str(&format!(
        "<title>Gloss prompt eval · {}</title>\n",
        esc(&report.mode)
    ));
    out.push_str(&format!("<style>{CSS}</style>\n</head>\n<body>\n"));
    render_header(&mut out, report);
    render_cards(&mut out, report, previous_run(report, history));
    render_confusion(&mut out, report);
    render_trend(&mut out, history);
    render_cases(&mut out, cases);
    out.push_str(
        "<p class=\"footnote\">Scope: the replay page measures the production validator on pinned \
         fixtures — prompt changes do not move it until fixtures are re-recorded with \
         `eval live --record`; true accuracy after prompt changes lives on the live page \
         (the live degraded rate includes request failures, marked with the engine-failure \
         badge in case rows). Trend data source: history.jsonl, timestamps in UTC.</p>\n",
    );
    out.push_str("<script>\nlet statusFilter='all',groupFilter='all';\n");
    out.push_str(
        "function apply(){document.querySelectorAll('details.case').forEach(el=>{\
         const s=statusFilter==='all'||statusFilter==='repairs'||el.dataset.status===statusFilter;\
         const r=statusFilter!=='repairs'||Number(el.dataset.repairs)>0;\
         const g=groupFilter==='all'||el.dataset.group===groupFilter;\
         el.style.display=s&&r&&g?'':'none';});}\n",
    );
    out.push_str(
        "function pick(kind,v){document.querySelectorAll('button.'+kind).forEach(b=>\
         {b.classList.toggle('on',b.dataset.v===v);});}\n",
    );
    out.push_str(
        "document.querySelector('.filters').addEventListener('click',e=>{\
         const b=e.target.closest('button');\
         if(!b)return;\
         if(b.classList.contains('status')){statusFilter=b.dataset.v;pick('status',statusFilter);apply();}\
         else if(b.classList.contains('group')){groupFilter=b.dataset.v;pick('group',groupFilter);apply();}});\n",
    );
    out.push_str("</script>\n</body>\n</html>\n");
    out
}

fn render_header(out: &mut String, report: &EvalReport) {
    let evaluated = report
        .classify
        .as_ref()
        .map_or(0, |stats| stats.metrics.evaluated)
        + report
            .tasks
            .iter()
            .map(|stats| stats.metrics.evaluated)
            .sum::<usize>();
    let mut meta = format!(
        "{} UTC · {}",
        format_utc(report.generated_at),
        esc(&report.mode)
    );
    if let Some(context) = &report.context {
        if let Some(commit) = &context.commit {
            meta.push_str(&format!(" · commit {}", esc(commit)));
        }
        if let Some(source) = &context.source {
            meta.push_str(&format!(" · source {}", esc(source)));
        }
    }
    meta.push_str(&format!(
        " · evaluated {evaluated}, skipped {}",
        report.skipped
    ));
    out.push_str(&format!(
        "<header><h1>Gloss prompt eval inspector <span class=\"mode\">{}</span></h1>\
         <p class=\"meta\">{}</p></header>\n",
        esc(&report.mode),
        meta
    ));
}

fn render_cards(out: &mut String, report: &EvalReport, previous: Option<&EvalReport>) {
    out.push_str("<section class=\"cards\">");
    if let Some(stats) = &report.classify {
        render_classify_card(out, stats, previous);
    }
    render_task_cards(out, report, previous);
    out.push_str("</section>\n");
}

fn render_classify_card(out: &mut String, stats: &ClassifyStats, previous: Option<&EvalReport>) {
    let metrics = &stats.metrics;
    let prev = previous.and_then(|prev| prev.classify.as_ref());
    let accuracy_delta = comparable(
        metrics.evaluated,
        prev,
        |prev| prev.metrics.evaluated,
        |prev| prev.accuracy * 100.0,
    );
    let fallback_delta = comparable(
        metrics.evaluated,
        prev,
        |prev| prev.metrics.evaluated,
        |prev| prev.fallback_rate * 100.0,
    );
    out.push_str(&format!(
        "<div class=\"card\"><div class=\"card-title\">classify accuracy {}</div>\
         <div class=\"card-main\">{:.2}%<span class=\"frac\">{}/{} cases</span></div>\
         <div class=\"card-sub\">fallback {:.2}%{} · repair rounds {} · invalid JSON {} · rejected {}{}</div></div>",
        delta_chip(stats.accuracy * 100.0, accuracy_delta, DeltaDir::UpGood),
        stats.accuracy * 100.0,
        metrics.correct,
        metrics.evaluated,
        stats.fallback_rate * 100.0,
        delta_chip(
            stats.fallback_rate * 100.0,
            fallback_delta,
            DeltaDir::DownGood
        ),
        metrics.repair_rounds,
        metrics.invalid_json,
        metrics.rejected,
        engine_note(metrics.engine_errors),
    ));
}

fn render_task_cards(out: &mut String, report: &EvalReport, previous: Option<&EvalReport>) {
    for stats in &report.tasks {
        let metrics = &stats.metrics;
        let prev = previous
            .and_then(|prev| prev.tasks.iter().find(|task| task.dataset == stats.dataset))
            .map(|task| &task.metrics);
        let prev_complete = comparable(
            metrics.evaluated,
            prev,
            |prev| prev.evaluated,
            |prev| rate(prev.fields_complete, prev.evaluated),
        );
        let prev_degraded = comparable(
            metrics.evaluated,
            prev,
            |prev| prev.evaluated,
            |prev| rate(prev.degraded, prev.evaluated),
        );
        let complete_rate = rate(metrics.fields_complete, metrics.evaluated);
        let degraded_rate = rate(metrics.degraded, metrics.evaluated);
        out.push_str(&format!(
            "<div class=\"card\"><div class=\"card-title\">{} · fields-complete {}</div>\
             <div class=\"card-main\">{complete_rate:.2}%<span class=\"frac\">{}/{} cases</span></div>\
             <div class=\"card-sub\">contract JSON {:.2}% · note presence {:.2}% · <span class=\"{}\">degraded {degraded_rate:.2}%{}</span> · repair rounds {}</div></div>",
            esc(dataset_label(&stats.dataset)),
            delta_chip(complete_rate, prev_complete, DeltaDir::UpGood),
            metrics.fields_complete,
            metrics.evaluated,
            rate(metrics.json_object, metrics.evaluated),
            rate(metrics.note_present, metrics.evaluated),
            if degraded_rate > 0.0 { "bad-text" } else { "" },
            delta_chip(degraded_rate, prev_degraded, DeltaDir::DownGood),
            metrics.repair_rounds,
        ));
    }
}

fn comparable<T: Copy>(
    evaluated: usize,
    prev: Option<T>,
    denominator_of: impl Fn(T) -> usize,
    rate_of: impl Fn(T) -> f64,
) -> Option<f64> {
    if evaluated == 0 {
        return None;
    }
    prev.filter(|prev| denominator_of(*prev) > 0).map(rate_of)
}

fn render_confusion(out: &mut String, report: &EvalReport) {
    let Some(stats) = &report.classify else {
        return;
    };
    if stats.confusion_rows.is_empty() {
        return;
    }
    out.push_str("<section><h2>Confusion matrix (expected → actual)</h2><table class=\"confusion\"><thead><tr><th>Expected</th><th>Actual</th><th>Count</th></tr></thead><tbody>");
    for row in &stats.confusion_rows {
        out.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
            esc(kind_label(row.expected)),
            esc(kind_label(row.actual)),
            row.count
        ));
    }
    out.push_str("</tbody></table></section>\n");
}

fn render_trend(out: &mut String, history: &[EvalReport]) {
    out.push_str(&format!(
        "<section><h2>Trend</h2>{}</section>\n",
        svg::render_svg(history)
    ));
}

fn render_cases(out: &mut String, cases: &[CaseDetail]) {
    out.push_str("<section><h2>Case details</h2><div class=\"filters\">");
    for (value, label) in [
        ("all", "All"),
        ("fail", "Failures"),
        ("warn", "Soft gaps"),
        ("repairs", "Repairs"),
    ] {
        let on = if value == "all" { " on" } else { "" };
        out.push_str(&format!(
            "<button class=\"status{on}\" data-v=\"{value}\">{label}</button>"
        ));
    }
    let mut groups: Vec<&str> = Vec::new();
    for case in cases {
        if !groups.contains(&case.group.as_str()) {
            groups.push(&case.group);
        }
    }
    out.push_str(
        "<span class=\"sep\"></span><button class=\"group on\" data-v=\"all\">All tracks</button>",
    );
    for group in groups {
        out.push_str(&format!(
            "<button class=\"group\" data-v=\"{}\">{}</button>",
            esc(group),
            esc(group_label(group))
        ));
    }
    out.push_str("</div><div class=\"cases\">");
    for case in cases {
        render_case(out, case);
    }
    out.push_str("</div></section>\n");
}

fn render_case(out: &mut String, case: &CaseDetail) {
    let (status, badge_class, badge) = case_badge(case);
    let verdict_text = case_verdict_text(case);
    out.push_str(&format!(
        "<details class=\"case\" data-status=\"{status}\" data-group=\"{}\" data-repairs=\"{}\">\
         <summary><span class=\"badge {badge_class}\">{badge}</span>\
         <span class=\"cid\">{}</span><span class=\"ctext\">{}</span>\
         <span class=\"ckind\">{}</span><span class=\"cverdict\">{}</span>\
         <span class=\"crounds\">{} rounds</span></summary><div class=\"case-body\">",
        esc(&case.group),
        case.repairs,
        esc(&case.id),
        esc(&case.text),
        esc(kind_label(case.expected)),
        esc(&verdict_text),
        case.rounds.len(),
    ));
    if case.engine_error {
        out.push_str(
            "<p class=\"muted\">The request itself failed (network or rate limit); no reply trajectory — unrelated to model behavior.</p>",
        );
    }
    if let Some(outcome) = &case.task {
        let note = if outcome.note.is_empty() {
            "(degraded: no note)".to_owned()
        } else {
            format!("final note: {}", outcome.note)
        };
        out.push_str(&format!("<p class=\"muted\">{}</p>\n", esc(&note)));
    }
    for (index, round) in case.rounds.iter().enumerate() {
        let tag = round
            .violation
            .as_deref()
            .map(|tag| format!("<span class=\"tag\">{tag}</span>"))
            .unwrap_or_default();
        out.push_str(&format!(
            "<div class=\"round\"><div class=\"round-head\">round {} {tag}</div><pre>{}</pre></div>\n",
            index + 1,
            esc(&round.text)
        ));
    }
    out.push_str("</div></details>\n");
}

fn case_badge(case: &CaseDetail) -> (&'static str, &'static str, &'static str) {
    if case.engine_error {
        return ("engine", "gray", "engine failed");
    }
    if let Some(outcome) = &case.classify {
        return match outcome {
            ClassifyOutcome::Correct => ("ok", "green", "correct"),
            ClassifyOutcome::Wrong { .. } => ("fail", "red", "misclassified"),
            ClassifyOutcome::InvalidJson => ("fail", "red", "invalid JSON"),
            ClassifyOutcome::Rejected => ("fail", "red", "rejected"),
        };
    }
    match &case.task {
        Some(TaskOutcome { degraded: true, .. }) => ("fail", "red", "degraded"),
        Some(TaskOutcome {
            fields_complete: false,
            ..
        }) => ("warn", "amber", "incomplete"),
        Some(_) => ("ok", "green", "contract met"),
        None => ("warn", "amber", "no verdict"),
    }
}

fn case_verdict_text(case: &CaseDetail) -> String {
    if let Some(ClassifyOutcome::Wrong { actual }) = &case.classify {
        return format!("misclassified as {}", kind_label(*actual));
    }
    if case.task.as_ref().is_some_and(|outcome| outcome.degraded) && !case.engine_error {
        return "degraded output".to_owned();
    }
    String::new()
}

fn previous_run<'a>(report: &EvalReport, history: &'a [EvalReport]) -> Option<&'a EvalReport> {
    history
        .iter()
        .rev()
        .find(|row| row.mode == report.mode && row.generated_at != report.generated_at)
}

#[derive(Debug, Clone, Copy)]
enum DeltaDir {
    UpGood,
    DownGood,
}

fn delta_chip(now: f64, previous: Option<f64>, dir: DeltaDir) -> String {
    let Some(previous) = previous else {
        return String::new();
    };
    let delta = now - previous;
    if delta.abs() < 0.005 {
        return String::new();
    }
    let up = delta > 0.0;
    let good = match dir {
        DeltaDir::UpGood => up,
        DeltaDir::DownGood => !up,
    };
    format!(
        "<span class=\"delta {}\" title=\"vs the previous run of the same mode\">{}{:.2}</span>",
        if good { "good" } else { "bad" },
        if up { "▲" } else { "▼" },
        delta.abs()
    )
}

fn engine_note(errors: usize) -> String {
    if errors == 0 {
        return String::new();
    }
    format!(" · engine failures {errors}")
}

fn rate(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64 * 100.0
    }
}

fn dataset_label(dataset: &str) -> &str {
    match dataset {
        "task_translate_word" => "word",
        "task_translate_sentence" => "sentence",
        "task_explain_code" => "code",
        other => other,
    }
}

fn group_label(group: &str) -> &str {
    match group {
        CLASSIFY_GROUP => CLASSIFY_GROUP,
        other => dataset_label(other),
    }
}

fn kind_label(kind: TaskKind) -> &'static str {
    match kind {
        TaskKind::TranslateWord => "word",
        TaskKind::TranslateSentence => "sentence",
        TaskKind::ExplainCode => "code",
        TaskKind::ImageExplain => "image",
        TaskKind::General => "general",
    }
}

fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

const CSS: &str = "body{font-family:-apple-system,'Segoe UI',sans-serif;color:#24292f;max-width:1000px;margin:24px auto;padding:0 16px;background:#fff}
h1{font-size:20px;margin:0 0 4px}h2{font-size:14px;margin:24px 0 8px}
.mode{font-size:12px;font-weight:600;background:#f6f8fa;border:1px solid #d7dce3;border-radius:10px;padding:2px 10px;vertical-align:2px}
.meta{color:#5b6470;font-size:12px;margin:0 0 16px}
.cards{display:flex;flex-wrap:wrap;gap:10px}
.card{border:1px solid #d7dce3;border-radius:8px;padding:10px 14px;min-width:210px}
.card-title{font-size:12px;color:#5b6470}
.card-main{font-size:22px;font-weight:650;margin:2px 0}
.frac{font-size:12px;font-weight:400;color:#5b6470;margin-left:8px}
.card-sub{font-size:12px;color:#5b6470}
.bad-text{color:#d64545}
.delta{font-size:11px;font-weight:600;margin-left:6px}
.delta.good{color:#2f9e57}.delta.bad{color:#d64545}
table.confusion{border-collapse:collapse;font-size:13px}
table.confusion th,table.confusion td{border:1px solid #d7dce3;padding:4px 12px;text-align:left}
.filters{margin:8px 0}
.filters button{font:inherit;font-size:12px;border:1px solid #d7dce3;background:#fff;border-radius:14px;padding:3px 12px;margin-right:6px;cursor:pointer;color:#5b6470}
.filters button.on{background:#24292f;color:#fff;border-color:#24292f}
.sep{display:inline-block;width:10px}
details.case{border:1px solid #d7dce3;border-radius:6px;margin:6px 0}
details.case>summary{display:grid;grid-template-columns:74px 130px 1fr 44px 110px 48px;gap:8px;align-items:center;padding:6px 10px;cursor:pointer;font-size:13px;list-style:none}
details.case>summary::-webkit-details-marker{display:none}
details.case[open]{background:#f6f8fa}
.badge{font-size:11px;font-weight:600;border-radius:9px;padding:1px 8px;text-align:center;white-space:nowrap}
.badge.green{background:#e3f4e9;color:#2f9e57}.badge.red{background:#fbe7e7;color:#d64545}
.badge.amber{background:#f9efd8;color:#b98212}.badge.gray{background:#eceff2;color:#7a828c}
.cid{color:#5b6470;font-size:12px;word-break:break-all}
.ctext{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.ckind,.crounds{color:#5b6470;font-size:12px;white-space:nowrap}
.cverdict{color:#d64545;font-size:12px;white-space:nowrap}
.case-body{padding:4px 12px 10px;border-top:1px dashed #d7dce3}
.round{margin:8px 0}
.round-head{font-size:12px;color:#5b6470;margin-bottom:2px}
.tag{background:#fbe7e7;color:#d64545;border-radius:8px;padding:0 8px;font-size:11px}
pre{margin:0;background:#f6f8fa;border:1px solid #d7dce3;border-radius:6px;padding:8px;font-size:12px;white-space:pre-wrap;word-break:break-all;max-height:220px;overflow:auto}
.muted{color:#5b6470;font-size:12px;margin:6px 0}
.footnote{color:#5b6470;font-size:12px;border-top:1px solid #d7dce3;margin-top:28px;padding-top:10px}
@media (max-width:820px){details.case>summary{grid-template-columns:74px 1fr 90px}.cid,.ckind,.crounds{display:none}}";

#[cfg(test)]
mod tests {
    use gloss_core::task::TaskKind;

    use crate::dataset::{load_classify, load_task};
    use crate::detail::{CaseDetail, classify_parts, task_parts};
    use crate::metrics::{ClassifyMetrics, TaskMetrics};
    use crate::report::{ConfusionRow, EvalReport, TrackOutputs, assemble_report};

    use super::render_html;

    fn classify_case(jsonl: &str) -> crate::dataset::ClassifyCase {
        load_classify(jsonl)
            .expect("case")
            .into_iter()
            .next()
            .expect("one case")
    }

    fn task_case(jsonl: &str) -> crate::dataset::TaskCase {
        load_task(jsonl)
            .expect("case")
            .into_iter()
            .next()
            .expect("one case")
    }

    fn sample() -> (EvalReport, Vec<CaseDetail>) {
        let ccase = classify_case(
            "{\"id\":\"zh-word-01\",\"text\":\"光泽\",\"expected\":\"TranslateWord\"}\n",
        );
        let (cdetail, verdict, track) = classify_parts(&ccase, &["不是 JSON".to_owned()]);
        assert!(matches!(
            verdict,
            crate::metrics::ClassifyVerdict::InvalidJson
        ));
        let mut classify = ClassifyMetrics {
            evaluated: 1,
            invalid_json: 1,
            ..ClassifyMetrics::default()
        };
        absorb(&mut classify, &track);

        let wcase = task_case(
            "{\"id\":\"task-word-01\",\"kind\":\"TranslateWord\",\"text\":\"gloss\",\"reference\":{\"phonetic\":null,\"examples\":[]}}\n",
        );
        let good = r#"{"note":"幂等的","structured":{"phonetic":null,"examples":[]}}"#.to_owned();
        let prose = "整段散文".to_owned();
        let (good_detail, good_verdict) = task_parts("task_translate_word", &wcase, &[good]);
        let (bad_detail, bad_verdict) = task_parts("task_translate_word", &wcase, &[prose]);
        let mut metrics = TaskMetrics::default();
        metrics.record(&good_verdict);
        metrics.record(&bad_verdict);
        (
            (assemble_report(
                "replay",
                TrackOutputs {
                    classify: Some(classify),
                    tasks: vec![("task_translate_word", metrics)],
                    latency: None,
                    judge: None,
                },
                0,
            )),
            vec![cdetail, good_detail, bad_detail],
        )
    }

    fn absorb(classify: &mut ClassifyMetrics, track: &crate::metrics::ViolationTrack) {
        for (violation, count) in &track.violations {
            *classify.violations.entry(*violation).or_default() += count;
        }
        classify.repair_rounds += track.repairs;
    }

    #[test]
    fn page_renders_headline_cases_and_trend() {
        let (report, cases) = sample();
        let html = render_html(&report, &cases, std::slice::from_ref(&report));
        assert!(html.contains("classify accuracy"), "headline card: {html}");
        assert!(html.contains("0.00%"), "the failing classify shows 0/1");
        assert!(html.contains("word · fields-complete"));
        assert!(
            html.contains("Failures") && html.contains("Repairs"),
            "filters"
        );
        assert!(html.contains("data-status=\"fail\"") && html.contains("data-status=\"ok\""));
        assert!(html.contains("<svg"), "the trend chart is inlined");
        assert!(html.contains("Case details") && html.contains("<details class=\"case\""));
        assert!(html.contains("round 1"), "per-round replies are listed");
        assert!(
            html.contains("Scope: the replay page"),
            "the calibration footnote rides the page"
        );
        assert!(
            !html.contains("Confusion matrix"),
            "no confusion section without error rows"
        );
    }

    #[test]
    fn page_escapes_dataset_and_model_text() {
        let ccase = classify_case(
            "{\"id\":\"xss-01\",\"text\":\"<script>alert(1)</script>\",\"expected\":\"TranslateWord\"}\n",
        );
        let (detail, _, _) =
            classify_parts(&ccase, &["</pre><script>alert(1)</script>".to_owned()]);
        let (report, _) = sample();
        let html = render_html(&report, &[detail], &[]);
        assert!(
            html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
            "untrusted text lands escaped: {html}"
        );
        assert!(
            !html.contains("<script>alert"),
            "no inline script from data"
        );
        assert!(
            !html.contains("</pre><script"),
            "no markup breakout from round text"
        );
    }

    fn previous_replay_report() -> EvalReport {
        assemble_report(
            "replay",
            TrackOutputs {
                classify: Some(ClassifyMetrics {
                    evaluated: 1,
                    correct: 1,
                    ..ClassifyMetrics::default()
                }),
                tasks: vec![(
                    "task_translate_word",
                    TaskMetrics {
                        evaluated: 2,
                        json_object: 2,
                        note_present: 2,
                        fields_complete: 2,
                        degraded: 0,
                        ..TaskMetrics::default()
                    },
                )],
                latency: None,
                judge: None,
            },
            0,
        )
    }

    #[test]
    fn deltas_come_from_the_previous_same_mode_run() {
        let (mut report, cases) = sample();
        report.generated_at = 2_000;
        report.classify.as_mut().expect("classify").confusion_rows = vec![ConfusionRow {
            expected: TaskKind::TranslateWord,
            actual: TaskKind::ExplainCode,
            count: 1,
        }];
        let mut previous = previous_replay_report();
        previous.generated_at = 1_000;
        let mut other_mode = previous.clone();
        other_mode.mode = "live".to_owned();

        let html = render_html(
            &report,
            &cases,
            &[other_mode.clone(), previous.clone(), report.clone()],
        );
        assert!(
            html.contains("Confusion matrix"),
            "confusion section with error rows"
        );
        assert!(
            html.contains("▼100.00"),
            "accuracy 0% vs previous 100% renders a down chip: {html}"
        );
        assert!(
            html.contains("▲100.00"),
            "fallback 100% vs previous 0% renders an up chip with bad color"
        );
        assert!(
            html.contains("delta bad"),
            "accuracy falling is colored bad"
        );
        assert!(
            !html.contains("delta good"),
            "no good-moving metric in this scenario"
        );

        let html = render_html(&report, &cases, &[other_mode]);
        assert!(
            !html.contains("delta "),
            "a live-only history never feeds replay deltas: {html}"
        );
    }

    #[test]
    fn deltas_skip_empty_denominator_runs() {
        let (mut report, cases) = sample();
        report.generated_at = 2_000;
        let mut previous = assemble_report(
            "replay",
            TrackOutputs {
                classify: Some(ClassifyMetrics::default()),
                tasks: vec![],
                latency: None,
                judge: None,
            },
            0,
        );
        previous.generated_at = 1_000;
        let html = render_html(&report, &cases, &[previous, report.clone()]);
        assert!(
            !html.contains("delta "),
            "a zero-evaluated previous run never feeds deltas: {html}"
        );
    }
}
