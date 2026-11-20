//! 完成态解析热点基准：模型回复到任务产物的落地解析，词卡少例与多例、
//! 长正文与纯文降级四档。基准框架（criterion）独立目标（自带进程入口），
//! 经基准配方运行；测量输入全部在测量循环外构造——字面量与字符串拼
//! 接，不经序列化库（serde）解析——经黑箱包装进出测量。

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use gloss_app::runtime::finalize::complete;
use gloss_core::task::TaskKind;

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

criterion_group!(benches, bench_complete);
criterion_main!(benches);
