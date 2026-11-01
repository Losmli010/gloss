//! 评测命令行（CLI）入口：迭代实跑、标注落库、基线固化、报告重渲染、
//! 历史重绘与语言门评测六个子命令的参数解析、环境读取与分派。流程装
//! 配（三条流的编排）住在库里，本壳只做参数解析、环境注入与打印——
//! 评测凭据与产物目录环境变量只在这里读取（库层零环境副作用），引擎
//! 与配置的端点装配也在此收口。
//!
//! 报告读评测产物目录里按模式命名的（JSON）报告并打到标准输出；历史
//! 重绘写回同目录的趋势文件；语言门报告写评测产物目录。本二进制属于
//! 评测工具链，不在应用启动路径上。
//!
//! 日志纪律的豁免面：打印报告与错误就是本命令行的全部产出（无日志
//! 订阅者、无长驻进程），标准输出与标准错误直写在此是有意为之，故
//! 整文件豁免打印类告警（缘由见上）。
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gloss_core::config::Config;
use gloss_core::config_handle::ConfigHandle;
use gloss_core::engine::llm::LlmClient;
use gloss_core::model::GlossError;
use gloss_core::ports::{AiEngine, ConfigStore, SecretStore};

const REPORT_DIR: &str = "target/eval";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match args.first().map(String::as_str) {
        Some("iterate") => run_iterate_cli(&args[1..]),
        Some("label") => run_label_cli(&args[1..]),
        Some("accept") => run_accept_cli(&args[1..]),
        Some("report") => run_report_cli(&args[1..]),
        Some("history") => run_history_cli(&args[1..]),
        Some("gate") => run_gate_cli(&args[1..]),
        _ => Err(usage().into()),
    };
    if let Err(error) = outcome {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn usage() -> &'static str {
    "usage: eval <iterate|label|accept|report|history|gate> [options]
  iterate [--no-judge] [--limit N]
                         run the curated set through the production loop, settle
                         contract/quality/regression, write iterate.json,
                         iterate.cases.json, iterate.outputs.jsonl and
                         iterate.html into the eval dir, and append
                         history.jsonl (same key overwrites in place); the
                         judge defaults on, --no-judge skips the extra model
                         call; needs GLOSS_LIVE_BASE_URL / GLOSS_LIVE_MODEL /
                         GLOSS_LIVE_API_KEY (all three, exit 1 when missing);
                         GLOSS_EVAL_DIR overrides the eval dir, GLOSS_EVAL_COMMIT
                         and GLOSS_EVAL_SOURCE ride the report context
  label --import FILE [--dir PATH]
                         merge a view-exported labels document into
                         labels.jsonl (offline); rows must pass the label
                         schema, ids outside the curated set are skipped with a
                         warning
  accept [--dir PATH]    freeze iterate.outputs.jsonl into golden.jsonl
                         (offline); unlabeled rows only warn
  report [--dir PATH]    render the latest iteration report under the eval dir
                         as a Markdown table
  history [--dir PATH]   redraw history.svg from the existing history.jsonl (no
                         evaluation run)
  gate [--judge] [--limit N]
                         offline language-gate verdicts over the gate dataset (precision/recall)
                         plus the remedy convergence table (violation → mechanical repair →
                         re-check); --judge additionally samples cases for an LLM second opinion
                         (needs GLOSS_LIVE_* credentials); writes target/eval/gate.md"
}

fn run_iterate_cli(args: &[String]) -> Result<(), String> {
    let mut judge = true;
    let mut limit = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--no-judge" => judge = false,
            "--limit" => {
                limit = Some(parse_usize(args, index, "--limit")?);
                index += 1;
            }
            other => return Err(format!("unknown option {other:?}\n{}", usage())),
        }
        index += 1;
    }
    let (config, engine) = live_runtime(LiveEnv::from_env()?)?;
    let context = gloss_eval::report::EvalContext {
        commit: env_value("GLOSS_EVAL_COMMIT"),
        source: env_value("GLOSS_EVAL_SOURCE"),
    };
    let dir = report_dir();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| format!("tokio runtime: {err}"))?;
    let report = runtime.block_on(gloss_eval::flow::run_iterate(
        engine.as_ref(),
        &config,
        gloss_eval::execute::ExecuteOptions { judge, limit },
        Path::new(&dir),
        context,
    ))?;
    println!("iterate complete: {} case(s) -> {dir}", report.totals.cases);
    print!("{}", report.render_markdown());
    Ok(())
}

fn run_label_cli(args: &[String]) -> Result<(), String> {
    let mut import: Option<PathBuf> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--import" => {
                let Some(value) = args.get(index + 1) else {
                    return Err("--import needs a file".into());
                };
                import = Some(PathBuf::from(value));
                index += 1;
            }
            "--dir" => {
                if args.get(index + 1).is_none() {
                    return Err("--dir needs a path".into());
                }
                index += 1;
            }
            other => return Err(format!("unknown option {other:?}\n{}", usage())),
        }
        index += 1;
    }
    let Some(file) = import else {
        return Err(format!("--import is required\n{}", usage()));
    };
    let dir = dir_option(args)?;
    let summary = gloss_eval::flow::import_labels(&dir, &file)?;
    println!(
        "labels merged: {} row(s) -> {}",
        summary.imported,
        dir.join(gloss_eval::flow::LABELS_FILE).display()
    );
    for id in &summary.skipped {
        println!("warning: {id} is not in the curated set, skipped");
    }
    Ok(())
}

fn run_accept_cli(args: &[String]) -> Result<(), String> {
    let dir = dir_option(args)?;
    let summary = gloss_eval::flow::accept(&dir)?;
    println!(
        "golden frozen: {} row(s) -> {}",
        summary.accepted,
        dir.join(gloss_eval::flow::GOLDEN_FILE).display()
    );
    if summary.unlabeled > 0 {
        println!(
            "warning: {} row(s) have no label yet and freeze without a human verdict",
            summary.unlabeled
        );
    }
    Ok(())
}

fn run_report_cli(args: &[String]) -> Result<(), String> {
    let dir = dir_option(args)?;
    let path = dir.join(format!("{}.json", gloss_eval::report::ITERATE_MODE));
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!(
                "no renderable report under {} — run eval iterate first",
                dir.display()
            ));
        }
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let report: gloss_eval::EvalReport =
        serde_json::from_slice(&bytes).map_err(|err| format!("{}: {err}", path.display()))?;
    print!("{}", report.render_markdown());
    Ok(())
}

fn run_history_cli(args: &[String]) -> Result<(), String> {
    let dir = dir_option(args)?;
    let path = dir.join(gloss_eval::history::HISTORY_FILE);
    let history = gloss_eval::history::load_history(&path)
        .map_err(|err| format!("{}: {err}", path.display()))?;
    let svg = gloss_eval::trend::render_svg(&history);
    let out = dir.join("history.svg");
    gloss_eval::history::write_atomic(&out, &svg)
        .map_err(|err| format!("write {}: {err}", out.display()))?;
    println!(
        "trend redrawn: {} ({} points)",
        out.display(),
        history.len()
    );
    Ok(())
}

fn run_gate_cli(args: &[String]) -> Result<(), String> {
    let mut judge = false;
    let mut limit = 8usize;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--judge" => judge = true,
            "--limit" => {
                limit = parse_usize(args, index, "--limit")?;
                index += 1;
            }
            other => return Err(format!("unknown option {other:?}\n{}", usage())),
        }
        index += 1;
    }
    let cases = gloss_eval::gate::load_gate_cases(gloss_eval::assets::LANG_GATE_DATASET)
        .map_err(|error| format!("load gate dataset: {error}"))?;
    let mut markdown = gloss_eval::gate::run_gate(&cases).render_markdown();
    let remedy_cases =
        gloss_eval::remedy::load_remedy_cases(gloss_eval::assets::LANG_GATE_REMEDY_DATASET)
            .map_err(|error| format!("load remedy dataset: {error}"))?;
    markdown.push_str(&gloss_eval::remedy::run_remedy(&remedy_cases).render_markdown());
    if judge {
        let (config, engine) = live_runtime(LiveEnv::from_env()?)?;
        let model = config.snapshot().model.clone();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| format!("tokio runtime: {err}"))?;
        let report = runtime.block_on(gloss_eval::gate::run_gate_judge(
            engine.as_ref(),
            model.as_ref(),
            &cases,
            limit,
        ));
        markdown.push_str(&report.render_markdown());
    }
    let dir = report_dir();
    let path = Path::new(&dir).join("gate.md");
    gloss_eval::history::write_atomic(&path, &markdown)
        .map_err(|err| format!("write {}: {err}", path.display()))?;
    print!("{markdown}");
    Ok(())
}

fn parse_usize(args: &[String], index: usize, flag: &str) -> Result<usize, String> {
    let Some(value) = args.get(index + 1) else {
        return Err(format!("{flag} needs a number"));
    };
    value.parse().map_err(|_| format!("{flag} needs a number"))
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

fn report_dir() -> String {
    std::env::var("GLOSS_EVAL_DIR").unwrap_or_else(|_| REPORT_DIR.to_owned())
}

fn env_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

struct LiveEnv {
    base_url: String,
    model: String,
    api_key: String,
}

impl std::fmt::Debug for LiveEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveEnv")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field(
                "api_key",
                &if self.api_key.is_empty() {
                    "<none>"
                } else {
                    "<set>"
                },
            )
            .finish()
    }
}

impl LiveEnv {
    fn from_env() -> Result<Self, String> {
        let read = |name: &str| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    format!(
                        "missing {name}; set GLOSS_LIVE_API_KEY=sk-... \
                         GLOSS_LIVE_BASE_URL=https://api.deepseek.com/v1 \
                         GLOSS_LIVE_MODEL=deepseek-flash before running live commands"
                    )
                })
        };
        Ok(Self {
            base_url: read("GLOSS_LIVE_BASE_URL")?,
            model: read("GLOSS_LIVE_MODEL")?,
            api_key: read("GLOSS_LIVE_API_KEY")?,
        })
    }
}

fn live_runtime(env: LiveEnv) -> Result<(Arc<ConfigHandle>, Arc<dyn AiEngine>), String> {
    let (config, store) = live_config(env);
    let engine: Arc<dyn AiEngine> = Arc::new(
        LlmClient::new(Arc::clone(&config), store).map_err(|err| format!("http client: {err}"))?,
    );
    Ok((config, engine))
}

fn live_config(env: LiveEnv) -> (Arc<ConfigHandle>, Arc<dyn SecretStore>) {
    let store = Arc::new(EnvSecretStore::new(env.api_key));
    let config = Config {
        base_url: env.base_url,
        model: env.model,
        ..Config::default()
    };
    let handle = Arc::new(ConfigHandle::with_config(
        Arc::clone(&store) as Arc<dyn ConfigStore>,
        config,
    ));
    (handle, Arc::clone(&store) as Arc<dyn SecretStore>)
}

struct EnvSecretStore {
    key: String,
}

impl EnvSecretStore {
    fn new(key: String) -> Self {
        Self { key }
    }
}

impl ConfigStore for EnvSecretStore {
    fn load(&self) -> Result<Config, GlossError> {
        Ok(Config::default())
    }

    fn save(&self, _config: &Config) -> Result<(), GlossError> {
        Ok(())
    }
}

impl SecretStore for EnvSecretStore {
    fn secret(&self, _key: &str) -> Result<Option<String>, GlossError> {
        Ok(Some(self.key.clone()))
    }

    fn set_secret(&self, _key: &str, _value: &str) -> Result<(), GlossError> {
        Ok(())
    }

    fn delete_secret(&self, _key: &str) -> Result<(), GlossError> {
        Ok(())
    }
}
