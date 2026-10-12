//! 评测 CLI：`eval replay`（离线重放，CI 安全）/ `eval live`（opt-in，
//! 需 `GLOSS_LIVE_*`）/ `eval report`（重渲染最近一次报告）/ `eval
//! history`（从现有 history.jsonl 重绘趋势 SVG）。
//!
//! 报告写 `target/eval/`（`<mode>.json` + `<mode>.md`）并把 Markdown 打
//! 到 stdout；报告落盘成功后追加 `history.jsonl` 并重绘 `history.svg`。
//! live 轨的 `--record` 把修复轨迹（每轮增量）按 id upsert 回 `fixtures/`。
//! `GLOSS_EVAL_COMMIT` / `GLOSS_EVAL_SOURCE` 在此注入采集上下文（lib 层
//! 零环境副作用）。本二进制属于评测工具链，不在应用启动路径上。
//!
//! 日志纪律的豁免面：打印报告/错误就是本 CLI 的全部产出（无 subscriber、
//! 无长驻进程），stdout/stderr 直写在此是有意为之，故整文件豁免
//! print_stdout / print_stderr（缘由见上）。
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gloss_core::engine::llm::LlmClient;

/// 报告目录（与 dev-plan 约定一致：报告出 target/eval/，不进仓库）。
const REPORT_DIR: &str = "target/eval";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match args.first().map(String::as_str) {
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
    "用法：eval <replay|live|report|history> [选项]
  replay                 离线重放夹具（CI 安全，无网络无凭据）；报告落盘
                         后追加 history.jsonl 并重绘 history.svg
  live [--record] [--judge] [--limit N]
                         真实 LLM 评测（需 GLOSS_LIVE_BASE_URL/MODEL/API_KEY；
                         --record 回写夹具、--judge 启用 judge 评分轨）；
                         报告落盘后同样追加历史并重绘趋势
  report [--dir PATH]    把 target/eval 下最近一次报告渲染为 Markdown 表
  history [--dir PATH]   从现有 history.jsonl 重绘 history.svg（不跑评测）"
}

fn run_replay_cli() -> Result<(), String> {
    let dir = report_dir();
    let markdown = run_replay_into(Path::new(&dir), eval_context())?;
    print!("{markdown}");
    Ok(())
}

/// 重放轨的完整落盘段：目录由参数表达 `GLOSS_EVAL_DIR` 的语义，context
/// 由调用方注入（环境读取只在 CLI 薄壳），测试直驱临时目录。
fn run_replay_into(
    dir: &Path,
    context: Option<gloss_eval::report::EvalContext>,
) -> Result<String, String> {
    let mut report = gloss_eval::runner::run_replay()?;
    report.context = context;
    finish_run_report(&report, dir)
}

/// 报告与趋势的统一落盘：先写 `<mode>.json`/`.md`，成功后追加
/// `history.jsonl` 并重绘 `history.svg`（任一步失败向上传播，不静默）。
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
    let (config, store) = gloss_eval::runner::live_config(&env);
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
    print!("{markdown}");
    Ok(())
}

fn run_report_cli(args: &[String]) -> Result<(), String> {
    let dir = dir_option(args)?;
    let mut rendered = 0;
    for mode in ["replay", "live"] {
        let path = dir.join(format!("{mode}.json"));
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
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

/// 从现有 `history.jsonl` 重绘 `<dir>/history.svg`（流水线重绘与本地补绘
/// 共用；不跑评测）。
fn run_history_cli(args: &[String]) -> Result<(), String> {
    let dir = dir_option(args)?;
    let path = dir.join("history.jsonl");
    let history = gloss_eval::history::load_history(&path)
        .map_err(|err| format!("{}: {err}", path.display()))?;
    let svg = gloss_eval::svg::render_svg(&history);
    let out = dir.join("history.svg");
    std::fs::write(&out, svg).map_err(|err| format!("write {}: {err}", out.display()))?;
    println!("趋势图已重绘：{}（{} 点）", out.display(), history.len());
    Ok(())
}

/// `--dir PATH` 选项：缺省回落报告目录（GLOSS_EVAL_DIR / target/eval）。
fn dir_option(args: &[String]) -> Result<PathBuf, String> {
    if let Some(pair) = args.iter().position(|arg| arg == "--dir") {
        let Some(value) = args.get(pair + 1) else {
            return Err("--dir needs a path".into());
        };
        return Ok(PathBuf::from(value));
    }
    Ok(PathBuf::from(report_dir()))
}

/// 采集上下文注入：GLOSS_EVAL_COMMIT / GLOSS_EVAL_SOURCE 只在本 CLI 读取
/// （lib 层零环境副作用）；两者皆缺省时不产 context 段。
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

/// 把录制按 id upsert 进夹具文件（分类/任务各自归位）。
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
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
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
        for entry in &entries {
            let line = serde_json::to_string(entry).map_err(|err| err.to_string())?;
            lines.push(line);
            written += 1;
        }
        std::fs::write(&path, format!("{}\n", lines.join("\n")))
            .map_err(|err| format!("write {}: {err}", path.display()))?;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use gloss_eval::report::EvalContext;

    use super::{finish_run_report, run_replay_into};

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
}
