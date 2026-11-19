//! 语言门判定热点基准：输出语言体例门禁对整卡产物做全量判定，词卡干
//! 净通过、长句原文回引、长句正文与代码容器四档。判定器是纯函数，测
//! 量输入全部在测量循环外以字面量构造，经黑箱包装进出测量；基准框架
//! （criterion）独立目标（自带进程入口），经基准配方运行。

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use gloss_core::model::Lang;
use gloss_core::pipeline::lang_gate;
use gloss_core::task::{OutcomeStructured, TaskInput, TaskKind, TaskOutcome};

const SOURCE_WORD: &str = "serendipity";

const SOURCE_PASSAGE: &str = "The committee decided that the proposal should be reviewed by \
every member before the final vote on the budget";

const LONG_SOURCE: &str = "The negotiation continued through the night as both delegations \
exchanged revised drafts of the agreement, each round narrowing the remaining disagreements \
about scheduling, verification, and the timeline for withdrawal.";

const SOURCE_SNIPPET: &str = "fn summarize(values: &[f64]) -> f64 {\n    let total: f64 = \
values.iter().sum();\n    let mean = total / values.len() as f64;\n    mean\n}\n";

fn text_case(
    kind: TaskKind,
    text: &str,
    note: &str,
    structured: OutcomeStructured,
) -> (TaskInput, TaskOutcome) {
    (
        TaskInput::Text {
            text: text.to_owned(),
        },
        TaskOutcome {
            kind,
            note: note.to_owned(),
            structured,
            degraded: false,
            lang_violations: Vec::new(),
        },
    )
}

fn plain(examples: &[&str]) -> OutcomeStructured {
    OutcomeStructured::Plain {
        examples: examples.iter().map(|s| (*s).to_owned()).collect(),
    }
}

fn word_card(phonetic: &str, examples: &[&str]) -> OutcomeStructured {
    OutcomeStructured::WordCard {
        phonetic: Some(phonetic.to_owned()),
        examples: examples.iter().map(|s| (*s).to_owned()).collect(),
    }
}

fn code(examples: &[&str]) -> OutcomeStructured {
    OutcomeStructured::Code {
        examples: examples.iter().map(|s| (*s).to_owned()).collect(),
        code_language: None,
    }
}

fn clean_word_card() -> (TaskInput, TaskOutcome) {
    text_case(
        TaskKind::TranslateWord,
        SOURCE_WORD,
        "不期而遇的机缘，指偶然间发现的幸运事物。",
        word_card("/ˌserənˈdɪpəti/", &["意外的发现", "机缘巧合"]),
    )
}

fn overlapping_sentence() -> (TaskInput, TaskOutcome) {
    text_case(
        TaskKind::TranslateSentence,
        SOURCE_PASSAGE,
        "原文陈述：The committee decided that the proposal should be reviewed by every \
member before the final vote on the budget。据此译出下句。",
        plain(&["预算终表决前须由每一名成员审阅", "提案先审后表决"]),
    )
}

fn long_sentence() -> (TaskInput, TaskOutcome) {
    text_case(
        TaskKind::TranslateSentence,
        LONG_SOURCE,
        "谈判持续了一整夜，双方代表团就修订后的协定草案逐轮交换意见，每一轮都让日程安排、\
核验方式与撤离时刻表上的剩余分歧进一步收窄，直到天亮才勉强接近可签署的文本。",
        plain(&["双方彻夜磋商", "分歧逐轮收窄", "文本接近可签署"]),
    )
}

fn code_containers() -> (TaskInput, TaskOutcome) {
    text_case(
        TaskKind::ExplainCode,
        SOURCE_SNIPPET,
        "这段代码先对切片求和得到总量，再除以元素个数算出均值，最后把均值作为返回值交还\
调用方：\n\n```text\nfn summarize(values: &[f64]) -> f64 {\n    let total: f64 = \
values.iter().sum();\n    let mean = total / values.len() as f64;\n    mean\n}\n```\n\n\
除数为零时按浮点规则得到无穷值，调用方需自行保证切片非空。",
        code(&[]),
    )
}

fn bench_lang_gate(c: &mut Criterion) {
    let mut group = c.benchmark_group("lang_gate");

    group.bench_function("word_card/clean", |b| {
        let (input, result) = clean_word_card();
        b.iter(|| {
            black_box(lang_gate::check(
                TaskKind::TranslateWord,
                &Lang::Zh,
                black_box(&input),
                black_box(&result),
            ))
        })
    });
    group.bench_function("sentence/overlap", |b| {
        let (input, result) = overlapping_sentence();
        b.iter(|| {
            black_box(lang_gate::check(
                TaskKind::TranslateSentence,
                &Lang::Zh,
                black_box(&input),
                black_box(&result),
            ))
        })
    });
    group.bench_function("sentence/long", |b| {
        let (input, result) = long_sentence();
        b.iter(|| {
            black_box(lang_gate::check(
                TaskKind::TranslateSentence,
                &Lang::Zh,
                black_box(&input),
                black_box(&result),
            ))
        })
    });
    group.bench_function("code/containers", |b| {
        let (input, result) = code_containers();
        b.iter(|| {
            black_box(lang_gate::check(
                TaskKind::ExplainCode,
                &Lang::Zh,
                black_box(&input),
                black_box(&result),
            ))
        })
    });

    group.finish();
}

criterion_group!(benches, bench_lang_gate);
criterion_main!(benches);
