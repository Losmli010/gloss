//! 执行编排：对精选集逐条驱动一轮对话（按子类分派驱动函数）并调契约
//! 与评审结算，产出逐例明细与延迟样本。轮数据一律经生产的修复循环取
//! 得，评测数字度量的才是生产行为；回归结算与产物落盘不在此——执行
//! 不知资产存在，标注、基线与单次输出由资产仓与流程装配层接管。

use std::sync::{Arc, Mutex};
use std::time::Instant;

use gloss_core::classify::classify_traced;
use gloss_core::config_handle::ConfigHandle;
use gloss_core::engine::{TaskStage, run_task_rounds};
use gloss_core::model::{GlossError, Locale};
use gloss_core::ports::{AiEngine, EngineRequest, TaskChunk};
use gloss_core::prompt::PromptRegistry;
use gloss_core::task::{Task, TaskInput, TaskKind, TaskOptions};

use crate::contract::settle_case;
use crate::dataset::{EvalCase, load_curated};
use crate::detail::{CaseDetail, EvalKind, JudgeScores};
use crate::quality;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Latency {
    samples_ms: Vec<f64>,
}

impl Latency {
    pub fn record(&mut self, ms: f64) {
        self.samples_ms.push(ms);
    }

    pub fn count(&self) -> usize {
        self.samples_ms.len()
    }

    pub fn percentile(&mut self, p: f64) -> Option<f64> {
        if self.samples_ms.is_empty() {
            return None;
        }
        self.samples_ms.sort_by(|a, b| a.total_cmp(b));
        let index = (p / 100.0) * (self.samples_ms.len() - 1) as f64;
        let lower = index.floor() as usize;
        let upper = index.ceil() as usize;
        let weight = index - lower as f64;
        Some(self.samples_ms[lower] * (1.0 - weight) + self.samples_ms[upper] * weight)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExecuteOptions {
    pub judge: bool,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExecuteRun {
    pub cases: Vec<CaseDetail>,
    pub latency: Latency,
}

struct RunCtx<'a> {
    engine: &'a dyn AiEngine,
    model: &'a str,
    judge: bool,
}

pub async fn run(
    engine: &dyn AiEngine,
    config: &ConfigHandle,
    options: ExecuteOptions,
) -> Result<ExecuteRun, String> {
    let cases = load_curated()?;
    let selected = match options.limit {
        Some(limit) => &cases[..limit.min(cases.len())],
        None => &cases[..],
    };
    let model = config.snapshot().model.clone();
    let ctx = RunCtx {
        engine,
        model: &model,
        judge: options.judge,
    };
    let mut latency = Latency::default();
    let mut details = Vec::with_capacity(selected.len());
    for case in selected {
        let started = Instant::now();
        let (rounds, engine_error) = drive_rounds(&ctx, case).await;
        latency.record(started.elapsed().as_secs_f64() * 1000.0);
        details.push(settled_detail(&ctx, case, rounds, engine_error).await);
    }
    Ok(ExecuteRun {
        cases: details,
        latency,
    })
}

async fn drive_rounds(ctx: &RunCtx<'_>, case: &EvalCase) -> (Vec<String>, bool) {
    match case.kind {
        EvalKind::Classify => {
            match classify_traced(ctx.engine, ctx.model, Locale::Zh, &case.text).await {
                Ok(decision) => (decision.rounds, false),
                Err(_) => (Vec::new(), true),
            }
        }
        EvalKind::Task(kind) => match drive_task(ctx, kind, case).await {
            Ok(rounds) => (rounds, false),
            Err(_) => (Vec::new(), true),
        },
    }
}

async fn settled_detail(
    ctx: &RunCtx<'_>,
    case: &EvalCase,
    rounds: Vec<String>,
    engine_error: bool,
) -> CaseDetail {
    let (contract, round_details) = settle_case(case, &rounds);
    let quality = if ctx.judge && quality::judge_eligible(&case.kind, engine_error, &contract) {
        let reply = rounds.last().map(String::as_str).unwrap_or("");
        judge_case(ctx, case, reply).await
    } else {
        None
    };
    CaseDetail {
        id: case.id.clone(),
        kind: case.kind.clone(),
        text: case.text.clone(),
        rounds: round_details,
        contract,
        quality,
        rule: None,
        engine_error,
    }
}

async fn drive_task(
    ctx: &RunCtx<'_>,
    kind: TaskKind,
    case: &EvalCase,
) -> Result<Vec<String>, GlossError> {
    let task = Task::new(
        kind,
        TaskInput::Text {
            text: case.text.clone(),
        },
        TaskOptions {
            target_lang: Some(case.target),
            prompt_locale: Some(Locale::Zh),
            ..TaskOptions::default()
        },
    )?;
    let prompts = PromptRegistry::new();
    let base = prompts.render(&task)?;
    let rounds = Arc::new(Mutex::new(vec![String::new()]));
    let chunks = Arc::clone(&rounds);
    let boundary = Arc::clone(&rounds);
    let stage = TaskStage {
        engine: ctx.engine,
        prompts: &prompts,
        kind,
        base,
        model: ctx.model,
        locale: Locale::Zh,
        input: task.input(),
        target: case.target,
    };
    run_task_rounds(
        stage,
        move |delta| {
            let mut held = chunks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(round) = held.last_mut() {
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
    let mut held = rounds
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Ok(std::mem::take(&mut *held))
}

async fn judge_case(ctx: &RunCtx<'_>, case: &EvalCase, reply: &str) -> Option<JudgeScores> {
    let request = EngineRequest {
        messages: quality::render_judge(case, reply),
        model: ctx.model.to_owned(),
        max_tokens: Some(quality::JUDGE_MAX_TOKENS),
    };
    let raw = collect_reply(ctx.engine, request).await.ok()?;
    quality::parse_scores(&raw)
}

async fn collect_reply(engine: &dyn AiEngine, request: EngineRequest) -> Result<String, String> {
    let mut stream = engine
        .execute(request)
        .await
        .map_err(|error| error.to_string())?;
    let mut reply = String::new();
    while let Some(item) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
        match item {
            Ok(TaskChunk::Delta(delta)) => reply.push_str(&delta),
            Ok(TaskChunk::Done { .. }) => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(reply)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Arc;

    use futures::stream;

    use gloss_core::config::Config;
    use gloss_core::config_handle::ConfigHandle;
    use gloss_core::model::GlossError;
    use gloss_core::ports::{
        AiEngine, BoxFuture, ConfigStore, EngineRequest, TaskChunk, TaskStream,
    };

    use super::{ExecuteOptions, Latency, run};
    use crate::asset::{GoldenMap, GoldenRow, LabelMap, LabelRow};
    use crate::detail::{ContractCheck, EvalKind, LabelVerdict, RuleVerdict};
    use crate::regression;

    const CLASSIFY_REPLY: &str = r#"{"kind":"TranslateWord"}"#;
    const OTHER_CATEGORY_REPLY: &str = r#"{"kind":"ExplainCode"}"#;
    const WORD_REPLY: &str = r#"{"note":"义","structured":{"phonetic":"yī","examples":["例"]}}"#;
    const JUDGE_REPLY: &str =
        r#"{"accuracy":90,"completeness":80,"fluency":85,"presentation":75,"reason":"术语准确"}"#;

    type Script = Vec<Result<gloss_core::ports::TaskChunk, GlossError>>;

    struct ScriptEngine {
        queue: std::sync::Mutex<(VecDeque<Script>, Script)>,
    }

    impl ScriptEngine {
        fn new(scripts: Vec<Script>) -> Self {
            Self {
                queue: std::sync::Mutex::new((scripts.into_iter().collect(), Vec::new())),
            }
        }
    }

    impl AiEngine for ScriptEngine {
        fn execute(
            &self,
            _request: EngineRequest,
        ) -> BoxFuture<'static, Result<TaskStream, GlossError>> {
            let mut held = self
                .queue
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let script = match held.0.pop_front() {
                Some(script) => {
                    held.1 = script.clone();
                    script
                }
                None => held.1.clone(),
            };
            Box::pin(async move { Ok(Box::pin(stream::iter(script)) as TaskStream) })
        }
    }

    fn script(text: &str) -> Script {
        vec![
            Ok(TaskChunk::Delta(text.to_owned())),
            Ok(TaskChunk::Done {
                finish_reason: None,
            }),
        ]
    }

    fn config() -> Arc<ConfigHandle> {
        struct FixedStore;
        impl ConfigStore for FixedStore {
            fn load(&self) -> Result<Config, GlossError> {
                Ok(Config::default())
            }

            fn save(&self, _config: &Config) -> Result<(), GlossError> {
                Ok(())
            }
        }
        Arc::new(ConfigHandle::with_config(
            Arc::new(FixedStore),
            Config::default(),
        ))
    }

    fn head_scripts(judge: bool) -> Vec<Script> {
        let mut scripts: Vec<Script> = (0..22).map(|_| script(CLASSIFY_REPLY)).collect();
        scripts.push(script(WORD_REPLY));
        if judge {
            scripts.push(script(JUDGE_REPLY));
        }
        scripts
    }

    fn golden_row(id: &str, reply: &str) -> (String, GoldenRow) {
        (
            id.to_owned(),
            GoldenRow {
                id: id.to_owned(),
                rounds: vec![vec![reply.to_owned()]],
                accepted_at: "1970-01-01T00:00:00Z".to_owned(),
            },
        )
    }

    fn label_row(id: &str, verdict: LabelVerdict) -> (String, LabelRow) {
        (
            id.to_owned(),
            LabelRow {
                id: id.to_owned(),
                verdict,
                note: None,
            },
        )
    }

    fn options(judge: bool, limit: usize) -> ExecuteOptions {
        ExecuteOptions {
            judge,
            limit: Some(limit),
        }
    }

    fn verdict_of(run: &super::ExecuteRun, id: &str) -> RuleVerdict {
        run.cases
            .iter()
            .find(|case| case.id == id)
            .and_then(|case| case.rule.as_ref())
            .map(|rule| rule.verdict)
            .expect("a settled non-engine case keeps a rule")
    }

    #[tokio::test]
    async fn stub_run_settles_layers_and_pends_without_baseline() {
        let engine = Arc::new(ScriptEngine::new(head_scripts(true)));
        let mut run = run(engine.as_ref(), &config(), options(true, 23))
            .await
            .expect("the stub run settles");
        assert_eq!(run.cases.len(), 23);
        assert_eq!(run.latency.count(), 23);

        let classify = &run.cases[0];
        assert_eq!(classify.id, "cls-word-01");
        assert_eq!(classify.kind, EvalKind::Classify);
        assert!(
            classify.quality.is_none(),
            "classify cases stay out of the judge"
        );
        assert_eq!(
            classify.contract.checks.get(&ContractCheck::KindMatch),
            Some(&true)
        );

        let task = run
            .cases
            .iter()
            .find(|case| case.id == "task-word-01")
            .expect("the head slice reaches the first task case");
        assert_eq!(
            task.contract.checks.get(&ContractCheck::FieldsComplete),
            Some(&true)
        );
        assert_eq!(
            task.quality.as_ref().map(|scores| scores.accuracy),
            Some(90),
            "the judge stub score rides the detail"
        );

        regression::settle(&mut run.cases, &GoldenMap::new(), &LabelMap::new());
        assert!(
            run.cases
                .iter()
                .all(|case| case.rule.as_ref().map(|rule| rule.verdict)
                    == Some(RuleVerdict::Pending)),
            "a first run without baselines stays pending"
        );
    }

    #[tokio::test]
    async fn golden_and_labels_settle_stub_rounds_into_pass_and_changed() {
        let engine = Arc::new(ScriptEngine::new(head_scripts(false)));
        let mut run = run(engine.as_ref(), &config(), options(false, 23))
            .await
            .expect("the stub run settles");
        let golden = GoldenMap::from([
            golden_row("cls-word-01", CLASSIFY_REPLY),
            golden_row("cls-word-02", OTHER_CATEGORY_REPLY),
            golden_row("task-word-01", WORD_REPLY),
        ]);
        let labels = LabelMap::from([
            label_row("cls-word-01", LabelVerdict::Pass),
            label_row("cls-word-02", LabelVerdict::Fail),
            label_row("task-word-01", LabelVerdict::Pass),
        ]);
        regression::settle(&mut run.cases, &golden, &labels);
        assert_eq!(verdict_of(&run, "cls-word-01"), RuleVerdict::Pass);
        assert_eq!(
            verdict_of(&run, "task-word-01"),
            RuleVerdict::Pass,
            "an identical structured reply passes against its baseline"
        );
        assert_eq!(
            verdict_of(&run, "cls-word-02"),
            RuleVerdict::Changed,
            "a failed label over a differing category counts as changed"
        );
    }

    #[tokio::test]
    async fn engine_failure_marks_the_case_and_drops_settlement() {
        let engine = Arc::new(ScriptEngine::new(vec![vec![Err(
            GlossError::EngineNetwork,
        )]]));
        let mut run = run(engine.as_ref(), &config(), options(true, 1))
            .await
            .expect("a per-case engine failure does not abort the run");
        assert_eq!(run.latency.count(), 1);
        assert!(run.cases[0].engine_error);
        assert!(run.cases[0].quality.is_none());
        assert!(
            run.cases[0].rounds.is_empty(),
            "a failed request leaves no settled round"
        );
        regression::settle(&mut run.cases, &GoldenMap::new(), &LabelMap::new());
        assert!(
            run.cases[0].rule.is_none(),
            "engine errors carry no regression verdict"
        );
    }

    #[tokio::test]
    async fn limit_slices_the_run_and_judge_toggle_keeps_quality_empty() {
        let engine = Arc::new(ScriptEngine::new(vec![script(CLASSIFY_REPLY)]));
        let run = run(engine.as_ref(), &config(), options(false, 2))
            .await
            .expect("the sliced run settles");
        assert_eq!(run.cases.len(), 2);
        assert_eq!(run.latency.count(), 2);
        assert!(
            run.cases.iter().all(|case| case.quality.is_none()),
            "a judgeless run keeps every quality field empty"
        );
    }

    #[test]
    fn latency_percentiles_interpolate() {
        let mut latency = Latency::default();
        assert_eq!(latency.percentile(50.0), None);
        for ms in [10.0, 20.0, 30.0, 40.0] {
            latency.record(ms);
        }
        assert_eq!(latency.percentile(50.0), Some(25.0));
        assert_eq!(latency.percentile(95.0), Some(38.5));
        assert_eq!(latency.percentile(0.0), Some(10.0));
        assert_eq!(latency.percentile(100.0), Some(40.0));
        assert_eq!(latency.count(), 4);
    }
}
