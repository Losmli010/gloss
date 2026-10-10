//! clone 热点基准：criterion 自定义测量，按「分配次数 / 分配字节」两组
//! 计量同一批热点（状态机任务周期、文案表错误映射、设置窗一帧、浮层
//! 一帧）。分配是 clone 的直接开销通道，计数比耗时稳定；耗时量化仍归
//! benches/core.rs 与 criterion 的墙钟机制。
//!
//! 除 `machine/lifecycle`（输入构造计入测量）外，测量输入全部在循环外
//! 构造，经 `black_box` 进出测量。UI 帧经 `ui::context::new_context`
//! 装入生产同款字体/主题后无头驱动：预热期推进时间越过淡入动画，测量
//! 期时间冻结——时间相关的缓存淘汰不再触发，逐帧分配恒定。

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;

use criterion::measurement::{Measurement, ValueFormatter};
use criterion::{BenchmarkGroup, Criterion, criterion_group, criterion_main};
use egui::{Context, RawInput};
use gloss_app::channel::{AcquireCommand, PlatformEvent};
use gloss_app::machine::{OverlayView, TaskStateMachine};
use gloss_app::ui::context;
use gloss_app::ui::i18n::Text;
use gloss_app::ui::popup::{self, RenderState};
use gloss_app::ui::settings;
use gloss_app::update::state::UpdateState;
use gloss_core::config::{Config, Theme};
use gloss_core::guard::SceneFacts;
use gloss_core::model::{GlossError, Locale, ScreenPoint};
use gloss_core::task::{OutcomeStructured, TaskInput, TaskKind, TaskOutcome};

const SOURCE: &str = "The quick brown fox jumps over the lazy dog near the river bank at dusk.";
const CHUNK: &str = "{\"word\":\"gloss\"}";
const ENGINE_DETAIL: &str = "upstream connect error";
const NOTE: &str = "**gloss**\n\n/ɡlɒs/ n. 光泽\n\n```gloss\n{\"word\":\"gloss\",\"phonetic\":\"/ɡlɒs/\",\"senses\":[{\"pos\":\"n.\",\"meaning\":\"光泽\"}]}\n```\n以上内容仅供参考";
const WARMUP_FRAMES: usize = 40;
const STEADY_TIME: f64 = 100.0;

thread_local! {
    static ALLOC_COUNT: Cell<u64> = const { Cell::new(0) };
    static ALLOC_BYTES: Cell<u64> = const { Cell::new(0) };
}

fn bump(size: usize) {
    let _count = ALLOC_COUNT.try_with(|c| c.set(c.get() + 1));
    let _bytes = ALLOC_BYTES.try_with(|b| b.set(b.get() + size as u64));
}

struct CountingAlloc;

// SAFETY: 分配语义原样转发 System 分配器，本实现只在成功路径旁路累加
// 本线程计数，不改变指针、布局与对齐契约。
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: 前置条件（指针有效、布局配对）与 System 契约一致，原样转发。
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            bump(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: 指针与布局来自调用方的配对契约，原样转发 System。
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static COUNTING_ALLOC: CountingAlloc = CountingAlloc;

struct UnitFormatter(&'static str);

impl ValueFormatter for UnitFormatter {
    fn scale_values(&self, _typical: f64, _values: &mut [f64]) -> &'static str {
        self.0
    }

    fn scale_throughputs(
        &self,
        _typical: f64,
        _throughput: &criterion::Throughput,
        _values: &mut [f64],
    ) -> &'static str {
        self.0
    }

    fn scale_for_machines(&self, _values: &mut [f64]) -> &'static str {
        self.0
    }
}

static COUNT_FORMAT: UnitFormatter = UnitFormatter("allocs");
static BYTES_FORMAT: UnitFormatter = UnitFormatter("bytes");

#[derive(Clone, Copy, Default)]
struct AllocCount;

impl Measurement for AllocCount {
    type Intermediate = ();
    type Value = u64;

    fn start(&self) {
        ALLOC_COUNT.with(|c| c.set(0));
    }

    fn end(&self, (): Self::Intermediate) -> Self::Value {
        ALLOC_COUNT.with(Cell::get)
    }

    fn add(&self, v1: &Self::Value, v2: &Self::Value) -> Self::Value {
        v1 + v2
    }

    fn zero(&self) -> Self::Value {
        0
    }

    fn to_f64(&self, value: &Self::Value) -> f64 {
        *value as f64
    }

    fn formatter(&self) -> &dyn ValueFormatter {
        &COUNT_FORMAT
    }
}

#[derive(Clone, Copy, Default)]
struct AllocBytes;

impl Measurement for AllocBytes {
    type Intermediate = ();
    type Value = u64;

    fn start(&self) {
        ALLOC_BYTES.with(|b| b.set(0));
    }

    fn end(&self, (): Self::Intermediate) -> Self::Value {
        ALLOC_BYTES.with(Cell::get)
    }

    fn add(&self, v1: &Self::Value, v2: &Self::Value) -> Self::Value {
        v1 + v2
    }

    fn zero(&self) -> Self::Value {
        0
    }

    fn to_f64(&self, value: &Self::Value) -> f64 {
        *value as f64
    }

    fn formatter(&self) -> &dyn ValueFormatter {
        &BYTES_FORMAT
    }
}

fn word_outcome() -> TaskOutcome {
    TaskOutcome {
        kind: TaskKind::TranslateWord,
        note: NOTE.to_owned(),
        code_language: None,
        structured: OutcomeStructured::WordCard {
            phonetic: Some("/ɡlɒs/".to_owned()),
            examples: vec!["a gloss of silk".to_owned()],
        },
    }
}

fn run_frame(ctx: &Context, time: f64, draw: impl FnMut(&mut egui::Ui)) {
    let input = RawInput {
        time: Some(time),
        ..RawInput::default()
    };
    let output = ctx.run_ui(input, draw);
    output.drop_without_applying_deltas();
}

fn bench_machine_lifecycle<M: Measurement>(group: &mut BenchmarkGroup<'_, M>) {
    let mut machine = TaskStateMachine::new();
    let event = PlatformEvent::SelectionGesture {
        pos: ScreenPoint { x: 120, y: 96 },
    };
    let config = Config::default();
    let scene = SceneFacts::default();

    group.bench_function("machine/lifecycle", |b| {
        b.iter(|| {
            let input = TaskInput::Text {
                text: SOURCE.to_owned(),
            };
            let delta = CHUNK.to_owned();
            let outcome = word_outcome();
            let Some(AcquireCommand::AcquireText { generation }) = machine.begin_selection_probe(
                black_box(&event),
                black_box(&config),
                Locale::Zh,
                black_box(&scene),
            ) else {
                return;
            };
            black_box(machine.commit_probe(generation, input));
            black_box(machine.accept_classified(generation, TaskKind::TranslateWord));
            black_box(machine.accept_chunk(generation, delta));
            black_box(machine.accept_done(generation, outcome));
            machine.hide_overlay();
        })
    });
}

fn bench_i18n<M: Measurement>(group: &mut BenchmarkGroup<'_, M>) {
    let text = Text::get(Locale::Zh);
    let plain = GlossError::EngineNetwork;
    let detail = GlossError::EngineResponse(ENGINE_DETAIL.to_owned());

    group.bench_function("i18n/for_error", |b| {
        b.iter(|| black_box(text.for_error(black_box(&plain))))
    });
    group.bench_function("i18n/for_error_fill", |b| {
        b.iter(|| black_box(text.for_error(black_box(&detail))))
    });
    group.bench_function("i18n/for_error_detail", |b| {
        b.iter(|| black_box(text.for_error_detail(black_box(&detail))))
    });
}

fn bench_settings_frame<M: Measurement>(group: &mut BenchmarkGroup<'_, M>) {
    let ctx = context::new_context(Theme::System);
    let config = Config::default();
    let mut state = settings::open(&config);
    let update = UpdateState::default();
    let text = Text::get(Locale::Zh);

    for i in 0..WARMUP_FRAMES {
        run_frame(&ctx, i as f64 / 30.0, |ui| {
            settings::draw(ui, &mut state, &update, text);
        });
    }
    group.bench_function("ui/settings_frame", |b| {
        b.iter(|| {
            run_frame(&ctx, STEADY_TIME, |ui| {
                black_box(settings::draw(ui, &mut state, &update, text));
            });
        })
    });
}

fn bench_popup_frame<M: Measurement>(group: &mut BenchmarkGroup<'_, M>) {
    let ctx = context::new_context(Theme::System);
    let render = RenderState::default();
    let text = Text::get(Locale::Zh);
    let view = OverlayView::Outcome {
        source: SOURCE.to_owned(),
        outcome: word_outcome(),
        code_lang: None,
    };

    for i in 0..WARMUP_FRAMES {
        run_frame(&ctx, i as f64 / 30.0, |ui| {
            popup::draw(ui, Some(&view), None, &render, text);
        });
    }
    group.bench_function("ui/popup_frame", |b| {
        b.iter(|| {
            run_frame(&ctx, STEADY_TIME, |ui| {
                black_box(popup::draw(ui, Some(&view), None, &render, text));
            });
        })
    });
}

fn hotspots<M: Measurement>(group: &mut BenchmarkGroup<'_, M>) {
    bench_machine_lifecycle(group);
    bench_i18n(group);
    bench_settings_frame(group);
    bench_popup_frame(group);
}

fn bench_allocs(c: &mut Criterion<AllocCount>) {
    let mut group = c.benchmark_group("clone_allocs");
    hotspots(&mut group);
    group.finish();
}

fn bench_bytes(c: &mut Criterion<AllocBytes>) {
    let mut group = c.benchmark_group("clone_bytes");
    hotspots(&mut group);
    group.finish();
}

criterion_group! {
    name = allocs;
    config = Criterion::default().with_measurement(AllocCount);
    targets = bench_allocs
}
criterion_group! {
    name = bytes;
    config = Criterion::default().with_measurement(AllocBytes);
    targets = bench_bytes
}
criterion_main!(allocs, bytes);
