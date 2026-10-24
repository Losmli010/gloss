//! 卡片固定件：页头（应用图标与动作区、拖动热区）、页脚带（状态文案、
//! 呼吸点与水印）、经注疏印章，以及印章与水印排版产物的跨帧缓存。
//!
//! 几何与配色常量是设计稿定稿值，随实现就近存放；字号规格与宋楷字体
//! 助手在文案整形模块。视图骨架与经注疏排布
//! 从这里取页头、页脚与印章。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use egui::{Align, RichText, Stroke, vec2};

use super::content::{SEAL_FONT, serif_font, watermark};
use super::{FOOTER_HEIGHT, HEADER_ICON, OverlayAction, PULSE_PERIOD_SECS, RenderState};
use crate::ui::i18n::Text;
use crate::ui::style::{color, font, space, stroke};

const ACTION_ICON_SIZE: f32 = 16.0;
const ACTION_BUTTON: f32 = 20.0;
const CLOSE_ARM: f32 = 5.0;
const CLOSE_STROKE: f32 = 2.0;
const DRAG_STRIP_INSET: f32 = 6.0;
const SEAL_SIZE: f32 = 21.0;
const SEAL_RADIUS: u8 = 5;
const SEAL_TEXT_PADDING: f32 = 5.0;
const ZHU_LINE_ALPHA: u8 = 140;
const FOOTER_DOT_RADIUS: f32 = 2.0;
const FOOTER_DOT_COUNT: usize = 3;
const FOOTER_DOT_ALPHA_MIN: u8 = 90;
const FOOTER_DOT_ALPHA_MAX: u8 = 180;
const WATERMARK_PADDING: f32 = 6.0;

fn hairline(ui: &egui::Ui) -> Stroke {
    Stroke::new(
        stroke::CARD,
        ui.visuals().widgets.noninteractive.bg_stroke.color,
    )
}

pub(super) fn header(
    ui: &mut egui::Ui,
    state: &RenderState,
    text: &'static Text,
) -> (Option<OverlayAction>, Option<egui::Vec2>) {
    let weak = ui.visuals().weak_text_color();
    let strong = ui.visuals().strong_text_color();
    let mut action = None;
    let mut drag = None;
    let mut gear_left = f32::INFINITY;
    ui.horizontal(|ui| {
        if let Some(icon) = state.icon_texture(ui.ctx()) {
            ui.add(
                egui::Image::from_texture(&icon).fit_to_exact_size(vec2(HEADER_ICON, HEADER_ICON)),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.visuals_mut().widgets.inactive.fg_stroke.color = weak;
            ui.visuals_mut().widgets.hovered.fg_stroke.color = strong;
            ui.visuals_mut().widgets.active.fg_stroke.color = strong;
            let close = close_button(ui, text);
            if close.clicked() {
                action = Some(OverlayAction::Dismiss);
            }
            let gear = ui.add(icon_button("⚙")); // i18n:allow 图标字形，非 locale 文案
            gear_left = gear.rect.left();
            gear.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    true,
                    text.gloss_popup_settings_label.as_str(),
                )
            });
            if gear.clicked() {
                action = Some(OverlayAction::OpenSettings);
            }
        });
        drag = drag_strip(ui, state, gear_left, text);
    });
    ui.add_space(space::ITEM);
    let line_y = ui.cursor().top();
    ui.painter()
        .hline(ui.max_rect().x_range(), line_y, hairline(ui));
    (action, drag)
}

fn drag_strip(
    ui: &mut egui::Ui,
    state: &RenderState,
    gear_left: f32,
    text: &'static Text,
) -> Option<egui::Vec2> {
    let row = ui.min_rect();
    let right = (gear_left - DRAG_STRIP_INSET).max(row.left());
    let rect = egui::Rect::from_min_max(
        egui::pos2(row.left(), row.top()),
        egui::pos2(right, row.bottom()),
    );
    let response = ui.interact(
        rect,
        egui::Id::new("overlay_header_drag"),
        egui::Sense::drag(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Other,
            true,
            text.gloss_popup_drag_label.as_str(),
        )
    });
    let cursor = if response.dragged() {
        egui::CursorIcon::Grabbing
    } else {
        egui::CursorIcon::Grab
    };
    let response = response.on_hover_cursor(cursor);
    let (pointer, pixels_per_point) = ui.input(|i| (i.pointer.interact_pos(), i.pixels_per_point));
    if response.drag_started() {
        state
            .drag_press_phys
            .set(pointer.map(|pos| pos * pixels_per_point));
        return Some(egui::Vec2::ZERO);
    }
    if !response.dragged() {
        state.drag_press_phys.set(None);
        return None;
    }
    let press_phys = state.drag_press_phys.get()?;
    let pointer_phys = pointer? * pixels_per_point;
    Some((pointer_phys - press_phys) / pixels_per_point)
}

fn close_button(ui: &mut egui::Ui, text: &'static Text) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ACTION_BUTTON, ACTION_BUTTON), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            text.gloss_popup_close_label.as_str(),
        )
    });
    let color = if response.hovered() || response.is_pointer_button_down_on() {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    let stroke = Stroke::new(CLOSE_STROKE, color);
    let center = rect.center();
    ui.painter().line_segment(
        [
            center - vec2(CLOSE_ARM, CLOSE_ARM),
            center + vec2(CLOSE_ARM, CLOSE_ARM),
        ],
        stroke,
    );
    ui.painter().line_segment(
        [
            center - vec2(CLOSE_ARM, -CLOSE_ARM),
            center + vec2(CLOSE_ARM, -CLOSE_ARM),
        ],
        stroke,
    );
    response
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum SealTint {
    Ink,
    Zhu,
    Wei,
}

pub(super) fn seal(ui: &mut egui::Ui, label: &'static str, tint: SealTint) {
    let (fg, fill, line) = match tint {
        SealTint::Ink => (
            paper_color(ui),
            ui.visuals().strong_text_color(),
            Stroke::NONE,
        ),
        SealTint::Zhu => (paper_color(ui), zhu_color(ui), Stroke::NONE),
        SealTint::Wei => {
            let dim = ui.visuals().weak_text_color();
            (dim, egui::Color32::TRANSPARENT, Stroke::new(1.0, dim))
        }
    };
    let galley = cached_galley(
        ui,
        CachedGalley::Seal(
            label,
            tint,
            ui.visuals().dark_mode,
            ui.ctx().pixels_per_point().to_bits(),
        ),
        || {
            ui.painter()
                .layout_no_wrap(label.to_owned(), serif_font(SEAL_FONT), fg)
        },
    );
    let width = SEAL_SIZE.max(galley.size().x + 2.0 * SEAL_TEXT_PADDING);
    let (rect, response) = ui.allocate_exact_size(vec2(width, SEAL_SIZE), egui::Sense::hover());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, label.to_owned()));
    ui.painter()
        .rect(rect, SEAL_RADIUS, fill, line, egui::StrokeKind::Inside);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, fg);
}

fn paper_color(ui: &egui::Ui) -> egui::Color32 {
    if ui.visuals().dark_mode {
        egui::Color32::from_rgb(0xF2, 0xE9, 0xE4)
    } else {
        egui::Color32::from_rgb(0xFF, 0xF6, 0xF2)
    }
}

fn zhu_color(ui: &egui::Ui) -> egui::Color32 {
    if ui.visuals().dark_mode {
        color::SEAL_ZHU_DARK
    } else {
        color::SEAL_ZHU_LIGHT
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum CachedGalley {
    Seal(&'static str, SealTint, bool, u32),
    Watermark(bool, u32),
    FooterLabel(&'static str, bool, u32),
}

#[derive(Default)]
struct CachedGalleys(HashMap<CachedGalley, Arc<egui::Galley>>);

impl egui::cache::CacheTrait for CachedGalleys {
    fn update(&mut self) {}

    fn len(&self) -> usize {
        self.0.len()
    }
}

pub(crate) fn clear_cached_text_galleys(ctx: &egui::Context) {
    ctx.memory_mut(|mem| mem.caches.cache::<CachedGalleys>().0.clear());
}

fn cached_galley(
    ui: &egui::Ui,
    slot: CachedGalley,
    layout: impl FnOnce() -> Arc<egui::Galley>,
) -> Arc<egui::Galley> {
    let ctx = ui.ctx();
    let cached = ctx.memory_mut(|mem| mem.caches.cache::<CachedGalleys>().0.get(&slot).cloned());
    if let Some(galley) = cached {
        return galley;
    }
    let galley = layout();
    ctx.memory_mut(|mem| {
        mem.caches
            .cache::<CachedGalleys>()
            .0
            .insert(slot, Arc::clone(&galley));
    });
    galley
}

pub(super) fn zhu_line_color(ui: &egui::Ui) -> egui::Color32 {
    let zhu = zhu_color(ui);
    egui::Color32::from_rgba_unmultiplied(zhu.r(), zhu.g(), zhu.b(), ZHU_LINE_ALPHA)
}

pub(super) fn footer(ui: &mut egui::Ui, streaming: bool, repairing: bool, text: &'static Text) {
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), FOOTER_HEIGHT),
        egui::Sense::hover(),
    );
    ui.painter().line_segment(
        [
            egui::pos2(rect.min.x, rect.min.y),
            egui::pos2(rect.max.x, rect.min.y),
        ],
        hairline(ui),
    );
    if streaming {
        let label_width = footer_status(ui, &rect, &response, repairing, text);
        footer_dots(ui, &rect, label_width);
    }
    footer_watermark(ui, &rect);
}

fn footer_status(
    ui: &egui::Ui,
    rect: &egui::Rect,
    response: &egui::Response,
    repairing: bool,
    text: &'static Text,
) -> f32 {
    let label = if repairing {
        &text.gloss_popup_reannotating
    } else {
        &text.gloss_popup_annotating
    };
    let dim = ui.visuals().weak_text_color();
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::ProgressIndicator, true, label.as_str())
    });
    let galley = cached_galley(
        ui,
        CachedGalley::FooterLabel(
            label.as_str(),
            ui.visuals().dark_mode,
            ui.ctx().pixels_per_point().to_bits(),
        ),
        || {
            egui::WidgetText::from(RichText::new(label.as_str()).size(font::TAG).color(dim))
                .into_galley(
                    ui,
                    Some(ui.wrap_mode()),
                    ui.available_width(),
                    egui::FontSelection::Default,
                )
        },
    );
    ui.painter().galley(
        egui::pos2(
            rect.min.x,
            rect.center().y + stroke::CARD - galley.size().y / 2.0,
        ),
        galley.clone(),
        dim,
    );
    galley.size().x
}

fn footer_dots(ui: &egui::Ui, rect: &egui::Rect, label_width: f32) {
    let now = ui.input(|i| i.time);
    let zhu = zhu_color(ui);
    let mut dot_center = egui::pos2(
        rect.min.x + label_width + space::TIGHT + FOOTER_DOT_RADIUS,
        rect.center().y + stroke::CARD,
    );
    for index in 0..FOOTER_DOT_COUNT {
        let wave = ((now / PULSE_PERIOD_SECS + index as f64 * 0.9).sin() + 1.0) / 2.0;
        let alpha = (FOOTER_DOT_ALPHA_MIN as f64
            + wave * f64::from(FOOTER_DOT_ALPHA_MAX - FOOTER_DOT_ALPHA_MIN))
            as f32;
        let alpha = alpha.round().clamp(0.0, 255.0) as u8;
        ui.painter().circle_filled(
            dot_center,
            FOOTER_DOT_RADIUS,
            egui::Color32::from_rgba_unmultiplied(zhu.r(), zhu.g(), zhu.b(), alpha),
        );
        dot_center.x += FOOTER_DOT_RADIUS * 3.0;
    }
    ui.ctx()
        .request_repaint_after(Duration::from_secs_f32(0.016));
}

fn footer_watermark(ui: &mut egui::Ui, rect: &egui::Rect) {
    let dim = ui.visuals().weak_text_color();
    let galley = cached_galley(
        ui,
        CachedGalley::Watermark(
            ui.visuals().dark_mode,
            ui.ctx().pixels_per_point().to_bits(),
        ),
        || {
            egui::WidgetText::from(RichText::new(watermark()).size(font::TAG).color(dim))
                .into_galley(
                    ui,
                    Some(ui.wrap_mode()),
                    ui.available_width(),
                    egui::FontSelection::Default,
                )
        },
    );
    let slot = galley.size().x + 2.0 * WATERMARK_PADDING;
    ui.put(
        egui::Rect::from_min_max(
            egui::pos2(rect.max.x - slot, rect.min.y),
            egui::pos2(rect.max.x, rect.max.y),
        ),
        egui::Label::new(egui::WidgetText::Galley(galley))
            .selectable(false)
            .halign(Align::RIGHT),
    );
}

fn icon_button(glyph: &'static str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(glyph).size(ACTION_ICON_SIZE))
        .frame(false)
        .min_size(vec2(ACTION_BUTTON, ACTION_BUTTON))
}
