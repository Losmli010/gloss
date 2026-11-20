//! 提示词渲染热点基准：提示词注册表按任务渲染整段提示词文本，词卡短
//! 词与代码长文两档。基准框架（criterion）独立目标（自带进程入口），经
//! 基准配方运行；测量输入全部在测量循环外构造——字面量与字符串拼接，
//! 不经序列化库（serde）解析——经黑箱包装进出测量。

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use gloss_core::prompt::PromptRegistry;
use gloss_core::task::{Task, TaskInput, TaskKind, TaskOptions};

const MODEL: &str = "mock-model";

fn long_code() -> String {
    "let value = compute(input);\nif value > threshold {\n    return summarize(value);\n}\n\n"
        .repeat(80)
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

criterion_group!(benches, bench_prompt_render);
criterion_main!(benches);
