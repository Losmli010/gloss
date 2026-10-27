//! 卡片固定件：页头（应用图标与动作区、拖动热区）、页脚带（状态文案、
//! 呼吸点与水印）、经注疏印章，以及印章与水印排版产物的跨帧缓存。
//!
//! 颜色经语义调色板解析，几何引用样式几何档；视图骨架与经注疏排布
//! 从这里取页头、页脚与印章。

use std::collections::HashMap;
use std::sync::Arc;

use egui::{Align, RichText, Stroke, vec2};

use super::content::watermark;
use super::{OverlayAction, RenderState};
use crate::ui::i18n::Text;
use crate::ui::style::{
    alpha, geometry, motion, palette, radius, serif, size, space, stroke, with_alpha,
};

fn hairline(ui: &egui::Ui) -> Stroke {
    Stroke::new(
        stroke::HAIRLINE,
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
            ui.add(egui::Image::from_texture(&icon).fit_to_exact_size(vec2(
                geometry::overlay::HEADER_ICON,
                geometry::overlay::HEADER_ICON,
            )));
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
    let right = (gear_left - geometry::action::DRAG_STRIP_INSET).max(row.left());
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
    let (rect, response) = ui.allocate_exact_size(
        vec2(geometry::action::BUTTON, geometry::action::BUTTON),
        egui::Sense::click(),
    );
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
    let stroke = Stroke::new(stroke::THICK, color);
    let center = rect.center();
    ui.painter().line_segment(
        [
            center - vec2(geometry::action::CLOSE_ARM, geometry::action::CLOSE_ARM),
            center + vec2(geometry::action::CLOSE_ARM, geometry::action::CLOSE_ARM),
        ],
        stroke,
    );
    ui.painter().line_segment(
        [
            center - vec2(geometry::action::CLOSE_ARM, -geometry::action::CLOSE_ARM),
            center + vec2(geometry::action::CLOSE_ARM, -geometry::action::CLOSE_ARM),
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
    let colors = palette(ui);
    let (fg, fill, line) = match tint {
        SealTint::Ink => (colors.paper, ui.visuals().strong_text_color(), Stroke::NONE),
        SealTint::Zhu => (colors.paper, colors.seal, Stroke::NONE),
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
            colors.is_dark,
            ui.ctx().pixels_per_point().to_bits(),
        ),
        || {
            ui.painter()
                .layout_no_wrap(label.to_owned(), serif(size::SEAL), fg)
        },
    );
    let width = geometry::seal::SIZE.max(galley.size().x + 2.0 * geometry::seal::TEXT_PADDING);
    let (rect, response) =
        ui.allocate_exact_size(vec2(width, geometry::seal::SIZE), egui::Sense::hover());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, label.to_owned()));
    ui.painter().rect(
        rect,
        radius::SEAL.round() as u8,
        fill,
        line,
        egui::StrokeKind::Inside,
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, fg);
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
    with_alpha(palette(ui).seal, alpha::SEAL_LINE)
}

pub(super) fn footer(ui: &mut egui::Ui, streaming: bool, repairing: bool, text: &'static Text) {
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), geometry::overlay::FOOTER_HEIGHT),
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
            palette(ui).is_dark,
            ui.ctx().pixels_per_point().to_bits(),
        ),
        || {
            egui::WidgetText::from(RichText::new(label.as_str()).size(size::TAG).color(dim))
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
            rect.center().y + stroke::HAIRLINE - galley.size().y / 2.0,
        ),
        galley.clone(),
        dim,
    );
    galley.size().x
}

fn footer_dots(ui: &egui::Ui, rect: &egui::Rect, label_width: f32) {
    let now = ui.input(|i| i.time);
    let zhu = palette(ui).seal;
    let mut dot_center = egui::pos2(
        rect.min.x + label_width + space::TIGHT + geometry::overlay::FOOTER_DOT_RADIUS,
        rect.center().y + stroke::HAIRLINE,
    );
    for index in 0..geometry::overlay::FOOTER_DOT_COUNT {
        let wave = ((now / motion::PULSE_PERIOD + index as f64 * 0.9).sin() + 1.0) / 2.0;
        let alpha01 = alpha::DOT_MIN + wave as f32 * (alpha::DOT_MAX - alpha::DOT_MIN);
        ui.painter().circle_filled(
            dot_center,
            geometry::overlay::FOOTER_DOT_RADIUS,
            with_alpha(zhu, alpha01),
        );
        dot_center.x += geometry::overlay::FOOTER_DOT_RADIUS * 3.0;
    }
    ui.ctx().request_repaint_after(motion::FRAME_BEAT);
}

fn footer_watermark(ui: &mut egui::Ui, rect: &egui::Rect) {
    let dim = ui.visuals().weak_text_color();
    let galley = cached_galley(
        ui,
        CachedGalley::Watermark(palette(ui).is_dark, ui.ctx().pixels_per_point().to_bits()),
        || {
            egui::WidgetText::from(RichText::new(watermark()).size(size::TAG).color(dim))
                .into_galley(
                    ui,
                    Some(ui.wrap_mode()),
                    ui.available_width(),
                    egui::FontSelection::Default,
                )
        },
    );
    let slot = galley.size().x + 2.0 * geometry::overlay::WATERMARK_PADDING;
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
    egui::Button::new(RichText::new(glyph).size(geometry::action::ICON_SIZE))
        .frame(false)
        .min_size(vec2(geometry::action::BUTTON, geometry::action::BUTTON))
}
