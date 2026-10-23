//! live 轨编排：数据集逐条打真实 LLM，延迟与 judge 评分只在这一轨采集。
//! 引擎与配置由调用方组装（`main` 从环境变量构造），本模块不读环境，
//! 保持可测；轮数据一律经生产的修复循环取得，与重放轨同口径。

use std::sync::{Arc, Mutex};
use std::time::Instant;

use gloss_core::classify::classify_traced;
use gloss_core::config::Config;
use gloss_core::config_handle::ConfigHandle;
use gloss_core::engine::{RunOutput, TaskStage, run_task_rounds};
use gloss_core::model::{GlossError, Locale};
use gloss_core::ports::{AiEngine, ConfigStore, EngineRequest};
use gloss_core::prompt::PromptRegistry;
use gloss_core::task::{Task, TaskInput, TaskOptions};

use crate::EvalReport;
use crate::assets;
use crate::dataset::{ClassifyCase, Fixture, TaskCase, load_classify, load_task};
use crate::detail::{self, CaseDetail};
use crate::judge;
use crate::metrics::{ClassifyMetrics, Latency, TaskMetrics};
use crate::report::{JudgeSummary, TrackOutputs, assemble_report};

use super::{TASK_DATASETS, score_classify_case};

#[derive(Debug, Clone)]
pub struct LiveRun {
    pub report: EvalReport,
    pub cases: Vec<CaseDetail>,
    pub recordings: Vec<Fixture>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LiveOptions {
    pub record: bool,
    pub judge: bool,
    pub limit: Option<usize>,
}

#[derive(Clone)]
pub struct LiveEnv {
    pub base_url: String,
    pub model: String,
    pub api_key: String,
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
    pub fn from_env() -> Result<Self, String> {
        let read = |name: &str| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    format!(
                        "missing {name}; set GLOSS_LIVE_API_KEY=sk-... \
                         GLOSS_LIVE_BASE_URL=https://api.deepseek.com/v1 \
                         GLOSS_LIVE_MODEL=deepseek-flash before running the live eval"
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

pub fn live_config(env: LiveEnv) -> (Arc<ConfigHandle>, Arc<dyn ConfigStore>) {
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
    (handle, store)
}

struct LiveCtx {
    engine: Arc<dyn AiEngine>,
    config: Arc<ConfigHandle>,
    options: LiveOptions,
}

#[derive(Default)]
struct LiveAcc {
    latency: Latency,
    details: Vec<CaseDetail>,
    recordings: Vec<Fixture>,
}

#[derive(Default)]
struct JudgeTracker {
    scores: Vec<u8>,
    attempted: usize,
}

impl JudgeTracker {
    async fn record(&mut self, ctx: &LiveCtx, case: &TaskCase, rounds: &[String]) {
        if !ctx.options.judge {
            return;
        }
        let Some(reply) = rounds.last() else {
            return;
        };
        self.attempted += 1;
        if let Some(score) = judge_one(ctx.engine.as_ref(), &ctx.config, case, reply).await {
            self.scores.push(score);
        }
    }

    fn summary(self, enabled: bool) -> Option<JudgeSummary> {
        enabled.then(|| {
            let count = self.scores.len();
            let average = if count == 0 {
                0.0
            } else {
                self.scores
                    .iter()
                    .map(|score| f64::from(*score))
                    .sum::<f64>()
                    / count as f64
            };
            JudgeSummary {
                count,
                attempted: self.attempted,
                average,
            }
        })
    }
}

pub async fn run_live(
    engine: Arc<dyn AiEngine>,
    config: Arc<ConfigHandle>,
    options: LiveOptions,
) -> Result<LiveRun, String> {
    let ctx = LiveCtx {
        engine,
        config,
        options,
    };
    let cases = load_classify(assets::CLASSIFY_DATASET)?;
    let mut acc = LiveAcc::default();
    let classify = live_classify(&ctx, &cases, &mut acc).await?;
    let (tasks, judge_summary) = live_tasks(&ctx, &mut acc).await?;
    Ok(LiveRun {
        report: (assemble_report(
            "live",
            TrackOutputs {
                classify: Some(classify),
                tasks,
                latency: Some(&mut acc.latency),
                judge: judge_summary,
            },
            0,
        )),
        cases: acc.details,
        recordings: acc.recordings,
    })
}

async fn live_classify(
    ctx: &LiveCtx,
    cases: &[ClassifyCase],
    acc: &mut LiveAcc,
) -> Result<ClassifyMetrics, String> {
    let mut classify = ClassifyMetrics::default();
    let cases: Vec<_> = match ctx.options.limit {
        Some(limit) => cases.iter().take(limit).collect(),
        None => cases.iter().collect(),
    };
    for case in cases {
        let started = Instant::now();
        let snapshot = ctx.config.snapshot();
        let model = snapshot.model.as_str();
        let decision =
            match classify_traced(ctx.engine.as_ref(), model, Locale::Zh, &case.text).await {
                Ok(decision) => decision,
                Err(_) => {
                    classify.evaluated += 1;
                    classify.engine_errors += 1;
                    acc.details.push(detail::engine_error_detail(
                        detail::CLASSIFY_GROUP,
                        &case.id,
                        &case.text,
                        case.expected,
                        false,
                    ));
                    continue;
                }
            };
        acc.latency.record(started.elapsed().as_secs_f64() * 1000.0);
        let rounds = decision.rounds;
        acc.details
            .push(score_classify_case(&mut classify, case, &rounds));
        if ctx.options.record {
            acc.recordings.push(fixture_from_rounds(&case.id, rounds));
        }
    }
    Ok(classify)
}

async fn live_tasks(
    ctx: &LiveCtx,
    acc: &mut LiveAcc,
) -> Result<(Vec<(&'static str, TaskMetrics)>, Option<JudgeSummary>), String> {
    let mut tasks = Vec::new();
    let mut judge = JudgeTracker::default();
    for (name, dataset) in TASK_DATASETS {
        let cases = load_task(dataset)?;
        let cases: Vec<_> = match ctx.options.limit {
            Some(limit) => cases.into_iter().take(limit).collect(),
            None => cases,
        };
        let metrics = live_task_dataset(ctx, name, &cases, &mut judge, acc).await;
        tasks.push((name, metrics));
    }
    Ok((tasks, judge.summary(ctx.options.judge)))
}

async fn live_task_dataset(
    ctx: &LiveCtx,
    name: &str,
    cases: &[TaskCase],
    judge: &mut JudgeTracker,
    acc: &mut LiveAcc,
) -> TaskMetrics {
    let mut metrics = TaskMetrics::default();
    for case in cases {
        let started = Instant::now();
        let (output, rounds) = match run_task_case(ctx.engine.as_ref(), &ctx.config, case).await {
            Ok(pair) => pair,
            Err(_) => {
                metrics.evaluated += 1;
                metrics.degraded += 1;
                acc.details.push(detail::engine_error_detail(
                    name, &case.id, &case.text, case.kind, true,
                ));
                continue;
            }
        };
        acc.latency.record(started.elapsed().as_secs_f64() * 1000.0);
        let (case_detail, verdict) = detail::task_parts(name, case, &rounds);
        debug_assert_eq!(
            verdict.repairs, output.repairs,
            "round records and RunOutput.repairs must agree"
        );
        metrics.record(&verdict);
        acc.details.push(case_detail);
        judge.record(ctx, case, &rounds).await;
        if ctx.options.record {
            acc.recordings.push(fixture_from_rounds(&case.id, rounds));
        }
    }
    metrics
}

pub(super) async fn run_task_case(
    engine: &dyn AiEngine,
    config: &ConfigHandle,
    case: &TaskCase,
) -> Result<(RunOutput, Vec<String>), GlossError> {
    let snapshot = config.snapshot();
    let model = snapshot.model.as_str();
    let task = Task::new(
        case.kind,
        TaskInput::Text {
            text: case.text.clone(),
        },
        TaskOptions {
            prompt_locale: Some(Locale::Zh),
            ..TaskOptions::default()
        },
    )?;
    let base = PromptRegistry::new().render(&task)?;
    let rounds = Arc::new(Mutex::new(vec![String::new()]));
    let chunks = Arc::clone(&rounds);
    let boundary = Arc::clone(&rounds);
    let output = run_task_rounds(
        TaskStage {
            engine,
            prompts: &PromptRegistry::new(),
            kind: case.kind,
            base,
            model,
            locale: Locale::Zh,
        },
        move |delta| {
            let mut rounds = chunks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(round) = rounds.last_mut() {
                round.push_str(&delta);
            }
        },
        move || {
            boundary
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(String::new());
        },
    )
    .await?;
    let rounds = {
        let mut held = rounds
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::mem::take(&mut *held)
    };
    Ok((output, rounds))
}

async fn judge_one(
    engine: &dyn AiEngine,
    config: &ConfigHandle,
    case: &TaskCase,
    reply: &str,
) -> Option<u8> {
    let snapshot = config.snapshot();
    let model = snapshot.model.clone();
    let reference = case.reference.to_string();
    let messages = judge::render_judge(&case.text, reply, &reference);
    let judge_reply = collect(
        engine,
        EngineRequest {
            messages,
            model,
            max_tokens: Some(judge::JUDGE_MAX_TOKENS),
        },
    )
    .await
    .ok()?;
    judge::parse_judge_reply(&judge_reply)
}

async fn collect(engine: &dyn AiEngine, request: EngineRequest) -> Result<String, GlossError> {
    let mut stream = engine.execute(request).await?;
    let mut reply = String::new();
    while let Some(item) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
        match item {
            Ok(gloss_core::ports::TaskChunk::Delta(delta)) => {
                gloss_core::classify::push_capped(&mut reply, &delta);
            }
            Ok(gloss_core::ports::TaskChunk::Done { .. }) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(reply)
}

fn fixture_from_rounds(id: &str, rounds: Vec<String>) -> Fixture {
    Fixture {
        id: id.to_owned(),
        rounds: rounds.into_iter().map(|round| vec![round]).collect(),
    }
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
