//! 评测 runner：重放轨（确定性、CI 安全）与 live 轨（opt-in）的编排。
//!
//! 两条轨共享同一套评分器（[`crate::metrics`]）与数据集；差别只在回复
//! 的来源——重放轨取 `fixtures/` 里录制的修复轨迹（每轮增量），live 轨
//! 经生产循环（`classify_traced` / `run_task_rounds`）打真实 LLM 并
//! （可选）把轮次交回调用方落盘夹具、启用 judge。违约类别与修复轮次
//! 两条轨同口径：逐轮跑生产校验器（`gloss_core::schema`）折算。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use gloss_core::classify::{CLASSIFY_KINDS, CLASSIFY_REPAIRS, classify_traced};
use gloss_core::config::Config;
use gloss_core::config_handle::ConfigHandle;
use gloss_core::engine::{RunOutput, TASK_REPAIRS, run_task_rounds};
use gloss_core::model::{GlossError, Locale};
use gloss_core::ports::{AiEngine, ConfigStore, EngineRequest};
use gloss_core::prompt::PromptRegistry;
use gloss_core::schema;
use gloss_core::task::{Task, TaskInput, TaskKind, TaskOptions};

use crate::EvalReport;
use crate::assets;
use crate::dataset::{ClassifyCase, Fixture, TaskCase, load_classify, load_fixtures, load_task};
use crate::judge;
use crate::metrics::{
    ClassifyMetrics, ClassifyVerdict, Latency, TaskMetrics, TaskVerdict, ViolationTrack,
    judge_classify_reply, violation_track,
};
use crate::report::assemble_report;

/// live 轨的一次运行产出：报告 + 夹具录制（调用方决定是否落盘）。
#[derive(Debug, Clone)]
pub struct LiveRun {
    /// 评测报告（含延迟与可选 judge 摘要）。
    pub report: EvalReport,
    /// `--record` 时的修复轨迹录制（分类 + 任务，按 id 可 upsert 回夹具）。
    pub recordings: Vec<Fixture>,
}

/// 重放轨：数据集 × 夹具，经生产解析函数算指标。无网络无凭据。
///
/// 只统计有夹具的条目（其余计入 skipped）——夹具是维护的受控工件，
/// 数量决定确定性轨的覆盖面。夹具轨迹必须与生产的修复循环同形（末轮
/// 之前每轮都违约、修复轮数不超生产预算），漂移的夹具按硬错误拦下
/// （[`check_classify_trajectory`] / [`check_task_trajectory`]）。
pub fn run_replay() -> Result<EvalReport, String> {
    let classify_cases = load_classify(assets::CLASSIFY_DATASET)?;
    let classify_fixtures = load_fixtures(assets::CLASSIFY_FIXTURES)?;
    let by_id: HashMap<&str, &Fixture> = classify_fixtures
        .iter()
        .map(|fixture| (fixture.id.as_str(), fixture))
        .collect();
    let allowed: Vec<TaskKind> = CLASSIFY_KINDS.to_vec();

    let mut classify = ClassifyMetrics::default();
    let mut skipped = 0usize;
    for case in &classify_cases {
        let Some(fixture) = by_id.get(case.id.as_str()) else {
            skipped += 1;
            continue;
        };
        let rounds = round_texts(fixture);
        check_classify_trajectory(&case.id, &rounds)?;
        score_classify_case(&mut classify, case, &rounds, &allowed);
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
            let rounds = round_texts(fixture);
            check_task_trajectory(&case.id, case.kind, &rounds)?;
            metrics.record(&TaskVerdict::for_rounds(case, &rounds));
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

/// 一条分类样例的结算（重放与 live 共用同一口径）：违约轨迹并入指标、
/// 计一条已评估，并按期望 kind 判分。
fn score_classify_case(
    classify: &mut ClassifyMetrics,
    case: &ClassifyCase,
    rounds: &[String],
    allowed: &[TaskKind],
) {
    absorb_classify_track(
        classify,
        &violation_track(
            |raw| schema::validate_classify(raw, allowed).map(|_| ()),
            rounds,
        ),
    );
    classify.evaluated += 1;
    let reply = rounds.last().map(String::as_str).unwrap_or("");
    match judge_classify_reply(reply, case.expected, allowed) {
        ClassifyVerdict::Correct => classify.correct += 1,
        ClassifyVerdict::Wrong(actual) => bump_confusion(classify, case.expected, actual),
        ClassifyVerdict::InvalidJson => classify.invalid_json += 1,
        ClassifyVerdict::Rejected => classify.rejected += 1,
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
    let allowed: Vec<TaskKind> = CLASSIFY_KINDS.to_vec();
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
        let snapshot = config.snapshot();
        let model = snapshot.model.as_str();
        let decision = match classify_traced(
            engine.as_ref(),
            model,
            Locale::Zh,
            &CLASSIFY_KINDS,
            &case.text,
        )
        .await
        {
            Ok(decision) => decision,
            Err(_) => {
                // 引擎失败单列（网络/限流与模型行为无关），但也计入回退率
                // （生产编排对这类条目同样落兜底）；逐条继续——评测要跑完
                // 全集而不是中途夭折。
                classify.evaluated += 1;
                classify.engine_errors += 1;
                continue;
            }
        };
        latency.record(started.elapsed().as_secs_f64() * 1000.0);
        let rounds = decision.rounds;
        score_classify_case(&mut classify, case, &rounds, &allowed);
        // 夹具落盘放在轮记录的最后一次使用之后：逐轮文本 move 进夹具，
        // 不再逐轮复制（落盘顺序与逐条顺序不变——每条至多一次 push）。
        if options.record {
            recordings.push(fixture_from_rounds(&case.id, rounds));
        }
    }

    let mut tasks = Vec::new();
    let mut judge_scores = Vec::new();
    let mut judge_attempted = 0usize;
    for (name, dataset) in TASK_DATASETS {
        let cases = load_task(dataset)?;
        let cases: Vec<_> = match options.limit {
            Some(limit) => cases.into_iter().take(limit).collect(),
            None => cases,
        };
        let mut metrics = TaskMetrics::default();
        for case in &cases {
            let started = Instant::now();
            // 与分类轨同一「跑完全集」策略；差别在记账——分类轨把引擎
            // 失败单列 engine_errors，任务轨并入 degraded（live 的降级
            // 率因此混入传输失败，见 TaskMetrics 文档）。
            let (output, rounds) = match run_task_case(engine.as_ref(), &config, case).await {
                Ok(pair) => pair,
                Err(_) => {
                    metrics.evaluated += 1;
                    metrics.degraded += 1;
                    continue;
                }
            };
            latency.record(started.elapsed().as_secs_f64() * 1000.0);
            // 轮记录来自 run_task_rounds 本身：修复轮次与 RunOutput.repairs
            // 同源同值（on_repair 与修复计数器逐一对应），折算走与重放轨
            // 同一份 for_rounds。
            let verdict = TaskVerdict::for_rounds(case, &rounds);
            debug_assert_eq!(
                verdict.repairs, output.repairs,
                "round records and RunOutput.repairs must agree"
            );
            metrics.record(&verdict);
            if options.judge
                && let Some(reply) = rounds.last()
            {
                judge_attempted += 1;
                if let Some(score) = judge_one(engine.as_ref(), &config, case, reply).await {
                    judge_scores.push(score);
                }
            }
            // 夹具落盘放在轮记录的最后一次使用之后：逐轮文本 move 进夹具，
            // 不再逐轮复制（落盘顺序与逐条顺序不变——每条至多一次 push）。
            if options.record {
                recordings.push(fixture_from_rounds(&case.id, rounds));
            }
        }
        tasks.push((name, metrics));
    }

    // judge 启用即产摘要：全失败（count=0）时 `attempted` 仍记录尝试数，
    // 与「未启用」（judge 段整体缺席）区分开——付费调用全失败不能无痕迹。
    let judge_summary = options.judge.then(|| {
        let count = judge_scores.len();
        let average = if count == 0 {
            0.0
        } else {
            judge_scores
                .iter()
                .map(|score| f64::from(*score))
                .sum::<f64>()
                / count as f64
        };
        crate::report::JudgeSummary {
            count,
            attempted: judge_attempted,
            average,
        }
    });

    Ok(LiveRun {
        report: assemble_report(
            "live",
            Some(classify),
            tasks,
            Some(&mut latency),
            judge_summary,
            0,
        ),
        recordings,
    })
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

/// 任务数据集清单：名字 → 内嵌资产。
const TASK_DATASETS: [(&str, &str); 3] = [
    ("task_translate_word", assets::TASK_WORD_DATASET),
    ("task_translate_sentence", assets::TASK_SENTENCE_DATASET),
    ("task_explain_code", assets::TASK_CODE_DATASET),
];

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

/// 夹具轮次 → 每轮完整回复（增量按到达序拼接，与生产流式累积一致）。
fn round_texts(fixture: &Fixture) -> Vec<String> {
    fixture
        .rounds
        .iter()
        .map(|deltas| deltas.concat())
        .collect()
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

/// 夹具轨迹体检（重放轨专用）：夹具是受控工件，分类轨迹必须与生产的
/// 修复循环同形——修复轮数不超 [`CLASSIFY_REPAIRS`]，且末轮之前的每轮
/// 都违约（修复轮由违约触发）。漂移的夹具会把指标带偏，按硬错误拦下。
fn check_classify_trajectory(id: &str, rounds: &[String]) -> Result<(), String> {
    if rounds.len() as u32 > 1 + CLASSIFY_REPAIRS {
        return Err(format!(
            "classify fixture {id}: {} rounds exceed the production budget \
             (1 + CLASSIFY_REPAIRS)",
            rounds.len()
        ));
    }
    for (index, round) in rounds[..rounds.len() - 1].iter().enumerate() {
        if schema::validate_classify(round, &CLASSIFY_KINDS).is_ok() {
            return Err(format!(
                "classify fixture {id}: round {} is contract-valid but a \
                 repair round follows",
                index + 1
            ));
        }
    }
    Ok(())
}

/// 夹具轨迹体检（重放轨专用，任务版）：修复轮数不超 [`TASK_REPAIRS`]，
/// 末轮之前的每轮都违约（[`gloss_core::schema::validate_task`]）。
fn check_task_trajectory(id: &str, kind: TaskKind, rounds: &[String]) -> Result<(), String> {
    if rounds.len() as u32 > 1 + TASK_REPAIRS {
        return Err(format!(
            "task fixture {id}: {} rounds exceed the production budget \
             (1 + TASK_REPAIRS)",
            rounds.len()
        ));
    }
    for (index, round) in rounds[..rounds.len() - 1].iter().enumerate() {
        if schema::validate_task(kind, round).is_ok() {
            return Err(format!(
                "task fixture {id}: round {} is contract-valid but a \
                 repair round follows",
                index + 1
            ));
        }
    }
    Ok(())
}

/// 把轨迹的违约计数并入分类指标（任务指标走 [`TaskMetrics::record`]，
/// 计数逻辑在 [`TaskVerdict`] 里同源折算）。
fn absorb_classify_track(metrics: &mut ClassifyMetrics, track: &ViolationTrack) {
    for (violation, count) in &track.violations {
        *metrics.violations.entry(*violation).or_default() += count;
    }
    metrics.repair_rounds += track.repairs;
}

/// 任务请求：与生产 execute 同一条渲染路径（[`PromptRegistry`]）+ 生产的
/// 任务段修复循环（[`run_task_rounds`]）。返回 `RunOutput` 与逐轮完整
/// 回复——轮边界由 `on_chunk`/`on_repair` 还原，与 `RunOutput.repairs`
/// 一致（生产循环里两者逐一对应）。
async fn run_task_case(
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
        engine,
        &PromptRegistry::new(),
        case.kind,
        base,
        model,
        Locale::Zh,
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
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use futures::Stream;
    use gloss_core::ports::{AiEngine, BoxFuture, TaskChunk, TaskStream};
    use gloss_core::schema::Violation;

    use super::{check_classify_trajectory, check_task_trajectory, run_replay, run_task_case};
    use crate::dataset::load_task;
    use crate::runner::{LiveEnv, LiveOptions, live_config, run_live};

    #[test]
    fn replay_runs_the_full_deterministic_track_over_embedded_assets() {
        let report = run_replay().expect("replay must succeed offline");
        assert_eq!(report.mode, "replay");
        let classify = report.classify.as_ref().expect("classify track");
        assert!(classify.metrics.evaluated > 0, "fixtures must cover cases");
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

    #[test]
    fn replay_counts_violations_and_repair_rounds_from_fixture_trajectories() {
        let report = run_replay().expect("replay must succeed offline");
        let classify = report.classify.as_ref().expect("classify track");
        assert!(
            classify.metrics.repair_rounds > 0,
            "the fixture set contains repaired classify trajectories"
        );
        for violation in [
            Violation::NotJson,
            Violation::KindNotAllowed,
            Violation::EmptyReply,
        ] {
            assert!(
                classify.metrics.violations.contains_key(&violation),
                "{violation:?}: the exception fixtures pin prose, off-list-kind \
                 and empty-reply violations: {:?}",
                classify.metrics.violations
            );
        }
        for stats in &report.tasks {
            assert!(
                stats.metrics.repair_rounds > 0 || stats.metrics.violations.is_empty(),
                "{}: violation counts ride repaired trajectories",
                stats.dataset
            );
        }
        let word = &report
            .tasks
            .iter()
            .find(|stats| stats.dataset == "task_translate_word")
            .expect("word dataset")
            .metrics;
        assert_eq!(
            word.violations.get(&Violation::EmptyReply),
            Some(&1),
            "the word dataset carries the empty-reply exception fixture"
        );
        let sentence = &report
            .tasks
            .iter()
            .find(|stats| stats.dataset == "task_translate_sentence")
            .expect("sentence dataset")
            .metrics;
        assert!(
            sentence.degraded > 0,
            "the sentence dataset carries a budget-exhausted exception fixture"
        );
        let code = &report
            .tasks
            .iter()
            .find(|stats| stats.dataset == "task_explain_code")
            .expect("code dataset")
            .metrics;
        assert_eq!(
            code.violations.get(&Violation::MissingNote),
            Some(&1),
            "the code dataset carries the missing-note exception fixture"
        );
    }

    #[test]
    fn a_contract_valid_middle_round_rejects_the_fixture_trajectory() {
        let valid = "{\"kind\":\"TranslateWord\"}".to_owned();
        assert!(
            check_classify_trajectory("c1", &[valid.clone(), valid]).is_err(),
            "a valid round cannot precede a repair round"
        );
        let good = "{\"note\":\"义\",\"structured\":{\"examples\":[]}}".to_owned();
        assert!(
            check_task_trajectory(
                "t1",
                gloss_core::task::TaskKind::TranslateWord,
                &["散文".to_owned(), good, "散文".to_owned()],
            )
            .is_err(),
            "a valid middle round cannot precede further repair rounds"
        );
    }

    #[test]
    fn a_trajectory_beyond_the_production_budget_rejects_the_fixture() {
        let prose = "整段散文".to_owned();
        let classify_rounds = vec![prose.clone(); 3];
        assert!(
            check_classify_trajectory("c1", &classify_rounds).is_err(),
            "classify repairs are budgeted by CLASSIFY_REPAIRS"
        );
        let task_rounds = vec![prose; 5];
        assert!(
            check_task_trajectory(
                "t1",
                gloss_core::task::TaskKind::TranslateWord,
                &task_rounds
            )
            .is_err(),
            "task repairs are budgeted by TASK_REPAIRS"
        );
    }

    #[test]
    fn live_track_records_rounds_and_repair_metrics_through_the_production_loops() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime should build");
        let calls = Arc::new(AtomicUsize::new(0));
        let engine: Arc<dyn gloss_core::ports::AiEngine> = Arc::new(ScriptEngine::new(
            vec![
                vec![chunk("分类首轮散文")],
                vec![chunk("{\"kind\":\"TranslateWord\"}")],
                vec![chunk("任务首轮散文")],
                vec![chunk(
                    "{\"note\":\"词卡\",\"structured\":{\"phonetic\":null,\"examples\":[]}}",
                )],
                vec![chunk(
                    "{\"note\":\"句译\",\"structured\":{\"examples\":[]}}",
                )],
                vec![chunk(
                    "{\"note\":\"讲解\",\"structured\":{\"examples\":[],\"code_language\":null}}",
                )],
            ],
            Arc::clone(&calls),
        ));
        let env = LiveEnv {
            base_url: "http://localhost".into(),
            model: "m".into(),
            api_key: String::new(),
        };
        let (config, _store) = live_config(env);
        let run = rt
            .block_on(run_live(
                engine,
                config,
                LiveOptions {
                    record: true,
                    judge: false,
                    limit: Some(1),
                },
            ))
            .expect("the live run should complete offline");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            6,
            "2 classify + 2 word + 1 sentence + 1 code rounds"
        );

        let classify = run.report.classify.as_ref().expect("classify track");
        assert_eq!(classify.metrics.evaluated, 1);
        assert_eq!(
            classify.metrics.correct, 1,
            "the repaired round settles on the expected kind"
        );
        assert_eq!(classify.metrics.repair_rounds, 1);
        assert_eq!(
            classify.metrics.violations.get(&Violation::NotJson),
            Some(&1),
            "the violated first round counts by category, not into the final-reply rates"
        );

        assert_eq!(run.report.tasks.len(), 3);
        let word = &run.report.tasks[0].metrics;
        assert_eq!(word.evaluated, 1);
        assert_eq!(
            word.repair_rounds, 1,
            "the violated task round repairs once"
        );
        assert_eq!(word.degraded, 0, "the repair converges on a contract reply");
        assert_eq!(run.report.tasks[1].metrics.repair_rounds, 0);
        assert_eq!(run.report.tasks[2].metrics.repair_rounds, 0);

        let by_id: std::collections::BTreeMap<_, _> = run
            .recordings
            .iter()
            .map(|fixture| (fixture.id.as_str(), fixture))
            .collect();
        let classify_fixture = by_id.get("zh-word-01").expect("classify recording");
        assert_eq!(
            classify_fixture.rounds.len(),
            2,
            "both classify rounds recorded"
        );
        let word_fixture = by_id.get("task-word-01").expect("word recording");
        assert_eq!(
            word_fixture.rounds.len(),
            2,
            "violation + repair rounds recorded"
        );
        assert_eq!(
            word_fixture.rounds[1].concat(),
            "{\"note\":\"词卡\",\"structured\":{\"phonetic\":null,\"examples\":[]}}"
        );
    }

    #[test]
    fn a_task_engine_failure_still_counts_one_degraded_case() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime should build");
        let engine: Arc<dyn gloss_core::ports::AiEngine> = Arc::new(ScriptEngine::new(
            vec![
                vec![chunk("{\"kind\":\"TranslateWord\"}")],
                vec![Err(gloss_core::model::GlossError::EngineNetwork)],
                vec![chunk(
                    "{\"note\":\"句译\",\"structured\":{\"examples\":[]}}",
                )],
                vec![chunk(
                    "{\"note\":\"讲解\",\"structured\":{\"examples\":[],\"code_language\":null}}",
                )],
            ],
            Arc::new(AtomicUsize::new(0)),
        ));
        let env = LiveEnv {
            base_url: "http://localhost".into(),
            model: "m".into(),
            api_key: String::new(),
        };
        let (config, _store) = live_config(env);
        let run = rt
            .block_on(run_live(
                engine,
                config,
                LiveOptions {
                    record: false,
                    judge: false,
                    limit: Some(1),
                },
            ))
            .expect("the live run should complete offline");
        assert_eq!(run.report.tasks[0].metrics.evaluated, 1);
        assert_eq!(
            run.report.tasks[0].metrics.degraded, 1,
            "a task-stage engine failure counts into degraded (documented caveat)"
        );
        assert_eq!(run.report.tasks[0].metrics.repair_rounds, 0);
        assert!(
            run.recordings.is_empty(),
            "nothing recorded without --record"
        );
    }

    #[test]
    fn run_task_case_splits_rounds_on_the_repair_boundary() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime should build");
        let engine: Arc<dyn gloss_core::ports::AiEngine> = Arc::new(ScriptEngine::new(
            vec![
                vec![chunk("先来半截散文"), chunk("再补一段")],
                vec![
                    chunk("{\"note\":\"义\","),
                    chunk("\"structured\":{\"examples\":[]}}"),
                ],
            ],
            Arc::new(AtomicUsize::new(0)),
        ));
        let env = LiveEnv {
            base_url: "http://localhost".into(),
            model: "m".into(),
            api_key: String::new(),
        };
        let (config, _store) = live_config(env);
        let cases = load_task(
            "{\"id\":\"task-word-01\",\"kind\":\"TranslateWord\",\"text\":\"gloss\",\"reference\":{\"phonetic\":null,\"examples\":[]}}\n",
        )
        .expect("case");
        let (output, rounds) = rt
            .block_on(run_task_case(engine.as_ref(), &config, &cases[0]))
            .expect("the task case should converge");
        assert_eq!(output.repairs, 1);
        assert_eq!(rounds.len(), 2);
        assert_eq!(rounds[0], "先来半截散文再补一段");
        assert_eq!(
            rounds[1],
            "{\"note\":\"义\",\"structured\":{\"examples\":[]}}"
        );
    }

    fn chunk(text: &str) -> Result<TaskChunk, gloss_core::model::GlossError> {
        Ok(TaskChunk::Delta(text.to_owned()))
    }

    struct ScriptEngine {
        script: Mutex<std::vec::IntoIter<Vec<Result<TaskChunk, gloss_core::model::GlossError>>>>,
        calls: Arc<AtomicUsize>,
    }

    impl ScriptEngine {
        fn new(
            script: Vec<Vec<Result<TaskChunk, gloss_core::model::GlossError>>>,
            calls: Arc<AtomicUsize>,
        ) -> Self {
            Self {
                script: Mutex::new(script.into_iter()),
                calls,
            }
        }
    }

    impl AiEngine for ScriptEngine {
        fn execute(
            &self,
            _request: gloss_core::ports::EngineRequest,
        ) -> BoxFuture<'static, Result<TaskStream, gloss_core::model::GlossError>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let items = self
                .script
                .lock()
                .expect("script")
                .next()
                .unwrap_or_else(|| panic!("script exhausted: the test underspecifies the run"));
            Box::pin(async move {
                Ok(Box::pin(ChunkStream {
                    items: items.into_iter(),
                }) as TaskStream)
            })
        }
    }

    struct ChunkStream {
        items: std::vec::IntoIter<Result<TaskChunk, gloss_core::model::GlossError>>,
    }

    impl Stream for ChunkStream {
        type Item = Result<TaskChunk, gloss_core::model::GlossError>;

        fn poll_next(
            mut self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Self::Item>> {
            std::task::Poll::Ready(self.items.next())
        }
    }
}
