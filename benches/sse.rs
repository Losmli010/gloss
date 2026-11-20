//! 流式事件解码热点基准：开放接口（OpenAI）方言的增量解码器整流喂字
//! 节，整流单块、定长分块、逐字节三档，再加单行越界一档。解码器住库
//! 内私有实现，基准目标以路径属性把生产源文件单源并入——与测试桩的
//! 同源机制一致，测到的与线上跑的是同一份源码；测量输入全部在测量循
//! 环外构造，经黑箱包装进出测量；解码器有状态，随测量新建无法外提。

#[allow(dead_code)]
#[path = "../crates/gloss-llm/src/openai/sse.rs"]
mod sse;

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use sse::SseDecoder;

const EVENTS: usize = 128;

fn stream(events: usize) -> String {
    let mut text = String::with_capacity(events * 72 + 96);
    for _ in 0..events {
        text.push_str("data: {\"choices\":[{\"delta\":{\"content\":\"光泽\"},\"index\":0}]}\n\n");
    }
    text.push_str("data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n");
    text.push_str("data: [DONE]\n\n");
    text
}

fn bench_sse_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("sse_decode");
    let whole = stream(EVENTS);

    group.bench_function("stream/one_push", |b| {
        b.iter(|| {
            let mut decoder = SseDecoder::new();
            black_box(decoder.push(black_box(whole.as_bytes())))
        })
    });

    for (name, size) in [("stream/chunk_64b", 64), ("stream/chunk_1b", 1)] {
        group.bench_function(name, |b| {
            b.iter(|| {
                let mut decoder = SseDecoder::new();
                for chunk in whole.as_bytes().chunks(size) {
                    black_box(decoder.push(black_box(chunk)));
                }
                black_box(decoder.finish())
            })
        });
    }

    group.bench_function("line/limit_breach", |b| {
        let flood = vec![b'a'; (1 << 20) + 1];
        b.iter(|| {
            let mut decoder = SseDecoder::new();
            black_box(decoder.push(black_box(&flood)))
        })
    });

    group.finish();
}

criterion_group!(benches, bench_sse_decode);
criterion_main!(benches);
