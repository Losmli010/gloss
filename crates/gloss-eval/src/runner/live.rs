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

/// live 轨的一次运行产出：报告 + 逐条明细 + 夹具录制（调用方决定落盘哪些）。
#[derive(Debug, Clone)]
pub struct LiveRun {
    /// 评测报告（含延迟与可选 judge 摘要）。
    pub report: EvalReport,
    /// 逐条明细（含引擎失败条目——它们也要被检视到）。
    pub cases: Vec<CaseDetail>,
    /// `--record` 时的修复轨迹录制（分类 + 任务，按 id 可 upsert 回夹具）。
    pub recordings: Vec<Fixture>,
}

/// live 轨的选项。
#[derive(Debug, Clone, Copy, Default)]
pub struct LiveOptions {
    /// 把逐条修复轨迹交回调用方（main 据此 upsert `fixtures/*.jsonl`）。
    pub record: bool,
    /// 启用 judge 评分（额外一次模型调用/条）。
    pub judge: bool,
    /// 每个数据集最多评测多少条（冒烟用）。
    pub limit: Option<usize>,
}

/// live 轨的端点配置（全部来自 `GLOSS_LIVE_*` 环境变量，缺一即失败）。
///
/// `Debug` 手写脱敏：key 不进任何日志面（密钥红线），排查时只能看到
/// 「有/无 key」。
#[derive(Clone)]
pub struct LiveEnv {
    /// OpenAI 兼容端点 base_url。
    pub base_url: String,
    /// 本次评测使用的模型 id（对全部 kind 生效，含分类）。
    pub model: String,
    /// API key（只进请求头；不进日志与报告）。
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

/// 组装 live 轨的配置句柄：端点与模型对全部 kind 生效，密钥经内存存储
/// 直查（不落盘、不进日志）。
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

/// 一次 live 运行共享的执行环境：引擎、配置句柄与选项——两个轨段与
/// judge 评分都要用，捆在一起避免逐函数透传三件套。
struct LiveCtx {
    engine: Arc<dyn AiEngine>,
    config: Arc<ConfigHandle>,
    options: LiveOptions,
}

/// 一次 live 运行的横切账本：延迟样本、逐条明细与夹具录制，两个轨段
/// 顺序追加，末尾随 [`LiveRun`] 一次交出。
#[derive(Default)]
struct LiveAcc {
    latency: Latency,
    details: Vec<CaseDetail>,
    recordings: Vec<Fixture>,
}

/// judge 评分的横切账本：尝试数与得分数分开记（全失败时 `attempted`
/// 仍要出现在摘要里）。
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

    /// judge 启用即产摘要：全失败（count=0）时 `attempted` 仍记录尝试数，
    /// 与「未启用」（judge 段整体缺席）区分开——付费调用全失败不能无痕迹。
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

/// live 轨：数据集逐条打真实 LLM。延迟只在这一轨采集。
///
/// `engine` 与 `config` 由调用方组装（`main` 从环境变量构造）——runner
/// 本身不读环境，保持可测。分类走 [`classify_traced`]、任务走
/// [`run_task_rounds`]（生产的同一循环）：违约类别与修复轮次来自轮记录，
/// 与重放轨同口径。
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
                    // 引擎失败单列（网络/限流与模型行为无关），但也计入回退率
                    // （生产编排对这类条目同样落兜底）；逐条继续——评测要跑完
                    // 全集而不是中途夭折。
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
        // 夹具落盘放在轮记录的最后一次使用之后：逐轮文本 move 进夹具，
        // 不再逐轮复制（落盘顺序与逐条顺序不变——每条至多一次 push）。
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
        // 与分类轨同一「跑完全集」策略；差别在记账——分类轨把引擎
        // 失败单列 engine_errors，任务轨并入 degraded（live 的降级
        // 率因此混入传输失败，见 TaskMetrics 文档）。
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
        // 轮记录来自 run_task_rounds 本身：修复轮次与 RunOutput.repairs
        // 同源同值（on_repair 与修复计数器逐一对应），折算走与重放轨
        // 同一份明细结算（task_parts 内部即 for_reply + 逐轮轨迹）。
        let (case_detail, verdict) = detail::task_parts(name, case, &rounds);
        debug_assert_eq!(
            verdict.repairs, output.repairs,
            "round records and RunOutput.repairs must agree"
        );
        metrics.record(&verdict);
        acc.details.push(case_detail);
        judge.record(ctx, case, &rounds).await;
        // 夹具落盘放在轮记录的最后一次使用之后：逐轮文本 move 进夹具，
        // 不再逐轮复制（落盘顺序与逐条顺序不变——每条至多一次 push）。
        if ctx.options.record {
            acc.recordings.push(fixture_from_rounds(&case.id, rounds));
        }
    }
    metrics
}

/// 任务请求：与生产 execute 同一条渲染路径（[`PromptRegistry`]）+ 生产的
/// 任务段修复循环（[`run_task_rounds`]）。返回 `RunOutput` 与逐轮完整
/// 回复——轮边界由 `on_chunk`/`on_repair` 还原，与 `RunOutput.repairs`
/// 一致（生产循环里两者逐一对应）。
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
    // 轮记录取走即用：闭包与 Arc 的存亡不再关心（数据已 take 出来）。
    let rounds = {
        let mut held = rounds
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::mem::take(&mut *held)
    };
    Ok((output, rounds))
}

/// judge 单条评分；judge 模型与任务模型同端点同 id。
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

/// 消费一条引擎流并累积完整回复（judge 评分请求专用——评分不计修复
/// 轨迹，单轮口径）。回复累积走生产的 `push_capped`（与分类编排同一道
/// 4KiB 界，不守约的端点不能把内存灌穿）。
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

/// 轮回复 → 夹具轮次：live 的轮数据是完整回复文本，落盘按单增量成轮
/// （重放只消费拼接结果，增量切分不进任何指标）。轮文本随 `Vec` move
/// 进夹具——调用方在轮记录的最后一次使用之后才转交所有权。
fn fixture_from_rounds(id: &str, rounds: Vec<String>) -> Fixture {
    Fixture {
        id: id.to_owned(),
        rounds: rounds.into_iter().map(|round| vec![round]).collect(),
    }
}

/// live 轨用的密钥存储：key 从环境变量来，只进请求头。
///
/// 其余方法按 [`ConfigStore`] 契约给无害实现（评测不落盘、不读文件）。
struct EnvSecretStore {
    key: String,
}

impl EnvSecretStore {
    /// 用环境变量里的 key 构造。
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
