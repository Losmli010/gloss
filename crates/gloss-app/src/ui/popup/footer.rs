//! 页脚带：一条常驻窄带，左端随视图变体变化（流式生成指示、待确认按钮、
//! 完成卡校对徽章与复制钮），右端恒为本品牌（Gloss）水印。
//!
//! 待确认卡的确认与取消、完成卡的复制是页脚仅有的两处交互出口，动作在
//! 绘制帧折成动作上交视图分发；复制钮点击后短暂显示已复制反馈（驻留时长
//! 取动效档位），图标矢量现画不依赖字形。颜色经语义调色板解析，
//! 几何引用样式几何档；排版产物的跨帧缓存借卡片固定件的缓存基建。

use egui::{Align, RichText, Stroke, vec2};

use super::card::{CachedGalley, cached_galley, hairline};
use super::content::watermark;
use super::{OverlayAction, RenderState};
use crate::ui::i18n::Text;
use crate::ui::pointing_hand;
use crate::ui::style::{alpha, geometry, motion, palette, radius, size, space, stroke, with_alpha};

#[derive(Clone, Copy)]
pub(super) enum FooterMode {
    Static,
    Streaming { repairing: bool },
    Pending,
    Done { unverified: bool },
}

pub(super) fn footer(
    ui: &mut egui::Ui,
    state: &RenderState,
    mode: FooterMode,
    text: &'static Text,
) -> Option<OverlayAction> {
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
    let action = match mode {
        FooterMode::Static => None,
        FooterMode::Streaming { repairing } => {
            let label_width = footer_status(ui, &rect, &response, repairing, text);
            footer_dots(ui, &rect, label_width);
            None
        }
        FooterMode::Pending => footer_confirm(ui, &rect, text),
        FooterMode::Done { unverified } => footer_copy(ui, state, &rect, unverified, text),
    };
    footer_watermark(ui, &rect);
    action
}

fn footer_confirm(
    ui: &mut egui::Ui,
    rect: &egui::Rect,
    text: &'static Text,
) -> Option<OverlayAction> {
    let colors = palette(ui);
    let button_height = geometry::overlay::FOOTER_HEIGHT - 2.0 * space::TIGHT;
    let mut action = None;
    let mut left = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.translate(vec2(0.0, space::TIGHT)))
            .id_salt("footer_confirm"),
    );
    left.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
        let send = egui::Button::new(
            RichText::new(text.gloss_popup_confirm_send.as_str())
                .size(size::CAPTION)
                .color(colors.on_accent),
        )
        .fill(colors.seal)
        .corner_radius(radius::SEAL)
        .min_size(vec2(0.0, button_height));
        if pointing_hand(ui.add(send)).clicked() {
            action = Some(OverlayAction::Confirm);
        }
        ui.add_space(space::TIGHT);
        let cancel = egui::Button::new(
            RichText::new(text.gloss_popup_confirm_cancel.as_str())
                .size(size::CAPTION)
                .color(ui.visuals().strong_text_color()),
        )
        .corner_radius(radius::SEAL)
        .min_size(vec2(0.0, button_height));
        if pointing_hand(ui.add(cancel)).clicked() {
            action = Some(OverlayAction::Dismiss);
        }
    });
    action
}

fn footer_copy(
    ui: &mut egui::Ui,
    state: &RenderState,
    rect: &egui::Rect,
    unverified: bool,
    text: &'static Text,
) -> Option<OverlayAction> {
    let now = ui.input(|i| i.time);
    let copied = state
        .copied_at
        .get()
        .is_some_and(|at| now - at < motion::COPY_FEEDBACK);
    let mut action = None;
    let mut left = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.translate(vec2(0.0, space::TIGHT)))
            .id_salt("footer_copy"),
    );
    left.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
        if unverified {
            footer_unverified(ui, text);
            ui.add_space(space::TIGHT);
        }
        let side = geometry::overlay::FOOTER_HEIGHT - 2.0 * space::INLINE;
        let (button, response) = ui.allocate_exact_size(vec2(side, side), egui::Sense::click());
        let response = pointing_hand(response);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                text.gloss_popup_copy.as_str(),
            )
        });
        let accent = if response.hovered() || response.is_pointer_button_down_on() {
            ui.visuals().strong_text_color()
        } else {
            ui.visuals().weak_text_color()
        };
        if copied {
            paint_check_mark(ui, button, palette(ui).seal);
        } else {
            paint_copy_icon(ui, button, accent);
        }
        if response.clicked() {
            state.copied_at.set(Some(now));
            action = Some(OverlayAction::CopyAnnotation);
        }
        if copied {
            footer_copied_feedback(ui, text);
        }
    });
    action
}

fn footer_copied_feedback(ui: &mut egui::Ui, text: &'static Text) {
    ui.add_space(space::TIGHT);
    ui.label(
        RichText::new(text.gloss_popup_copied.as_str())
            .size(size::TAG)
            .color(palette(ui).seal),
    );
    ui.ctx().request_repaint_after(motion::FRAME_BEAT);
}

fn footer_unverified(ui: &mut egui::Ui, text: &'static Text) {
    let side = geometry::overlay::FOOTER_HEIGHT - 2.0 * space::INLINE;
    let (rect, response) = ui.allocate_exact_size(vec2(side, side), egui::Sense::hover());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Other,
            true,
            text.gloss_popup_output_unverified_label.as_str(),
        )
    });
    let _ = response.on_hover_text(text.gloss_popup_output_unverified.as_str());
    paint_alert_icon(ui, rect, palette(ui).warn);
}

fn paint_alert_icon(ui: &egui::Ui, rect: egui::Rect, color: egui::Color32) {
    let stroke = Stroke::new(stroke::LINE, color);
    let arm = geometry::action::ICON_ARM;
    let c = rect.center();
    let apex = c + egui::vec2(0.0, -arm);
    let bottom_right = c + egui::vec2(arm * 1.1, arm * 0.8);
    let bottom_left = c + egui::vec2(-arm * 1.1, arm * 0.8);
    ui.painter().add(egui::Shape::line(
        vec![apex, bottom_right, bottom_left, apex],
        stroke,
    ));
    ui.painter().line_segment(
        [
            c + egui::vec2(0.0, -arm * 0.45),
            c + egui::vec2(0.0, arm * 0.15),
        ],
        Stroke::new(stroke::THICK, color),
    );
    ui.painter()
        .circle_filled(c + egui::vec2(0.0, arm * 0.55), stroke::THICK * 0.5, color);
}

fn paint_copy_icon(ui: &egui::Ui, rect: egui::Rect, color: egui::Color32) {
    let stroke = Stroke::new(stroke::LINE, color);
    let arm = geometry::action::ICON_ARM;
    let offset = 2.0 * space::INLINE;
    let bounds =
        egui::Rect::from_center_size(rect.center(), vec2(2.0 * arm + offset, 2.0 * arm + offset));
    let back = egui::Rect::from_min_max(bounds.min + egui::vec2(offset, offset), bounds.max);
    ui.painter()
        .rect_stroke(back, radius::ICON, stroke, egui::StrokeKind::Inside);
    let front = egui::Rect::from_min_max(bounds.min, bounds.max - egui::vec2(offset, offset));
    ui.painter().rect(
        front,
        radius::ICON,
        ui.visuals().window_fill,
        stroke,
        egui::StrokeKind::Inside,
    );
}

fn paint_check_mark(ui: &egui::Ui, rect: egui::Rect, color: egui::Color32) {
    let arm = geometry::action::ICON_ARM;
    let c = rect.center();
    ui.painter().line_segment(
        [
            c + egui::vec2(-arm, 0.0),
            c + egui::vec2(-arm * 0.25, arm * 0.75),
        ],
        Stroke::new(stroke::THICK, color),
    );
    ui.painter().line_segment(
        [
            c + egui::vec2(-arm * 0.25, arm * 0.75),
            c + egui::vec2(arm, -arm * 0.75),
        ],
        Stroke::new(stroke::THICK, color),
    );
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
        egui::pos2(rect.min.x, rect.center().y - galley.size().y / 2.0),
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
        rect.center().y,
    );
    for index in 0..geometry::overlay::FOOTER_DOT_COUNT {
        let wave = ((now / motion::PULSE_PERIOD + index as f64 * motion::PULSE_PHASE_STEP).sin()
            + 1.0)
            / 2.0;
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
    let text_height = galley.size().y;
    let text_top = rect.center().y - text_height / 2.0;
    ui.put(
        egui::Rect::from_min_max(
            egui::pos2(rect.max.x - slot, text_top),
            egui::pos2(rect.max.x, text_top + text_height),
        ),
        egui::Label::new(egui::WidgetText::Galley(galley))
            .selectable(false)
            .halign(Align::RIGHT),
    );
}
