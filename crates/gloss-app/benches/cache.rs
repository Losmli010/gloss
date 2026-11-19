//! 任务缓存热点基准：缓存键派生与缓存端口吞吐两组。键派生覆盖短词、
//! 长文与两档图像字节，缓存吞吐覆盖命中、未命中、同键覆盖与新键驱逐。
//! 基准框架（criterion）独立目标（自带进程入口），经基准配方运行；测
//! 量输入全部在测量循环外构造——字面量与字符串拼接，不经序列化库
//! （serde）解析——经黑箱包装进出测量，缓存写入一组的产物构造用批处
//! 理助手移出测量区间。

use std::hint::black_box;
use std::sync::Arc;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use gloss_app::runtime::cache::{TaskCache, cache_key};
use gloss_core::task::{OutcomeStructured, TaskInput, TaskKind, TaskOptions, TaskOutcome};

const MODEL: &str = "mock-model";

fn long_code() -> String {
    "let value = compute(input);\nif value > threshold {\n    return summarize(value);\n}\n\n"
        .repeat(80)
}

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
        lang_violations: Vec::new(),
    }
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

criterion_group!(benches, bench_cache_key, bench_task_cache);
criterion_main!(benches);
