//! 评测命令行（CLI）入口：检视子命令做离线重放并产出单文件检视页；
//! 重放子命令做离线重放，持续集成（CI）安全；实跑（live）子命令按需
//! 启用，需评测凭据环境变量；报告子命令重渲染最近一次报告；历史子
//! 命令从现有历史记录文件重绘趋势矢量图（SVG）。
//!
//! 报告写评测产物目录——按模式命名的（JSON）与（Markdown）报告——
//! 并把（Markdown）打到标准输出；报告落盘成功后追加历史记录文件并
//! 重绘趋势图。检视与实跑轨额外落检视件：逐条明细文件与单文件检视
//! 页（准确率头条、逐条轨迹与趋势，供人工检视）。实跑轨的回写把
//! 修复轨迹（每轮增量）按条目标识逐条覆盖写回夹具目录。采集上下文
//! 环境变量在此注入（库层零环境副作用）。本二进制属于评测工具链，
//! 不在应用启动路径上。
//!
//! 日志纪律的豁免面：打印报告与错误就是本命令行的全部产出（无日志
//! 订阅者、无长驻进程），标准输出与标准错误直写在此是有意为之，故
//! 整文件豁免打印类告警（缘由见上）。
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gloss_core::engine::llm::LlmClient;
use gloss_eval::EvalReport;
use gloss_eval::detail::CaseDetail;

const REPORT_DIR: &str = "target/eval";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match args.first().map(String::as_str) {
        Some("inspect") => run_inspect_cli(),
        Some("replay") => run_replay_cli(),
        Some("live") => run_live_cli(&args[1..]),
        Some("report") => run_report_cli(&args[1..]),
        Some("history") => run_history_cli(&args[1..]),
        _ => Err(usage().into()),
    };
    if let Err(error) = outcome {
        eprintln!("错误：{error}");
        std::process::exit(1);
    }
}

fn usage() -> &'static str {
    "用法：eval <inspect|replay|live|report|history> [选项]
  inspect                离线重放 + 单文件检视页 target/eval/inspect.html
                         （准确率头条、逐条期望 vs 实际、逐轮回复、趋势）；
                         prompt 改动后人工检视的入口
  replay                 离线重放夹具（CI 安全，无网络无凭据）；报告落盘
                         后追加 history.jsonl 并重绘 history.svg
  live [--record] [--judge] [--limit N]
                         真实 LLM 评测（需 GLOSS_LIVE_BASE_URL/MODEL/API_KEY；
                         --record 回写夹具、--judge 启用 judge 评分轨）；
                         报告落盘后同样追加历史并重绘趋势，并落检视件
  report [--dir PATH]    把 target/eval 下最近一次报告渲染为 Markdown 表
  history [--dir PATH]   从现有 history.jsonl 重绘 history.svg（不跑评测)"
}

fn run_replay_cli() -> Result<(), String> {
    let dir = report_dir();
    let markdown = run_replay_into(Path::new(&dir), eval_context())?;
    print!("{markdown}");
    Ok(())
}

fn run_inspect_into(
    dir: &Path,
    context: Option<gloss_eval::report::EvalContext>,
) -> Result<String, String> {
    let mut run = gloss_eval::runner::run_replay_detailed()?;
    run.report.context = context;
    let markdown = finish_run_report(&run.report, dir)?;
    write_inspect(dir, &run.report, &run.cases)?;
    Ok(markdown)
}

fn run_inspect_cli() -> Result<(), String> {
    let dir = report_dir();
    let markdown = run_inspect_into(Path::new(&dir), eval_context())?;
    print!("{markdown}");
    println!("检视页：{}", Path::new(&dir).join("inspect.html").display());
    Ok(())
}

fn write_inspect(dir: &Path, report: &EvalReport, cases: &[CaseDetail]) -> Result<(), String> {
    let history = gloss_eval::history::load_history(&dir.join("history.jsonl"))
        .map_err(|err| format!("load history: {err}"))?;
    let html = gloss_eval::inspect::render_html(report, cases, &history);
    gloss_eval::history::write_atomic(&dir.join("inspect.html"), &html)
        .map_err(|err| format!("write inspect.html: {err}"))?;
    let cases_json =
        serde_json::to_string_pretty(cases).map_err(|err| format!("serialize cases: {err}"))?;
    gloss_eval::history::write_atomic(
        &dir.join(format!("{}.cases.json", report.mode)),
        &cases_json,
    )
    .map_err(|err| format!("write cases: {err}"))?;
    Ok(())
}

fn run_replay_into(
    dir: &Path,
    context: Option<gloss_eval::report::EvalContext>,
) -> Result<String, String> {
    let mut report = gloss_eval::runner::run_replay()?;
    report.context = context;
    finish_run_report(&report, dir)
}

fn finish_run_report(report: &gloss_eval::EvalReport, dir: &Path) -> Result<String, String> {
    let markdown = gloss_eval::report::write_report(report, dir)
        .map_err(|err| format!("write report: {err}"))?;
    gloss_eval::history::record_run(report, dir).map_err(|err| format!("record history: {err}"))?;
    Ok(markdown)
}

fn run_live_cli(args: &[String]) -> Result<(), String> {
    let env = gloss_eval::runner::LiveEnv::from_env()?;
    let mut options = gloss_eval::runner::LiveOptions::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--record" => options.record = true,
            "--judge" => {
                options.judge = true;
                let enabled = std::env::var("GLOSS_LIVE_JUDGE").ok().as_deref() == Some("1");
                if !enabled {
                    return Err(
                        "--judge additionally requires GLOSS_LIVE_JUDGE=1 (it costs one extra model call per case)"
                            .into(),
                    );
                }
            }
            "--limit" => {
                let Some(value) = args.get(index + 1) else {
                    return Err("--limit needs a number".into());
                };
                options.limit = Some(value.parse().map_err(|_| "--limit needs a number")?);
                index += 1;
            }
            other => return Err(format!("unknown option {other:?}\n{}", usage())),
        }
        index += 1;
    }
    let (config, store) = gloss_eval::runner::live_config(env);
    let engine: Arc<dyn gloss_core::ports::AiEngine> = Arc::new(
        LlmClient::new(Arc::clone(&config), store).map_err(|err| format!("http client: {err}"))?,
    );
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|err| format!("tokio runtime: {err}"))?;
    let mut run = runtime.block_on(gloss_eval::runner::run_live(engine, config, options))?;
    if options.record {
        let recorded = upsert_fixtures(&run.recordings)?;
        println!("夹具已更新 {recorded} 条（fixtures/）。");
    }
    let dir = report_dir();
    run.report.context = eval_context();
    let markdown = finish_run_report(&run.report, Path::new(&dir))?;
    write_inspect(Path::new(&dir), &run.report, &run.cases)?;
    print!("{markdown}");
    Ok(())
}

fn run_report_cli(args: &[String]) -> Result<(), String> {
    let dir = dir_option(args)?;
    let mut rendered = 0;
    for mode in ["replay", "live"] {
        let path = dir.join(format!("{mode}.json"));
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("{}: {error}", path.display())),
        };
        let report: gloss_eval::EvalReport =
            serde_json::from_slice(&bytes).map_err(|err| format!("{}: {err}", path.display()))?;
        print!("{}", report.render_markdown());
        rendered += 1;
    }
    if rendered == 0 {
        return Err(format!(
            "{} 下没有可渲染的报告——先跑 eval replay 或 eval live",
            dir.display()
        ));
    }
    Ok(())
}

fn run_history_cli(args: &[String]) -> Result<(), String> {
    let dir = dir_option(args)?;
    let path = dir.join("history.jsonl");
    let history = gloss_eval::history::load_history(&path)
        .map_err(|err| format!("{}: {err}", path.display()))?;
    let svg = gloss_eval::svg::render_svg(&history);
    let out = dir.join("history.svg");
    gloss_eval::history::write_atomic(&out, &svg)
        .map_err(|err| format!("write {}: {err}", out.display()))?;
    println!("趋势图已重绘：{}（{} 点）", out.display(), history.len());
    Ok(())
}

fn dir_option(args: &[String]) -> Result<PathBuf, String> {
    if let Some(pair) = args.iter().position(|arg| arg == "--dir") {
        let Some(value) = args.get(pair + 1) else {
            return Err("--dir needs a path".into());
        };
        return Ok(PathBuf::from(value));
    }
    Ok(PathBuf::from(report_dir()))
}

fn eval_context() -> Option<gloss_eval::report::EvalContext> {
    let commit = env_option("GLOSS_EVAL_COMMIT");
    let source = env_option("GLOSS_EVAL_SOURCE");
    (commit.is_some() || source.is_some())
        .then_some(gloss_eval::report::EvalContext { commit, source })
}

fn env_option(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn report_dir() -> String {
    std::env::var("GLOSS_EVAL_DIR").unwrap_or_else(|_| REPORT_DIR.to_owned())
}

fn upsert_fixtures(fixtures: &[gloss_eval::dataset::Fixture]) -> Result<usize, String> {
    let mut by_file: Vec<(&str, Vec<&gloss_eval::dataset::Fixture>)> = vec![
        ("classify_replay.jsonl", Vec::new()),
        ("task_replay.jsonl", Vec::new()),
    ];
    let classify_ids: std::collections::BTreeSet<String> =
        gloss_eval::dataset::load_classify(gloss_eval::assets::CLASSIFY_DATASET)?
            .into_iter()
            .map(|case| case.id)
            .collect();
    for fixture in fixtures {
        let slot = if classify_ids.contains(&fixture.id) {
            &mut by_file[0]
        } else {
            &mut by_file[1]
        };
        slot.1.push(fixture);
    }
    let mut written = 0;
    for (file, entries) in by_file {
        if entries.is_empty() {
            continue;
        }
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(file);
        written += merge_fixtures(&path, &entries)?;
    }
    Ok(written)
}

fn merge_fixtures(
    path: &std::path::Path,
    entries: &[&gloss_eval::dataset::Fixture],
) -> Result<usize, String> {
    let existing = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("read {}: {error}", path.display())),
    };
    let mut lines: Vec<String> = existing
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .filter(|line| {
            let id = serde_json::from_str::<serde_json::Value>(line)
                .ok()
                .and_then(|value| {
                    value
                        .get("id")
                        .and_then(|id| id.as_str().map(str::to_owned))
                });
            !entries
                .iter()
                .any(|entry| id.as_deref() == Some(entry.id.as_str()))
        })
        .collect();
    for entry in entries {
        let line = serde_json::to_string(entry).map_err(|err| err.to_string())?;
        lines.push(line);
    }
    gloss_eval::history::write_atomic(path, &format!("{}\n", lines.join("\n")))
        .map_err(|err| format!("write {}: {err}", path.display()))?;
    Ok(entries.len())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use gloss_eval::report::EvalContext;

    use super::{finish_run_report, run_inspect_into, run_replay_into};

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "gloss-eval-cli-{tag}-{}-{nanos}",
            std::process::id()
        ))
    }

    #[test]
    fn replay_run_writes_report_and_history_files() {
        let dir = temp_dir("replay-files");
        run_replay_into(&dir, None).expect("the offline replay run should succeed");
        assert!(dir.join("replay.json").is_file());
        assert!(dir.join("replay.md").is_file());
        let history = dir.join("history.jsonl");
        assert!(history.is_file(), "history.jsonl rides the same run");
        let text = std::fs::read_to_string(&history).expect("history text");
        assert_eq!(text.lines().count(), 1, "one run appends exactly one row");
        assert!(
            dir.join("history.svg").is_file(),
            "the trend file is redrawn"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn second_replay_run_appends_without_commit() {
        let dir = temp_dir("replay-append");
        run_replay_into(&dir, None).expect("first run");
        run_replay_into(&dir, None).expect("second run");
        let text = std::fs::read_to_string(dir.join("history.jsonl")).expect("history text");
        assert_eq!(
            text.lines().count(),
            2,
            "local runs without a commit always append"
        );
        assert!(dir.join("history.svg").is_file());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn finish_run_report_writes_the_injected_context_through() {
        let dir = temp_dir("replay-context");
        let mut report = gloss_eval::runner::run_replay().expect("replay");
        report.context = Some(EvalContext {
            commit: Some("ec96217".to_owned()),
            source: Some("pipeline".to_owned()),
        });
        finish_run_report(&report, &dir).expect("finish");
        let json = std::fs::read_to_string(dir.join("replay.json")).expect("report json");
        assert!(
            json.contains(r#""context": {"#) && json.contains(r#""commit": "ec96217""#),
            "the injected context lands in the written report"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn inspect_run_writes_the_inspection_files() {
        let dir = temp_dir("inspect-files");
        run_inspect_into(&dir, None).expect("the inspect run should succeed offline");
        assert!(
            dir.join("replay.json").is_file(),
            "report rides the inspect run"
        );
        let html = std::fs::read_to_string(dir.join("inspect.html")).expect("inspect html");
        assert!(html.contains("分类 accuracy"), "headline: {html}");
        assert!(html.contains("<svg"), "trend inlined");
        let cases = std::fs::read_to_string(dir.join("replay.cases.json")).expect("cases json");
        assert!(
            cases.contains("\"zh-word-01\""),
            "per-case details are dumped for diffing"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
