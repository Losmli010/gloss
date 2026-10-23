//! 检视页渲染：报告 + 逐条明细 + 历史 → 单文件 HTML（`target/eval/inspect.html`）。
//!
//! [`render_html`] 是纯函数，不读环境、不落盘；数据集文本与模型回复等
//! 外来内容一律经 HTML 转义落进静态标记，交互（过滤）只用内联的十几行
//! vanilla JS——产物零外部资源，离线双击可开，也可直接分发给无终端的
//! 评审者。
//!
//! 口径脚注必须随页走：replay 页的准确率是生产校验器对**固化夹具**的
//! 判定，改 prompt 并不会动它（需 `--record` 重录夹具）；改 prompt 后的
//! 真实准确率看 live 页。这条不写清，检视页会把人引向错误结论。

use gloss_core::task::TaskKind;

use crate::detail::{CLASSIFY_GROUP, CaseDetail, ClassifyOutcome, TaskOutcome};
use crate::report::{ClassifyStats, EvalReport, format_utc};
use crate::svg;

/// 渲染检视页。`history` 供趋势图与「对比上一次同 mode 运行」的差值。
pub fn render_html(report: &EvalReport, cases: &[CaseDetail], history: &[EvalReport]) -> String {
    let mut out = String::new();
    out.push_str("<!doctype html>\n<html lang=\"zh-CN\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str(&format!(
        "<title>Gloss Prompt 评估 · {}</title>\n",
        esc(&report.mode)
    ));
    out.push_str(&format!("<style>{CSS}</style>\n</head>\n<body>\n"));
    render_header(&mut out, report);
    render_cards(&mut out, report, previous_run(report, history));
    render_confusion(&mut out, report);
    render_trend(&mut out, history);
    render_cases(&mut out, cases);
    out.push_str(
        "<p class=\"footnote\">口径：replay 页度量生产校验器对固化夹具的判定——\
         改 prompt 不改变它，需 `eval live --record` 重录夹具后才反映；\
         改 prompt 后的真实准确率看 live 页（live 的任务降级率含请求失败条目，\
         明细行以「引擎失败」徽标区分）。趋势数据源 history.jsonl，时间一律 UTC。</p>\n",
    );
    out.push_str("<script>\nlet statusFilter='all',groupFilter='all';\n");
    out.push_str(
        "function apply(){document.querySelectorAll('details.case').forEach(el=>{\
         const s=statusFilter==='all'||statusFilter==='repairs'||el.dataset.status===statusFilter;\
         const r=statusFilter!=='repairs'||Number(el.dataset.repairs)>0;\
         const g=groupFilter==='all'||el.dataset.group===groupFilter;\
         el.style.display=s&&r&&g?'':'none';});}\n",
    );
    // 过滤值只在 data-v 属性上传递、由事件委托读取：内联 onclick 的属性
    // 实体会先于 JS 解码，等价于把数据值拼进 JS 字符串字面量。
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

/// 头部：标题 + mode 徽标 + 运行上下文与覆盖面。
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
            meta.push_str(&format!(" · 来源 {}", esc(source)));
        }
    }
    meta.push_str(&format!(
        " · 评测 {evaluated} 条，跳过 {} 条",
        report.skipped
    ));
    out.push_str(&format!(
        "<header><h1>Gloss Prompt 评估检视 <span class=\"mode\">{}</span></h1>\
         <p class=\"meta\">{}</p></header>\n",
        esc(&report.mode),
        meta
    ));
}

/// 指标卡：分类一张、任务轨每数据集一张；与上一次同 mode 运行比出差值。
fn render_cards(out: &mut String, report: &EvalReport, previous: Option<&EvalReport>) {
    out.push_str("<section class=\"cards\">");
    if let Some(stats) = &report.classify {
        render_classify_card(out, stats, previous);
    }
    render_task_cards(out, report, previous);
    out.push_str("</section>\n");
}

/// 分类卡：accuracy 与回退率带上与上一次同 mode 运行的差值徽标。
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
        "<div class=\"card\"><div class=\"card-title\">分类 accuracy {}</div>\
         <div class=\"card-main\">{:.2}%<span class=\"frac\">{}/{} 条</span></div>\
         <div class=\"card-sub\">回退率 {:.2}%{} · 修复 {} 轮 · 非 JSON {} · 拒绝 {}{}</div></div>",
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

/// 任务轨卡片：每个数据集一张，字段完整率/降级率与上次比差值。
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
            "<div class=\"card\"><div class=\"card-title\">{} · 字段完整率 {}</div>\
             <div class=\"card-main\">{complete_rate:.2}%<span class=\"frac\">{}/{} 条</span></div>\
             <div class=\"card-sub\">契约 JSON 率 {:.2}% · note 在场率 {:.2}% · <span class=\"{}\">降级率 {degraded_rate:.2}%{}</span> · 修复 {} 轮</div></div>",
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

/// 可比的上次比率：两侧分母都非零才返回（`None` 即不出差值徽标）——
/// 空分母（无夹具/全跳过/零 evaluated）的 0/0 与上次的比率没有可比性。
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

/// 混淆矩阵（仅错误路径）；无错误行整段跳过。
fn render_confusion(out: &mut String, report: &EvalReport) {
    let Some(stats) = &report.classify else {
        return;
    };
    if stats.confusion_rows.is_empty() {
        return;
    }
    out.push_str("<section><h2>混淆矩阵（期望 → 实际）</h2><table class=\"confusion\"><thead><tr><th>期望</th><th>实际</th><th>次数</th></tr></thead><tbody>");
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
        "<section><h2>趋势</h2>{}</section>\n",
        svg::render_svg(history)
    ));
}

/// 逐条明细：状态过滤 + 轨道过滤（内联 JS），每条一个原生 `<details>`
/// 展开逐轮原始回复。
fn render_cases(out: &mut String, cases: &[CaseDetail]) {
    out.push_str("<section><h2>逐条明细</h2><div class=\"filters\">");
    for (value, label) in [
        ("all", "全部"),
        ("fail", "仅错误"),
        ("warn", "仅软缺口"),
        ("repairs", "仅修复轨迹"),
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
        "<span class=\"sep\"></span><button class=\"group on\" data-v=\"all\">全部轨道</button>",
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
         <span class=\"crounds\">{} 轮</span></summary><div class=\"case-body\">",
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
            "<p class=\"muted\">请求本身失败（网络/限流），无回复轨迹——与模型行为无关。</p>",
        );
    }
    if let Some(outcome) = &case.task {
        let note = if outcome.note.is_empty() {
            "（降级：无 note）".to_owned()
        } else {
            format!("末轮 note：{}", outcome.note)
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
            "<div class=\"round\"><div class=\"round-head\">第 {} 轮 {tag}</div><pre>{}</pre></div>\n",
            index + 1,
            esc(&round.text)
        ));
    }
    out.push_str("</div></details>\n");
}

/// 状态归档（过滤键 + 徽标）：引擎失败独立于模型行为，误判/格式错/拒绝
/// 同为硬错误，缺疏证是软缺口（不降级）。
fn case_badge(case: &CaseDetail) -> (&'static str, &'static str, &'static str) {
    if case.engine_error {
        return ("engine", "gray", "引擎失败");
    }
    if let Some(outcome) = &case.classify {
        return match outcome {
            ClassifyOutcome::Correct => ("ok", "green", "判定正确"),
            ClassifyOutcome::Wrong { .. } => ("fail", "red", "误判"),
            ClassifyOutcome::InvalidJson => ("fail", "red", "非 JSON"),
            ClassifyOutcome::Rejected => ("fail", "red", "校验拒绝"),
        };
    }
    match &case.task {
        Some(TaskOutcome { degraded: true, .. }) => ("fail", "red", "降级"),
        Some(TaskOutcome {
            fields_complete: false,
            ..
        }) => ("warn", "amber", "缺疏证"),
        Some(_) => ("ok", "green", "契约达成"),
        None => ("warn", "amber", "无判定"),
    }
}

/// 判定列文本：误判给实际落点、降级给产物状态（末轮 note 预览在展开区；
/// 期望列已示期望 kind，不再重复）。
fn case_verdict_text(case: &CaseDetail) -> String {
    if let Some(ClassifyOutcome::Wrong { actual }) = &case.classify {
        return format!("判成{}", kind_label(*actual));
    }
    if case.task.as_ref().is_some_and(|outcome| outcome.degraded) && !case.engine_error {
        return "落降级产物".to_owned();
    }
    String::new()
}

/// 上一次同 mode 运行（差值基准）：时间戳相同视为本次本身，不再向前比。
fn previous_run<'a>(report: &EvalReport, history: &'a [EvalReport]) -> Option<&'a EvalReport> {
    history
        .iter()
        .rev()
        .find(|row| row.mode == report.mode && row.generated_at != report.generated_at)
}

/// 差值方向语义：accuracy/完整率涨是好，回退/降级率涨是坏。
#[derive(Debug, Clone, Copy)]
enum DeltaDir {
    UpGood,
    DownGood,
}

/// 差值徽标：无基准（首次运行或分母为 0）不显示；四舍五入后为零不发声。
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
        "<span class=\"delta {}\" title=\"对比上一次同模式运行\">{}{:.2}</span>",
        if good { "good" } else { "bad" },
        if up { "▲" } else { "▼" },
        delta.abs()
    )
}

fn engine_note(errors: usize) -> String {
    if errors == 0 {
        return String::new();
    }
    format!(" · 引擎失败 {errors}")
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
        "task_translate_word" => "词卡",
        "task_translate_sentence" => "句译",
        "task_explain_code" => "代码",
        other => other,
    }
}

/// 分组的展示名：任务数据集名 → 数据集简称；其余（分类）原样。
fn group_label(group: &str) -> &str {
    match group {
        CLASSIFY_GROUP => CLASSIFY_GROUP,
        other => dataset_label(other),
    }
}

fn kind_label(kind: TaskKind) -> &'static str {
    match kind {
        TaskKind::TranslateWord => "词卡",
        TaskKind::TranslateSentence => "句译",
        TaskKind::ExplainCode => "代码",
        TaskKind::ImageExplain => "图像",
        TaskKind::General => "通用",
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
        assert!(html.contains("分类 accuracy"), "headline card: {html}");
        assert!(html.contains("0.00%"), "the failing classify shows 0/1");
        assert!(html.contains("词卡 · 字段完整率"));
        assert!(
            html.contains("仅错误") && html.contains("仅修复轨迹"),
            "filters"
        );
        assert!(html.contains("data-status=\"fail\"") && html.contains("data-status=\"ok\""));
        assert!(html.contains("<svg"), "the trend chart is inlined");
        assert!(html.contains("逐条明细") && html.contains("<details class=\"case\""));
        assert!(html.contains("第 1 轮"), "per-round replies are listed");
        assert!(
            html.contains("口径：replay"),
            "the calibration footnote rides the page"
        );
        assert!(
            !html.contains("混淆矩阵"),
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
            html.contains("混淆矩阵"),
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
