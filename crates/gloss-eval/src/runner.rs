//! 评测执行器：重放轨与实跑轨（live）的编排，前者确定性且持续集成
//! （CI）安全，后者按需启用。
//!
//! 两条轨共享同一套评分器（指标模块）与数据集；差别只在回复的来源
//! ——重放轨取夹具目录里录制的修复轨迹（每轮增量），实跑轨经生产
//! 循环（分类与任务的逐轮执行）打真实大语言模型，并可选把轮次交回
//! 调用方落盘夹具、启用评审。违约类别与修复轮次两条轨同口径：逐轮
//! 跑生产校验器（核心层契约模块）折算。实跑轨的编排与端点装配在其
//! 子模块。

mod live;

pub use live::{LiveEnv, LiveOptions, LiveRun, live_config, run_live};

use std::collections::HashMap;

use gloss_core::classify::CLASSIFY_REPAIRS;
use gloss_core::engine::TASK_REPAIRS;
use gloss_core::schema;
use gloss_core::task::TaskKind;

use crate::EvalReport;
use crate::assets;
use crate::dataset::{ClassifyCase, Fixture, load_classify, load_fixtures, load_task};
use crate::detail::{self, CaseDetail};
use crate::metrics::{ClassifyMetrics, ClassifyVerdict, TaskMetrics, ViolationTrack};
use crate::report::{TrackOutputs, assemble_report};

#[derive(Debug, Clone)]
pub struct ReplayRun {
    pub report: EvalReport,
    pub cases: Vec<CaseDetail>,
}

pub fn run_replay() -> Result<EvalReport, String> {
    run_replay_detailed().map(|run| run.report)
}

pub fn run_replay_detailed() -> Result<ReplayRun, String> {
    let mut classify = ClassifyMetrics::default();
    let mut case_details = Vec::new();
    let mut skipped = 0usize;
    replay_classify(&mut classify, &mut case_details, &mut skipped)?;
    let mut tasks = Vec::new();
    replay_tasks(&mut tasks, &mut case_details, &mut skipped)?;

    Ok(ReplayRun {
        report: assemble_report(
            "replay",
            TrackOutputs {
                classify: Some(classify),
                tasks,
                latency: None,
                judge: None,
            },
            skipped,
        ),
        cases: case_details,
    })
}

fn replay_classify(
    classify: &mut ClassifyMetrics,
    case_details: &mut Vec<CaseDetail>,
    skipped: &mut usize,
) -> Result<(), String> {
    let classify_cases = load_classify(assets::CLASSIFY_DATASET)?;
    let classify_fixtures = load_fixtures(assets::CLASSIFY_FIXTURES)?;
    let by_id: HashMap<&str, &Fixture> = classify_fixtures
        .iter()
        .map(|fixture| (fixture.id.as_str(), fixture))
        .collect();
    for case in &classify_cases {
        let Some(fixture) = by_id.get(case.id.as_str()) else {
            *skipped += 1;
            continue;
        };
        let rounds = round_texts(fixture);
        check_classify_trajectory(&case.id, &rounds)?;
        case_details.push(score_classify_case(classify, case, &rounds));
    }
    Ok(())
}

fn replay_tasks(
    tasks: &mut Vec<(&'static str, TaskMetrics)>,
    case_details: &mut Vec<CaseDetail>,
    skipped: &mut usize,
) -> Result<(), String> {
    let task_fixtures = load_fixtures(assets::TASK_FIXTURES)?;
    let task_by_id: HashMap<&str, &Fixture> = task_fixtures
        .iter()
        .map(|fixture| (fixture.id.as_str(), fixture))
        .collect();
    for (name, dataset) in TASK_DATASETS {
        let cases = load_task(dataset)?;
        let mut metrics = TaskMetrics::default();
        for case in &cases {
            let Some(fixture) = task_by_id.get(case.id.as_str()) else {
                *skipped += 1;
                continue;
            };
            let rounds = round_texts(fixture);
            check_task_trajectory(&case.id, case.kind, &rounds)?;
            let (case_detail, verdict) = detail::task_parts(name, case, &rounds);
            metrics.record(&verdict);
            case_details.push(case_detail);
        }
        tasks.push((name, metrics));
    }
    Ok(())
}

fn score_classify_case(
    classify: &mut ClassifyMetrics,
    case: &ClassifyCase,
    rounds: &[String],
) -> CaseDetail {
    let (case_detail, verdict, track) = detail::classify_parts(case, rounds);
    absorb_classify_track(classify, &track);
    classify.evaluated += 1;
    match verdict {
        ClassifyVerdict::Correct => classify.correct += 1,
        ClassifyVerdict::Wrong(actual) => bump_confusion(classify, case.expected, actual),
        ClassifyVerdict::InvalidJson => classify.invalid_json += 1,
        ClassifyVerdict::Rejected => classify.rejected += 1,
    }
    case_detail
}

const TASK_DATASETS: [(&str, &str); 3] = [
    ("task_translate_word", assets::TASK_WORD_DATASET),
    ("task_translate_sentence", assets::TASK_SENTENCE_DATASET),
    ("task_explain_code", assets::TASK_CODE_DATASET),
];

fn round_texts(fixture: &Fixture) -> Vec<String> {
    fixture
        .rounds
        .iter()
        .map(|deltas| deltas.concat())
        .collect()
}

fn check_classify_trajectory(id: &str, rounds: &[String]) -> Result<(), String> {
    if rounds.len() as u32 > 1 + CLASSIFY_REPAIRS {
        return Err(format!(
            "classify fixture {id}: {} rounds exceed the production budget \
             (1 + CLASSIFY_REPAIRS)",
            rounds.len()
        ));
    }
    for (index, round) in rounds[..rounds.len() - 1].iter().enumerate() {
        if schema::validate_classify(round).is_ok() {
            return Err(format!(
                "classify fixture {id}: round {} is contract-valid but a \
                 repair round follows",
                index + 1
            ));
        }
    }
    Ok(())
}

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

fn absorb_classify_track(metrics: &mut ClassifyMetrics, track: &ViolationTrack) {
    for (violation, count) in &track.violations {
        *metrics.violations.entry(*violation).or_default() += count;
    }
    metrics.repair_rounds += track.repairs;
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use futures::Stream;
    use gloss_core::config_handle::ConfigHandle;
    use gloss_core::ports::{AiEngine, BoxFuture, ConfigStore, TaskChunk, TaskStream};
    use gloss_core::schema::Violation;

    use super::live::run_task_case;
    use super::{
        check_classify_trajectory, check_task_trajectory, run_replay, run_replay_detailed,
    };
    use crate::EvalReport;
    use crate::dataset::load_task;
    use crate::detail::{CLASSIFY_GROUP, ClassifyOutcome};
    use crate::metrics::{ClassifyMetrics, TaskMetrics};
    use crate::report::TaskStats;
    use crate::runner::{LiveEnv, LiveOptions, LiveRun, live_config, run_live};

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
        assert_pinned_classify_violations(&classify.metrics);
        assert_repair_rounds_ride_violations(&report.tasks);
        let word = dataset_metrics(&report, "task_translate_word", "word dataset");
        assert_eq!(
            word.violations.get(&Violation::EmptyReply),
            Some(&1),
            "the word dataset carries the empty-reply exception fixture"
        );
        let sentence = dataset_metrics(&report, "task_translate_sentence", "sentence dataset");
        assert!(
            sentence.degraded > 0,
            "the sentence dataset carries a budget-exhausted exception fixture"
        );
        let code = dataset_metrics(&report, "task_explain_code", "code dataset");
        assert_eq!(
            code.violations.get(&Violation::MissingNote),
            Some(&1),
            "the code dataset carries the missing-note exception fixture"
        );
    }

    #[test]
    fn replay_details_agree_with_the_report_metrics() {
        let run = run_replay_detailed().expect("replay must succeed offline");
        let classify = run.report.classify.as_ref().expect("classify track");
        let classify_rows: Vec<_> = run
            .cases
            .iter()
            .filter(|case| case.group == CLASSIFY_GROUP)
            .collect();
        assert_eq!(
            classify_rows.len(),
            classify.metrics.evaluated,
            "classify details and the report share the same denominator"
        );
        assert_eq!(
            classify_rows
                .iter()
                .filter(|case| case.classify.as_ref() == Some(&ClassifyOutcome::Correct))
                .count(),
            classify.metrics.correct
        );
        assert_eq!(
            classify_rows.iter().map(|case| case.repairs).sum::<u32>(),
            classify.metrics.repair_rounds
        );
        for stats in &run.report.tasks {
            let rows: Vec<_> = run
                .cases
                .iter()
                .filter(|case| case.group == stats.dataset)
                .collect();
            assert_eq!(rows.len(), stats.metrics.evaluated, "{}", stats.dataset);
            assert_eq!(
                rows.iter()
                    .filter(|case| case.task.as_ref().is_some_and(|outcome| outcome.degraded))
                    .count(),
                stats.metrics.degraded,
                "{}",
                stats.dataset
            );
            assert_eq!(
                rows.iter().map(|case| case.repairs).sum::<u32>(),
                stats.metrics.repair_rounds,
                "{}",
                stats.dataset
            );
        }
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
        let engine = scripted_engine(
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
        );
        let (config, _store) = live_test_config();
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
        assert_live_report_and_details(&run);
        assert_live_recordings(&run);
    }

    #[test]
    fn a_task_engine_failure_still_counts_one_degraded_case() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime should build");
        let engine = scripted_engine(
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
        );
        let (config, _store) = live_test_config();
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
        let failed = run
            .cases
            .iter()
            .find(|case| case.engine_error)
            .expect("the failed task rides the details as an engine error");
        assert_eq!(failed.id, "task-word-01");
        assert_eq!(failed.group, "task_translate_word");
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
        let engine = scripted_engine(
            vec![
                vec![chunk("先来半截散文"), chunk("再补一段")],
                vec![
                    chunk("{\"note\":\"义\","),
                    chunk("\"structured\":{\"examples\":[]}}"),
                ],
            ],
            Arc::new(AtomicUsize::new(0)),
        );
        let (config, _store) = live_test_config();
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

    fn dataset_metrics<'a>(report: &'a EvalReport, dataset: &str, note: &str) -> &'a TaskMetrics {
        let stats = report
            .tasks
            .iter()
            .find(|stats| stats.dataset == dataset)
            .expect(note);
        &stats.metrics
    }

    fn assert_pinned_classify_violations(metrics: &ClassifyMetrics) {
        for violation in [
            Violation::NotJson,
            Violation::KindNotAllowed,
            Violation::EmptyReply,
        ] {
            assert!(
                metrics.violations.contains_key(&violation),
                "{violation:?}: the exception fixtures pin prose, off-list-kind \
                 and empty-reply violations: {:?}",
                metrics.violations
            );
        }
    }

    fn assert_repair_rounds_ride_violations(tasks: &[TaskStats]) {
        for stats in tasks {
            assert!(
                stats.metrics.repair_rounds > 0 || stats.metrics.violations.is_empty(),
                "{}: violation counts ride repaired trajectories",
                stats.dataset
            );
        }
    }

    fn live_test_config() -> (Arc<ConfigHandle>, Arc<dyn ConfigStore>) {
        live_config(LiveEnv {
            base_url: "http://localhost".into(),
            model: "m".into(),
            api_key: String::new(),
        })
    }

    fn scripted_engine(
        script: Vec<Vec<Result<TaskChunk, gloss_core::model::GlossError>>>,
        calls: Arc<AtomicUsize>,
    ) -> Arc<dyn AiEngine> {
        Arc::new(ScriptEngine::new(script, calls))
    }

    fn assert_live_report_and_details(run: &LiveRun) {
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

        assert_eq!(
            run.cases.len(),
            4,
            "1 classify + 3 task details ride the same run"
        );
        assert_eq!(run.cases[0].id, "zh-word-01");
        assert!(run.cases[0].classify.is_some());
        assert!(run.cases.iter().all(|case| !case.engine_error));
    }

    fn assert_live_recordings(run: &LiveRun) {
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
