//! gloss-core 热点基准：`cache_key` / `parse_structured` / `prompt_render` /
//! `moka_cache` 四组。criterion `harness = false` 目标，`just bench`
//! （`cargo bench --bench core`）运行。
//!
//! 测量输入全部在测量循环外构造（字面量 / 字符串拼接，不经 serde 解析），
//! 经 `black_box` 进出测量；`moka_cache` 的 `set` 用 `iter_batched` 把产物
//! 构造移出测量区间。

use std::hint::black_box;
use std::sync::Arc;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use gloss_core::cache::{MokaCache, cache_key};
use gloss_core::engine::parse_structured;
use gloss_core::model::ScreenRect;
use gloss_core::ports::Cache;
use gloss_core::prompt::PromptRegistry;
use gloss_core::task::{
    InputHint, OutcomeStructured, Sense, Task, TaskInput, TaskKind, TaskOptions, TaskOutcome,
};

const MODEL: &str = "mock-model";

fn text_task(kind: TaskKind, text: String, hint: Option<InputHint>) -> Task {
    Task {
        kind,
        input: TaskInput::Text { text, hint },
        options: TaskOptions::default(),
    }
}

fn image_task(png_bytes: usize) -> Task {
    Task {
        kind: TaskKind::ImageOcr,
        input: TaskInput::Image {
            png: Arc::from(vec![0x89u8; png_bytes]),
            region: ScreenRect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
        },
        options: TaskOptions::default(),
    }
}

fn word_outcome() -> TaskOutcome {
    TaskOutcome {
        kind: TaskKind::TranslateWord,
        body: "# gloss\n\n/ɡlɒs/ n. 光泽".into(),
        structured: OutcomeStructured::WordCard {
            word: "gloss".into(),
            phonetic: Some("/ɡlɒs/".into()),
            senses: vec![Sense {
                pos: Some("n.".into()),
                meaning: "光泽".into(),
                examples: vec!["a gloss of silk".into()],
            }],
        },
    }
}

fn senses_json(count: usize) -> String {
    (0..count)
        .map(|i| {
            format!(
                r#"{{"pos":"n.","meaning":"meaning {i}","examples":["example {i}a","example {i}b"]}}"#
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn word_card_body(sense_count: usize) -> String {
    let senses = senses_json(sense_count);
    format!(
        "**gloss**\n\n/ɡlɒs/ n. 光泽\n\n```gloss\n{{\"word\":\"gloss\",\"phonetic\":\"/ɡlɒs/\",\"senses\":[{senses}]}}\n```\n以上内容仅供参考"
    )
}

fn long_code() -> String {
    "let value = compute(input);\nif value > threshold {\n    return summarize(value);\n}\n\n"
        .repeat(80)
}

fn bench_cache_key(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_key");

    group.bench_function("text/short", |b| {
        let task = text_task(TaskKind::TranslateWord, "gloss".into(), None);
        b.iter(|| black_box(cache_key(black_box(&task), MODEL)))
    });
    group.bench_function("text/long", |b| {
        let task = text_task(
            TaskKind::ExplainCode,
            long_code(),
            Some(InputHint::CodeLanguage("rust".into())),
        );
        b.iter(|| black_box(cache_key(black_box(&task), MODEL)))
    });
    group.bench_function("image/1mb", |b| {
        let task = image_task(1024 * 1024);
        b.iter(|| black_box(cache_key(black_box(&task), MODEL)))
    });
    group.bench_function("image/4mb", |b| {
        let task = image_task(4 * 1024 * 1024);
        b.iter(|| black_box(cache_key(black_box(&task), MODEL)))
    });

    group.finish();
}

fn bench_parse_structured(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_structured");

    group.bench_function("word_card/ok", |b| {
        let body = word_card_body(3);
        b.iter(|| black_box(parse_structured(TaskKind::TranslateWord, black_box(&body))))
    });
    group.bench_function("word_card/many_senses", |b| {
        let body = word_card_body(20);
        b.iter(|| black_box(parse_structured(TaskKind::TranslateWord, black_box(&body))))
    });
    group.bench_function("fence_missing", |b| {
        let body = "plain markdown body without any structured fence. ".repeat(64);
        b.iter(|| {
            black_box(parse_structured(
                TaskKind::TranslateSentence,
                black_box(&body),
            ))
        })
    });
    group.bench_function("json_broken", |b| {
        let body = "markdown 段落\n```gloss\n{\"word\": \"gloss\", broken\n```";
        b.iter(|| black_box(parse_structured(TaskKind::ExplainCode, black_box(body))))
    });
    group.bench_function("ocr/ok", |b| {
        let text = "extracted ocr line ".repeat(80);
        let body = format!("markdown 段落\n```gloss\n{{\"text\":\"{text}\"}}\n```");
        b.iter(|| black_box(parse_structured(TaskKind::ImageOcr, black_box(&body))))
    });

    group.finish();
}

fn bench_prompt_render(c: &mut Criterion) {
    let mut group = c.benchmark_group("prompt_render");
    let registry = PromptRegistry;

    group.bench_function("word/short", |b| {
        let task = text_task(TaskKind::TranslateWord, "gloss".into(), None);
        b.iter(|| black_box(registry.render(black_box(&task)).ok()))
    });
    group.bench_function("code/long", |b| {
        let task = text_task(
            TaskKind::ExplainCode,
            long_code(),
            Some(InputHint::CodeLanguage("rust".into())),
        );
        b.iter(|| black_box(registry.render(black_box(&task)).ok()))
    });

    group.finish();
}

fn bench_moka_cache(c: &mut Criterion) {
    let mut group = c.benchmark_group("moka_cache");
    let cache = MokaCache::new();
    let hit_key = cache_key(
        &text_task(TaskKind::TranslateWord, "gloss".into(), None),
        MODEL,
    );
    let miss_key = cache_key(
        &text_task(TaskKind::TranslateWord, "never inserted".into(), None),
        MODEL,
    );
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
    bench_parse_structured,
    bench_prompt_render,
    bench_moka_cache
);
criterion_main!(benches);
