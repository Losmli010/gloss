//! 评测命令行（CLI）入口：报告子命令把最近一次迭代报告重渲染为
//! （Markdown）表格；历史子命令从现有历史记录文件重绘趋势矢量图
//! （SVG）；语言门子命令离线评测语言门判定，可选启用大语言模型
//! （LLM）评委抽样复核。评测凭据环境变量只在本入口读取（库层零
//! 环境副作用），引擎与配置的端点装配也在此收口。
//!
//! 报告读评测产物目录里按模式命名的（JSON）报告并打到标准输出；
//! 历史重绘写回同目录的趋势文件。语言门报告写评测产物目录。
//! 本二进制属于评测工具链，不在应用启动路径上。
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
use gloss_core::ports::{ConfigStore, SecretStore};

const REPORT_DIR: &str = "target/eval";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match args.first().map(String::as_str) {
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
    "usage: eval <report|history|gate> [options]
  report [--dir PATH]    render the latest iteration report under the eval dir as a Markdown table
  history [--dir PATH]   redraw history.svg from the existing history.jsonl (no evaluation run)
  gate [--judge] [--limit N]
                         offline language-gate verdicts over the gate dataset (precision/recall)
                         plus the remedy convergence table (violation → mechanical repair →
                         re-check); --judge additionally samples cases for an LLM second opinion
                         (needs GLOSS_LIVE_* credentials); writes target/eval/gate.md"
}

fn run_report_cli(args: &[String]) -> Result<(), String> {
    let dir = dir_option(args)?;
    let path = dir.join("iterate.json");
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
    let path = dir.join("history.jsonl");
    let history = gloss_eval::history::load_history(&path)
        .map_err(|err| format!("{}: {err}", path.display()))?;
    let svg = gloss_eval::svg::render_svg(&history);
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
                let Some(value) = args.get(index + 1) else {
                    return Err("--limit needs a number".into());
                };
                limit = value.parse().map_err(|_| "--limit needs a number")?;
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
        let env = LiveEnv::from_env()?;
        let (config, store) = live_config(env);
        let engine: Arc<dyn gloss_core::ports::AiEngine> = Arc::new(
            LlmClient::new(Arc::clone(&config), store)
                .map_err(|err| format!("http client: {err}"))?,
        );
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
                         GLOSS_LIVE_MODEL=deepseek-flash before running with --judge"
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
