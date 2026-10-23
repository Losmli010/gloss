//! `gloss-core`/`gloss-app` 热点基准：`cache_key` / `complete`（完成态解析）/
//! `prompt_render` / `task_cache` 四组。`criterion`（`harness = false`）目标，
//! `just bench`（`cargo bench --bench core`）运行。
//!
//! 测量输入全部在测量循环外构造（字面量 / 字符串拼接，不经 `serde` 解析），
//! 经 `black_box` 进出测量；`task_cache` 的 `set` 用 `iter_batched` 把产物
//! 构造移出测量区间。

use std::hint::black_box;
use std::sync::Arc;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use gloss_app::runtime::cache::{TaskCache, cache_key};
use gloss_app::runtime::finalize::complete;
use gloss_core::prompt::PromptRegistry;
use gloss_core::task::{OutcomeStructured, Task, TaskInput, TaskKind, TaskOptions, TaskOutcome};

const MODEL: &str = "mock-model";

fn text_options(text: &str) -> (TaskInput, TaskOptions) {
    (
        TaskInput::Text {
            text: text.to_owned(),
        },
        TaskOptions {
            model: MODEL.into(),
            ..TaskOptions::default()
        },
    )
}

#[allow(clippy::expect_used)]
fn text_task(kind: TaskKind, text: String) -> Task {
    Task::new(
        kind,
        TaskInput::Text { text },
        TaskOptions {
            model: MODEL.into(),
            ..TaskOptions::default()
        },
    )
    .expect("the benchmark task is legal")
}

fn image_input(png_bytes: usize) -> TaskInput {
    TaskInput::Image {
        png: Arc::from(vec![0x89u8; png_bytes]),
    }
}

fn word_outcome() -> TaskOutcome {
    TaskOutcome {
        kind: TaskKind::TranslateWord,
        note: "# gloss\n\n/ɡlɒs/ n. 光泽".into(),
        structured: OutcomeStructured::WordCard {
            phonetic: Some("/ɡlɒs/".into()),
            examples: vec!["a gloss of silk".into()],
        },
        degraded: false,
    }
}

fn examples_json(count: usize) -> String {
    (0..count)
        .map(|i| format!(r#""例句 {i}：a gloss of silk example""#))
        .collect::<Vec<_>>()
        .join(",")
}

fn word_card_reply(example_count: usize) -> String {
    let examples = examples_json(example_count);
    format!(
        r#"{{"note":"**gloss** 的义释正文","structured":{{"phonetic":"/ɡlɒs/","examples":[{examples}]}}}}"#
    )
}

fn long_code() -> String {
    "let value = compute(input);\nif value > threshold {\n    return summarize(value);\n}\n\n"
        .repeat(80)
}

fn bench_cache_key(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_key");

    group.bench_function("text/short", |b| {
        let (input, options) = text_options("gloss");
        b.iter(|| black_box(cache_key(black_box(&input), black_box(&options))))
    });
    group.bench_function("text/long", |b| {
        let (input, options) = text_options(&long_code());
        b.iter(|| black_box(cache_key(black_box(&input), black_box(&options))))
    });
    group.bench_function("image/1mb", |b| {
        let input = image_input(1024 * 1024);
        let options = TaskOptions::default();
        b.iter(|| black_box(cache_key(black_box(&input), black_box(&options))))
    });
    group.bench_function("image/4mb", |b| {
        let input = image_input(4 * 1024 * 1024);
        let options = TaskOptions::default();
        b.iter(|| black_box(cache_key(black_box(&input), black_box(&options))))
    });

    group.finish();
}

fn bench_complete(c: &mut Criterion) {
    let mut group = c.benchmark_group("complete");

    group.bench_function("word_card/ok", |b| {
        let reply = word_card_reply(3);
        b.iter(|| black_box(complete(TaskKind::TranslateWord, black_box(&reply))))
    });
    group.bench_function("word_card/many_examples", |b| {
        let reply = word_card_reply(20);
        b.iter(|| black_box(complete(TaskKind::TranslateWord, black_box(&reply))))
    });
    group.bench_function("long_note/ok", |b| {
        let body = "plain markdown note body without any structure. ".repeat(64);
        let reply = format!(r#"{{"note":"{body}","structured":{{"examples":[]}}}}"#);
        b.iter(|| black_box(complete(TaskKind::TranslateSentence, black_box(&reply))))
    });
    group.bench_function("prose/degraded", |b| {
        let body = "plain markdown body that never was JSON. ".repeat(64);
        b.iter(|| black_box(complete(TaskKind::ExplainCode, black_box(&body))))
    });

    group.finish();
}

fn bench_prompt_render(c: &mut Criterion) {
    let mut group = c.benchmark_group("prompt_render");
    let registry = PromptRegistry;

    group.bench_function("word/short", |b| {
        let task = text_task(TaskKind::TranslateWord, "gloss".into());
        b.iter(|| black_box(registry.render(black_box(&task)).ok()))
    });
    group.bench_function("code/long", |b| {
        let task = text_task(TaskKind::ExplainCode, long_code());
        b.iter(|| black_box(registry.render(black_box(&task)).ok()))
    });

    group.finish();
}

fn bench_task_cache(c: &mut Criterion) {
    let mut group = c.benchmark_group("task_cache");
    let cache = TaskCache::new();
    let (hit_input, hit_options) = text_options("gloss");
    let hit_key = cache_key(&hit_input, &hit_options);
    let (miss_input, miss_options) = text_options("never inserted");
    let miss_key = cache_key(&miss_input, &miss_options);
    cache.set(hit_key, word_outcome());

    group.bench_function("get/hit", |b| {
        b.iter(|| black_box(cache.get(black_box(hit_key))))
    });
    group.bench_function("get/miss", |b| {
        b.iter(|| black_box(cache.get(black_box(miss_key))))
    });
    group.bench_function("set/same_key", |b| {
        b.iter_batched(
            word_outcome,
            |outcome| cache.set(hit_key, outcome),
            BatchSize::SmallInput,
        )
    });
    group.bench_function("set/new_key", |b| {
        let mut next_key = hit_key;
        b.iter_batched(
            word_outcome,
            |outcome| {
                next_key = next_key.wrapping_add(1);
                cache.set(next_key, outcome);
            },
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_cache_key,
    bench_complete,
    bench_prompt_render,
    bench_task_cache
);
criterion_main!(benches);
