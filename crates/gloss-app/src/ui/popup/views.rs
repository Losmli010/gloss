//! 浮层视图分发与卡骨架：按视图变体选卡（自检、流式、降级
//! 产物、产物、失败），平铺/滚动骨架在此拼装；经位与注疏的正文排布委托
//! 经注疏排布模块，页头页脚印章等固定件委托卡片固定件模块。
//!
//! 骨架条（「经显注未至」的占位脉动条）也在这里：它是流式正文的占位
//! 形态，与页脚呼吸点同源脉动。

use std::borrow::Cow;
use std::time::Duration;

use egui::{RichText, ScrollArea, vec2};

use super::card::{footer, header};
use super::content::ZHU_FONT;
use super::note::{StreamFields, stream_fields};
use super::outcome::{
    apply_markdown_typography, attached_image, is_code, jing_section, outcome_body,
    render_markdown, selfcheck_body, shu_examples, source_block, zhu_section,
};
use super::{OverlayAction, PULSE_PERIOD_SECS, RenderCtx};
use crate::machine::{ErrorAction, FailureCause, OverlayView};
use crate::ui::i18n::Text;
use crate::ui::style::{font, space};

const MIN_BODY_VIEWPORT: f32 = 48.0;
const FOOTER_RESERVE: f32 = super::FOOTER_HEIGHT + space::PARAGRAPH;
const ARROW_SCROLL_STEP: f32 = 48.0;
const SHIMMER_BAR_HEIGHT: f32 = 12.0;
const SHIMMER_BAR_RADIUS: u8 = 4;
const SHIMMER_ALPHA_MIN: u8 = 38;
const SHIMMER_ALPHA_MAX: u8 = 96;

fn record_scrolled_height(
    ui: &egui::Ui,
    content_h: &mut f32,
    scrolled: &egui::scroll_area::ScrollAreaOutput<()>,
) {
    let overflow = (scrolled.content_size.y - scrolled.inner_rect.height()).max(0.0);
    *content_h = ui.min_rect().height() + overflow;
}

fn viewport_max(ui: &egui::Ui) -> f32 {
    (ui.available_height() - FOOTER_RESERVE).max(MIN_BODY_VIEWPORT)
}

fn arrow_key_scroll(ui: &egui::Ui) {
    let step = ui.input(|input| {
        if !input.modifiers.any() {
            if input.key_pressed(egui::Key::ArrowDown) {
                return Some(-ARROW_SCROLL_STEP);
            }
            if input.key_pressed(egui::Key::ArrowUp) {
                return Some(ARROW_SCROLL_STEP);
            }
        }
        None
    });
    if let Some(dy) = step {
        ui.scroll_with_delta(egui::vec2(0.0, dy));
    }
}

fn shimmer_bars(ui: &mut egui::Ui) {
    let now = ui.input(|i| i.time);
    let ratios = [0.95, 0.78, 0.6];
    let full = ui.available_width();
    let gap = space::ITEM;
    let total_h = ratios.len() as f32 * SHIMMER_BAR_HEIGHT + (ratios.len() - 1) as f32 * gap;
    let (rect, _response) = ui.allocate_exact_size(vec2(full, total_h), egui::Sense::hover());
    let weak = ui.visuals().weak_text_color();
    for (index, ratio) in ratios.iter().enumerate() {
        let phase = (index as f64) * 0.9;
        let wave = ((now / PULSE_PERIOD_SECS + phase).sin() + 1.0) / 2.0;
        let alpha =
            SHIMMER_ALPHA_MIN as f64 + wave * f64::from(SHIMMER_ALPHA_MAX - SHIMMER_ALPHA_MIN);
        let alpha = (alpha as f32).round().clamp(0.0, 255.0) as u8;
        let bar = egui::Rect::from_min_size(
            egui::pos2(
                rect.min.x,
                rect.min.y + index as f32 * (SHIMMER_BAR_HEIGHT + gap),
            ),
            vec2(full * ratio, SHIMMER_BAR_HEIGHT),
        );
        ui.painter().rect_filled(
            bar,
            SHIMMER_BAR_RADIUS,
            egui::Color32::from_rgba_unmultiplied(weak.r(), weak.g(), weak.b(), alpha),
        );
    }
    ui.ctx()
        .request_repaint_after(Duration::from_secs_f32(0.016));
}

fn streamed_sections(ui: &mut egui::Ui, ctx: &RenderCtx<'_>, fields: &StreamFields) {
    if fields.note.is_empty() && fields.examples.is_empty() {
        shimmer_bars(ui);
        return;
    }
    if !fields.note.is_empty() {
        zhu_section(ui, ctx.text, |ui| {
            apply_markdown_typography(ui, ZHU_FONT);
            render_markdown(ui, ctx.state, &fields.note);
        });
    }
    if !fields.examples.is_empty() {
        ui.add_space(space::PARAGRAPH);
        shu_examples(ui, ctx, &fields.examples);
    }
}

fn streamed_body(
    ui: &mut egui::Ui,
    ctx: &RenderCtx<'_>,
    fields: &StreamFields,
    jing: impl FnOnce(&mut egui::Ui),
) {
    jing_section(ui, ctx.text, jing);
    ui.add_space(space::PARAGRAPH);
    streamed_sections(ui, ctx, fields);
}

pub(super) fn render_content(
    ui: &mut egui::Ui,
    ctx: &RenderCtx<'_>,
    view: Option<&OverlayView>,
    content_h: &mut f32,
) -> (Option<OverlayAction>, Option<egui::Vec2>) {
    match view {
        None => plain_card(ui, ctx, content_h, |ui, _| selfcheck_body(ui)),
        Some(OverlayView::Streaming {
            source,
            raw,
            classified,
            repairing,
        }) => {
            let fields = stream_fields(raw);
            let code = is_code(*classified);
            let mode = FooterMode::Streaming {
                repairing: *repairing,
            };
            match ctx.attached {
                Some(png) => scrolled_card(ui, ctx, content_h, mode, |ui| {
                    streamed_body(ui, ctx, &fields, |ui| attached_image(ui, ctx, png));
                }),
                None => scrolled_card(ui, ctx, content_h, mode, |ui| {
                    streamed_body(ui, ctx, &fields, |ui| source_block(ui, source, code, None));
                }),
            }
        }
        Some(OverlayView::Outcome { source: _, outcome }) if outcome.degraded => {
            degraded_card(ui, ctx, content_h)
        }
        Some(OverlayView::Outcome { source, outcome }) => {
            scrolled_card(ui, ctx, content_h, FooterMode::Static, |ui| {
                outcome_body(ui, ctx, source, outcome)
            })
        }
        Some(OverlayView::Failed {
            cause,
            action: error_action,
        }) => failed_card(ui, ctx, content_h, cause, error_action),
    }
}

fn plain_card(
    ui: &mut egui::Ui,
    ctx: &RenderCtx<'_>,
    content_h: &mut f32,
    body: impl FnOnce(&mut egui::Ui, &mut Option<OverlayAction>),
) -> (Option<OverlayAction>, Option<egui::Vec2>) {
    let (mut action, drag) = header(ui, ctx.state, ctx.text);
    ui.add_space(space::SECTION);
    body(ui, &mut action);
    ui.add_space(space::PARAGRAPH);
    footer(ui, false, false, ctx.text);
    *content_h = ui.min_rect().height();
    (action, drag)
}

#[derive(Clone, Copy)]
enum FooterMode {
    Static,
    Streaming { repairing: bool },
}

fn scrolled_card(
    ui: &mut egui::Ui,
    ctx: &RenderCtx<'_>,
    content_h: &mut f32,
    mode: FooterMode,
    body: impl FnOnce(&mut egui::Ui),
) -> (Option<OverlayAction>, Option<egui::Vec2>) {
    let (action, drag) = header(ui, ctx.state, ctx.text);
    ui.add_space(space::SECTION);
    let scrolled = ScrollArea::new([false, true])
        .auto_shrink([false, true])
        .max_height(viewport_max(ui))
        .show(ui, |ui| {
            arrow_key_scroll(ui);
            body(ui);
        });
    ui.add_space(space::PARAGRAPH);
    match mode {
        FooterMode::Static => footer(ui, false, false, ctx.text),
        FooterMode::Streaming { repairing } => footer(ui, true, repairing, ctx.text),
    }
    record_scrolled_height(ui, content_h, &scrolled);
    (action, drag)
}

fn degraded_card(
    ui: &mut egui::Ui,
    ctx: &RenderCtx<'_>,
    content_h: &mut f32,
) -> (Option<OverlayAction>, Option<egui::Vec2>) {
    plain_card(ui, ctx, content_h, |ui, action| {
        ui.label(
            RichText::new(&ctx.text.gloss_popup_output_abnormal)
                .size(font::NOTICE)
                .color(ui.visuals().warn_fg_color),
        );
        ui.add_space(space::SECTION);
        if ui
            .button(
                RichText::new(ctx.text.gloss_popup_retry.as_str())
                    .size(font::CAPTION)
                    .color(ui.visuals().strong_text_color()),
            )
            .clicked()
        {
            *action = Some(OverlayAction::Retry);
        }
    })
}

fn failed_card(
    ui: &mut egui::Ui,
    ctx: &RenderCtx<'_>,
    content_h: &mut f32,
    cause: &FailureCause,
    error_action: &Option<ErrorAction>,
) -> (Option<OverlayAction>, Option<egui::Vec2>) {
    plain_card(ui, ctx, content_h, |ui, action| {
        ui.label(
            RichText::new(failure_message(cause, ctx.text))
                .size(font::NOTICE)
                .color(ui.visuals().warn_fg_color),
        );
        if let Some(error_action) = *error_action {
            ui.add_space(space::SECTION);
            if ui
                .button(
                    RichText::new(action_label(error_action, ctx.text))
                        .size(font::CAPTION)
                        .color(ui.visuals().strong_text_color()),
                )
                .clicked()
            {
                *action = Some(error_action.into());
            }
        }
    })
}

fn failure_message<'a>(cause: &'a FailureCause, text: &'a Text) -> Cow<'a, str> {
    match cause {
        FailureCause::Task(error) => text.for_error(error),
        FailureCause::TransportChannel => Cow::Borrowed(&text.gloss_errors_inference_channel),
    }
}

fn action_label(action: ErrorAction, text: &'static Text) -> &'static str {
    match action {
        ErrorAction::Retry => text.gloss_popup_retry.as_str(),
        ErrorAction::OpenSettings => text.gloss_popup_open_settings.as_str(),
    }
}
