//! 评测 CLI：`eval replay`（离线重放，CI 安全）/ `eval live`（opt-in，
//! 需 `GLOSS_LIVE_*`）/ `eval report`（重渲染最近一次报告）。
//!
//! 报告写 `target/eval/`（`<mode>.json` + `<mode>.md`）并把 Markdown 打
//! 到 stdout；live 轨的 `--record` 把增量按 id upsert 回 `fixtures/`。
//! 本二进制属于评测工具链，不在应用启动路径上。
//!
//! 日志纪律的豁免面：打印报告/错误就是本 CLI 的全部产出（无 subscriber、
//! 无长驻进程），stdout/stderr 直写在此是有意为之，故整文件豁免
//! print_stdout / print_stderr（缘由见上）。
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::path::PathBuf;
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
        _ => Err(usage().into()),
    };
    if let Err(error) = outcome {
        eprintln!("错误：{error}");
        std::process::exit(1);
    }
}

fn usage() -> &'static str {
    "用法：eval <replay|live|report> [选项]
  replay                 离线重放夹具（CI 安全，无网络无凭据）
  live [--record] [--judge] [--limit N]
                         真实 LLM 评测（需 GLOSS_LIVE_BASE_URL/MODEL/API_KEY；
                         --record 回写夹具、--judge 启用 judge 评分轨）
  report [--dir PATH]    把 target/eval 下最近一次报告渲染为 Markdown 表"
}

fn run_replay_cli() -> Result<(), String> {
    let report = gloss_eval::runner::run_replay()?;
    let markdown = write_report(&report)?;
    print!("{markdown}");
    Ok(())
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
    let run = runtime.block_on(gloss_eval::runner::run_live(engine, config, options))?;
    if options.record {
        let recorded = upsert_fixtures(&run.recordings)?;
        println!("夹具已更新 {recorded} 条（fixtures/）。");
    }
    let markdown = write_report(&run.report)?;
    print!("{markdown}");
    Ok(())
}

fn run_report_cli(args: &[String]) -> Result<(), String> {
    let mut dir = PathBuf::from(report_dir());
    if let Some(pair) = args.iter().position(|arg| arg == "--dir") {
        let Some(value) = args.get(pair + 1) else {
            return Err("--dir needs a path".into());
        };
        dir = PathBuf::from(value);
    }
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

fn write_report(report: &gloss_eval::EvalReport) -> Result<String, String> {
    let dir = report_dir();
    gloss_eval::report::write_report(report, std::path::Path::new(&dir))
        .map_err(|err| format!("write report: {err}"))
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
