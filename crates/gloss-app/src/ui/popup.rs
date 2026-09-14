//! 浮层内容：按 [`TaskKind`] 分发的结果卡与流式/失败视图（M3-T9）。
//!
//! 词卡精排（音标/词性/释义/例句），其余任务展示 markdown 正文（M3 以
//! 可选中富文本呈现，语法级 markdown 渲染在需要时引入 egui_commonmark）；
//! OCR 另提供纯文本一键复制。流式视图按 [`STRUCTURED_FENCE`] 过滤未完成
//! 的结构化块——原始流里的 JSON 围栏不该闪现在用户面前。

use egui::{Color32, CornerRadius, Frame, Margin, RichText, ScrollArea, Stroke, vec2};
use gloss_core::prompt::STRUCTURED_FENCE;
use gloss_core::task::{OutcomeStructured, TaskKind};

use crate::app::OverlayView;

/// 浮层宽度（UI 规范 §2）
pub const WIDTH: f32 = 380.0;
/// 卡片圆角（UI 规范 §2）
const CORNER_RADIUS: u8 = 8;
/// 内容区内边距（UI 规范 §2）
const PADDING: i8 = 14;
/// 头部身份圆点：品牌珊瑚橙（UI 规范 §〇）
const BRAND_DOT: Color32 = Color32::from_rgb(0xD8, 0x5A, 0x30);
/// 流式正文/产物的展示字符上限（超出截断，浮层窗口固定）。
const MAX_BODY_CHARS: usize = 4000;

/// 画一帧浮层。根 `Ui` 覆盖整个窗口，卡片铺满它，圆角之外由透明窗口露出桌面。
///
/// `view` 为 `None` 时显示渲染自检卡（预热与自检路径）。
pub(crate) fn draw(ui: &mut egui::Ui, view: Option<&OverlayView>) {
    let fill = ui.visuals().window_fill;
    let stroke = ui.visuals().window_stroke;

    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(0.5, stroke.color))
        .corner_radius(CornerRadius::same(CORNER_RADIUS))
        .inner_margin(Margin::same(PADDING))
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            match view {
                None => {
                    header(ui, "自检", None);
                    ui.add_space(12.0);
                    selfcheck_body(ui);
                }
                Some(OverlayView::Streaming { source, body }) => {
                    header(ui, "推理中", None);
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(source)
                            .size(13.0)
                            .color(ui.visuals().weak_text_color()),
                    );
                    ui.add_space(8.0);
                    streamed_body(ui, body);
                }
                Some(OverlayView::Outcome(outcome)) => {
                    header(ui, kind_tag(outcome.kind), Some(copy_text(outcome)));
                    ui.add_space(12.0);
                    ScrollArea::vertical()
                        .auto_shrink(false)
                        .show(ui, |ui| outcome_body(ui, outcome));
                }
                Some(OverlayView::Failed { message }) => {
                    header(ui, "失败", None);
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(message.as_str())
                            .size(13.0)
                            .color(ui.visuals().warn_fg_color),
                    );
                }
            }
        });
}

/// 任务类型 → 头部标签。
fn kind_tag(kind: TaskKind) -> &'static str {
    match kind {
        TaskKind::TranslateWord => "词卡",
        TaskKind::TranslateSentence => "翻译",
        TaskKind::ExplainCode => "代码解释",
        TaskKind::ImageOcr => "提取结果",
        TaskKind::ImageExplain => "图片解释",
    }
}

/// 复制按钮写入剪贴板的文本：OCR 取纯文本，其余取 markdown 正文。
fn copy_text(outcome: &gloss_core::task::TaskOutcome) -> String {
    match &outcome.structured {
        OutcomeStructured::Extracted { text } => text.clone(),
        _ => outcome.body.clone(),
    }
}

/// 头部：身份圆点 + 标题；右侧状态位与可选的一键复制按钮。
fn header(ui: &mut egui::Ui, tag: &str, copy: Option<String>) {
    let weak = ui.visuals().weak_text_color();
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 4.0, BRAND_DOT);
        ui.label(RichText::new("翻译").size(12.0).color(weak));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(text) = copy
                && ui
                    .button(RichText::new("复制").size(11.0).color(weak))
                    .clicked()
            {
                ui.ctx().copy_text(text);
            }
            ui.label(RichText::new(tag).size(11.0).color(weak));
        });
    });
}

/// 流式正文：过滤掉已开始出现的结构化块（可能跨 chunk 切分，因此在
/// 累积文本上按最后一次围栏标记截断），可滚动查看。
fn streamed_body(ui: &mut egui::Ui, raw: &str) {
    let visible = match raw.rfind(STRUCTURED_FENCE) {
        Some(pos) => &raw[..pos],
        None => raw,
    };
    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        ui.label(
            RichText::new(truncate(visible))
                .size(14.0)
                .strong()
                .color(ui.visuals().strong_text_color()),
        );
    });
}

/// 产物正文：词卡精排，其余任务展示 markdown 正文/提取文本。
fn outcome_body(ui: &mut egui::Ui, outcome: &gloss_core::task::TaskOutcome) {
    match &outcome.structured {
        OutcomeStructured::WordCard {
            word,
            phonetic,
            senses,
        } => word_card(ui, word, phonetic.as_deref(), senses),
        OutcomeStructured::Plain { title } => {
            if let Some(title) = title {
                ui.label(
                    RichText::new(title.as_str())
                        .size(15.0)
                        .strong()
                        .color(ui.visuals().strong_text_color()),
                );
                ui.add_space(8.0);
            }
            selectable_body(ui, &outcome.body);
        }
        OutcomeStructured::Extracted { text } => selectable_body(ui, text),
    }
}

/// 词卡精排：词条 + 音标，按词性分组的释义与例句（UI 规范 §3.3 的最小
/// 落地；精排细节随 M4 真实数据调优）。
fn word_card(
    ui: &mut egui::Ui,
    word: &str,
    phonetic: Option<&str>,
    senses: &[gloss_core::task::Sense],
) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(word)
                .size(18.0)
                .strong()
                .color(ui.visuals().strong_text_color()),
        );
        if let Some(phonetic) = phonetic {
            ui.label(
                RichText::new(phonetic)
                    .size(13.0)
                    .color(ui.visuals().weak_text_color()),
            );
        }
    });
    ui.add_space(10.0);
    let strong = ui.visuals().strong_text_color();
    let weak = ui.visuals().weak_text_color();
    for sense in senses {
        ui.horizontal_wrapped(|ui| {
            if let Some(pos) = &sense.pos {
                ui.label(RichText::new(pos.as_str()).size(13.0).color(weak));
            }
            ui.label(
                RichText::new(sense.meaning.as_str())
                    .size(14.0)
                    .color(strong),
            );
        });
        for example in &sense.examples {
            ui.indent("example", |ui| {
                ui.label(RichText::new(format!("· {example}")).size(12.0).color(weak));
            });
        }
        ui.add_space(6.0);
    }
}

/// 可选中、自动换行的正文标签（超长截断；markdown 富渲染按需后续引入）。
fn selectable_body(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(
            RichText::new(truncate(text))
                .size(14.0)
                .color(ui.visuals().strong_text_color()),
        )
        .wrap()
        .selectable(true),
    );
}

/// 展示截断：超出上限的尾部以省略号收束（截断只发生在展示层）。
fn truncate(text: &str) -> String {
    if text.chars().count() <= MAX_BODY_CHARS {
        return text.to_owned();
    }
    let mut truncated: String = text.chars().take(MAX_BODY_CHARS).collect();
    truncated.push('…');
    truncated
}

/// 自检卡正文（预热与显隐自检路径）：中英混排一眼可辨字体链路健康。
fn selfcheck_body(ui: &mut egui::Ui) {
    let strong = ui.visuals().strong_text_color();
    let weak = ui.visuals().weak_text_color();
    ui.label(
        RichText::new("The quick brown fox jumps over the lazy dog.")
            .size(13.0)
            .color(weak),
    );
    ui.add_space(12.0);
    ui.label(
        RichText::new("敏捷的棕色狐狸从懒狗身上跳过。")
            .size(15.0)
            .strong()
            .color(strong),
    );
    ui.add_space(12.0);
    // 中英混排 + 中文标点：字体 fallback 链接没接上，一眼能看出来
    ui.label(
        RichText::new("中文渲染自检：划词翻译、代码解释、图片识别。")
            .size(13.0)
            .color(weak),
    );
}
