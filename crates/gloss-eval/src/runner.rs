//! 评测 runner：重放轨（确定性、CI 安全）与 live 轨（opt-in）的编排。
//!
//! 两条轨共享同一套评分器（[`crate::metrics`]）与数据集；差别只在回复
//! 的来源——重放轨取 `fixtures/` 里录制的增量，live 轨打真实 LLM 并
//! （可选）把增量交回调用方落盘夹具、启用 judge。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use gloss_core::config::{ALL_KINDS, Config, ModelBinding};
use gloss_core::config_handle::ConfigHandle;
use gloss_core::model::{GlossError, Locale};
use gloss_core::ports::{AiEngine, ConfigStore, EngineRequest};
use gloss_core::prompt::PromptRegistry;
use gloss_core::task::{Task, TaskInput, TaskKind, TaskOptions};

use crate::EvalReport;
use crate::assets;
use crate::dataset::{Fixture, TaskCase, load_classify, load_fixtures, load_task};
use crate::judge;
use crate::metrics::{ClassifyMetrics, Latency, TaskMetrics, TaskVerdict, judge_classify_reply};
use crate::report::assemble_report;

/// live 轨的一次运行产出：报告 + 夹具录制（调用方决定是否落盘）。
#[derive(Debug, Clone)]
pub struct LiveRun {
    /// 评测报告（含延迟与可选 judge 摘要）。
    pub report: EvalReport,
    /// `--record` 时的增量录制（分类 + 任务，按 id 可 upsert 回夹具）。
    pub recordings: Vec<Fixture>,
}

/// 重放轨：数据集 × 夹具，经生产解析函数算指标。无网络无凭据。
///
/// 只统计有夹具的条目（其余计入 skipped）——夹具是维护的受控工件，
/// 数量决定确定性轨的覆盖面。
pub fn run_replay() -> Result<EvalReport, String> {
    let classify_cases = load_classify(assets::CLASSIFY_DATASET)?;
    let classify_fixtures = load_fixtures(assets::CLASSIFY_FIXTURES)?;
    let by_id: HashMap<&str, &Fixture> = classify_fixtures
        .iter()
        .map(|fixture| (fixture.id.as_str(), fixture))
        .collect();
    let allowed: Vec<TaskKind> = ALL_KINDS
        .iter()
        .copied()
        .filter(|kind| kind.accepts_text())
        .collect();

    let mut classify = ClassifyMetrics::default();
    let mut skipped = 0usize;
    for case in &classify_cases {
        let Some(fixture) = by_id.get(case.id.as_str()) else {
            skipped += 1;
            continue;
        };
        let reply = fixture.deltas.concat();
        classify.evaluated += 1;
        match judge_classify_reply(&reply, case.expected, &allowed) {
            crate::metrics::ClassifyVerdict::Correct => classify.correct += 1,
            crate::metrics::ClassifyVerdict::Wrong(actual) => {
                bump_confusion(&mut classify, case.expected, actual);
            }
            crate::metrics::ClassifyVerdict::InvalidJson => classify.invalid_json += 1,
            crate::metrics::ClassifyVerdict::Rejected => classify.rejected += 1,
        }
    }

    let task_fixtures = load_fixtures(assets::TASK_FIXTURES)?;
    let task_by_id: HashMap<&str, &Fixture> = task_fixtures
        .iter()
        .map(|fixture| (fixture.id.as_str(), fixture))
        .collect();
    let mut tasks = Vec::new();
    for (name, dataset) in TASK_DATASETS {
        let cases = load_task(dataset)?;
        let mut metrics = TaskMetrics::default();
        for case in &cases {
            let Some(fixture) = task_by_id.get(case.id.as_str()) else {
                skipped += 1;
                continue;
            };
            metrics.record(&TaskVerdict::for_reply(case, &fixture.deltas.concat()));
        }
        tasks.push((name, metrics));
    }

    Ok(assemble_report(
        "replay",
        Some(classify),
        tasks,
        None,
        None,
        skipped,
    ))
}

/// live 轨：数据集逐条打真实 LLM。延迟只在这一轨采集。
///
/// `engine` 与 `config` 由调用方组装（`main` 从环境变量构造）——runner
/// 本身不读环境，保持可测。
pub async fn run_live(
    engine: Arc<dyn AiEngine>,
    config: Arc<ConfigHandle>,
    options: LiveOptions,
) -> Result<LiveRun, String> {
    let allowed: Vec<TaskKind> = ALL_KINDS
        .iter()
        .copied()
        .filter(|kind| kind.accepts_text())
        .collect();
    let classify_cases = load_classify(assets::CLASSIFY_DATASET)?;
    let mut classify = ClassifyMetrics::default();
    let mut latency = Latency::default();
    let mut recordings = Vec::new();

    let cases: Vec<_> = match options.limit {
        Some(limit) => classify_cases.into_iter().take(limit).collect(),
        None => classify_cases,
    };
    for case in &cases {
        let started = Instant::now();
        let reply = match run_classify_request(engine.as_ref(), &config, &case.text).await {
            Ok(reply) => reply,
            Err(_) => {
                // 网络失败也计入回退侧（分类请求失败＝该条没有可用判定），
                // 逐条继续——评测要跑完全集而不是中途夭折。
                classify.evaluated += 1;
                classify.rejected += 1;
                continue;
            }
        };
        latency.record(started.elapsed().as_secs_f64() * 1000.0);
        if options.record {
            recordings.push(Fixture {
                id: case.id.clone(),
                deltas: vec![reply.clone()],
            });
        }
        classify.evaluated += 1;
        match judge_classify_reply(&reply, case.expected, &allowed) {
            crate::metrics::ClassifyVerdict::Correct => classify.correct += 1,
            crate::metrics::ClassifyVerdict::Wrong(actual) => {
                bump_confusion(&mut classify, case.expected, actual);
            }
            crate::metrics::ClassifyVerdict::InvalidJson => classify.invalid_json += 1,
            crate::metrics::ClassifyVerdict::Rejected => classify.rejected += 1,
        }
    }

    let mut tasks = Vec::new();
    let mut judge_scores = Vec::new();
    for (name, dataset) in TASK_DATASETS {
        let cases = load_task(dataset)?;
        let cases: Vec<_> = match options.limit {
            Some(limit) => cases.into_iter().take(limit).collect(),
            None => cases,
        };
        let mut metrics = TaskMetrics::default();
        for case in &cases {
            let started = Instant::now();
            let reply = run_task_request(engine.as_ref(), &config, case)
                .await
                .map_err(|err| err.to_string())?;
            latency.record(started.elapsed().as_secs_f64() * 1000.0);
            if options.record {
                recordings.push(Fixture {
                    id: case.id.clone(),
                    deltas: vec![reply.clone()],
                });
            }
            metrics.record(&TaskVerdict::for_reply(case, &reply));
            if options.judge
                && let Some(score) = judge_one(engine.as_ref(), &config, case, &reply).await
            {
                judge_scores.push(score);
            }
        }
        tasks.push((name, metrics));
    }

    let judge_summary = options.judge.then(|| {
        let count = judge_scores.len();
        let average = if judge_scores.is_empty() {
            0.0
        } else {
            judge_scores
                .iter()
                .map(|score| f64::from(*score))
                .sum::<f64>()
                / count as f64
        };
        crate::report::JudgeSummary { count, average }
    });

    Ok(LiveRun {
        report: assemble_report(
            "live",
            Some(classify),
            tasks,
            Some(&latency),
            judge_summary,
            0,
        ),
        recordings,
    })
}

/// live 轨的端点配置（全部来自 `GLOSS_LIVE_*` 环境变量，缺一即失败）。
#[derive(Debug, Clone)]
pub struct LiveEnv {
    /// OpenAI 兼容端点 base_url。
    pub base_url: String,
    /// 本次评测使用的模型 id（对全部 kind 生效，含分类）。
    pub model: String,
    /// API key（只进请求头；不进日志与报告）。
    pub api_key: String,
}

impl LiveEnv {
    /// 从 `GLOSS_LIVE_BASE_URL` / `GLOSS_LIVE_MODEL` / `GLOSS_LIVE_API_KEY`
    /// 读取；缺失时带修复指引报错（与平台 live 测试同一套变量）。
    pub fn from_env() -> Result<Self, String> {
        let read = |name: &str| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    format!(
                        "missing {name}; set GLOSS_LIVE_API_KEY=sk-... \
                         GLOSS_LIVE_BASE_URL=https://api.deepseek.com/v1 \
                         GLOSS_LIVE_MODEL=deepseek-chat before running the live eval"
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

/// live 轨的选项。
#[derive(Debug, Clone, Copy, Default)]
pub struct LiveOptions {
    /// 把逐条增量交回调用方（main 据此 upsert `fixtures/*.jsonl`）。
    pub record: bool,
    /// 启用 judge 评分（额外一次模型调用/条）。
    pub judge: bool,
    /// 每个数据集最多评测多少条（冒烟用）。
    pub limit: Option<usize>,
}

/// 任务数据集清单：名字 → 内嵌资产。
const TASK_DATASETS: [(&str, &str); 3] = [
    ("task_translate_word", assets::TASK_WORD_DATASET),
    ("task_translate_sentence", assets::TASK_SENTENCE_DATASET),
    ("task_explain_code", assets::TASK_CODE_DATASET),
];

/// 组装 live 轨的配置句柄：端点与模型对全部 kind 生效，密钥经内存存储
/// 直查（不落盘、不进日志）。
pub fn live_config(env: &LiveEnv) -> (Arc<ConfigHandle>, Arc<dyn ConfigStore>) {
    let store = Arc::new(EnvSecretStore::new(env.api_key.clone()));
    let config = Config {
        base_url: env.base_url.clone(),
        model_by_kind: ALL_KINDS
            .iter()
            .map(|kind| ModelBinding {
                kind: *kind,
                model: env.model.clone(),
            })
            .collect(),
        ..Config::default()
    };
    let handle = Arc::new(ConfigHandle::with_config(
        Arc::clone(&store) as Arc<dyn ConfigStore>,
        config,
    ));
    (handle, store)
}

/// 分类请求：与生产桥同一形状（极小 prompt + max_tokens 截断）。
async fn run_classify_request(
    engine: &dyn AiEngine,
    config: &ConfigHandle,
    text: &str,
) -> Result<String, GlossError> {
    let snapshot = config.snapshot();
    let model = snapshot
        .resolved_model(snapshot.classify_fallback())
        .ok_or_else(|| GlossError::Config("no model for classification".into()))?
        .to_owned();
    let allowed: Vec<TaskKind> = ALL_KINDS
        .iter()
        .copied()
        .filter(|kind| kind.accepts_text())
        .collect();
    let messages = PromptRegistry::new().render_classify(Locale::Zh, &allowed, text);
    collect(
        engine,
        &EngineRequest {
            kind: TaskKind::Auto,
            messages,
            model,
            max_tokens: Some(gloss_core::classify::CLASSIFY_MAX_TOKENS),
        },
    )
    .await
}

/// 任务请求：与生产 execute 同一条渲染路径（PromptRegistry）。
async fn run_task_request(
    engine: &dyn AiEngine,
    config: &ConfigHandle,
    case: &TaskCase,
) -> Result<String, GlossError> {
    let snapshot = config.snapshot();
    let model = snapshot
        .resolved_model(case.kind)
        .ok_or_else(|| GlossError::Config("no model for this task kind".into()))?
        .to_owned();
    let task = Task {
        kind: case.kind,
        input: TaskInput::Text {
            text: case.text.clone(),
            hint: None,
        },
        options: TaskOptions {
            prompt_locale: Some(Locale::Zh),
            ..TaskOptions::default()
        },
    };
    let messages = PromptRegistry::new().render(&task)?;
    collect(
        engine,
        &EngineRequest {
            kind: case.kind,
            messages,
            model,
            max_tokens: None,
        },
    )
    .await
}

/// judge 单条评分；judge 模型与任务模型同端点同 id。
async fn judge_one(
    engine: &dyn AiEngine,
    config: &ConfigHandle,
    case: &TaskCase,
    reply: &str,
) -> Option<u8> {
    let model = config
        .snapshot()
        .resolved_model(case.kind)
        .map(str::to_owned)?;
    let reference = case.reference.to_string();
    let messages = judge::render_judge(&case.text, reply, &reference);
    let judge_reply = collect(
        engine,
        &EngineRequest {
            kind: case.kind,
            messages,
            model,
            max_tokens: Some(gloss_core::classify::CLASSIFY_MAX_TOKENS),
        },
    )
    .await
    .ok()?;
    judge::parse_judge_reply(&judge_reply)
}

/// 消费一条引擎流：拼出完整回复原文（指标判定与夹具录制共用）。
async fn collect(engine: &dyn AiEngine, request: &EngineRequest) -> Result<String, GlossError> {
    let mut stream = engine.execute(request).await?;
    let mut reply = String::new();
    while let Some(item) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
        match item {
            Ok(delta) => reply.push_str(&delta),
            Err(error) => return Err(error),
        }
    }
    Ok(reply)
}

fn bump_confusion(metrics: &mut ClassifyMetrics, expected: TaskKind, actual: TaskKind) {
    metrics
        .confusion
        .entry(expected)
        .or_default()
        .entry(actual)
        .and_modify(|count| *count += 1)
        .or_insert(1);
}

/// live 轨用的密钥存储：key 从环境变量来，只进请求头。
///
/// 其余方法按 [`ConfigStore`] 契约给无害实现（评测不落盘、不读文件）。
pub struct EnvSecretStore {
    key: String,
}

impl EnvSecretStore {
    /// 用环境变量里的 key 构造。
    pub fn new(key: String) -> Self {
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

#[cfg(test)]
mod tests {
    use super::run_replay;

    #[test]
    fn replay_runs_the_full_deterministic_track_over_embedded_assets() {
        let report = run_replay().expect("replay must succeed offline");
        assert_eq!(report.mode, "replay");
        let classify = report.classify.as_ref().expect("classify track");
        assert!(classify.metrics.evaluated > 0, "fixtures must cover cases");
        assert!(
            classify.metrics.evaluated < 68,
            "fixtures cover a maintained subset, not the whole dataset"
        );
        assert!(
            classify.accuracy > 0.0,
            "the fixture set contains correct judgments: {}",
            classify.accuracy
        );
        assert!(
            classify.metrics.invalid_json > 0 && classify.metrics.rejected > 0,
            "the fixture set pins the invalid-json and rejected paths"
        );
        assert!(
            !report.tasks.is_empty()
                && report.tasks.iter().all(|stats| stats.metrics.evaluated > 0),
            "every task dataset must have replay coverage"
        );
        assert!(report.skipped > 0);
        assert!(report.latency.is_none(), "replay has no latency samples");
    }
}
