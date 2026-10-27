//! 设置页的控件基元：动作行、提示行、分区卡片、输入框与错误标注、开关
//! 滑块、双列行——只认设置页私有类型，不认具体配置字段；分区内容的组装
//! 在分区内容模块。

use std::collections::HashMap;

use egui::{RichText, Stroke};

use super::{FieldError, FieldKey, SettingsAction, SettingsState, build_save};
use crate::ui::i18n::{Text, fill};
use crate::ui::style::{palette, radius, size, space};

fn accent(ui: &egui::Ui) -> egui::Color32 {
    palette(ui).accent
}

pub(super) fn action_row(
    ui: &mut egui::Ui,
    state: &mut SettingsState,
    action: &mut SettingsAction,
    text: &Text,
) {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), ACTION_ROW_HEIGHT),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            let save = ui.add(
                egui::Button::new(
                    RichText::new(text.gloss_settings_save.as_str()).color(egui::Color32::WHITE),
                )
                .fill(accent(ui))
                .corner_radius(egui::CornerRadius::same(6)),
            );
            if save.clicked() {
                *action = build_save(state);
            }
            if ui.button(text.gloss_settings_cancel.as_str()).clicked() {
                *action = SettingsAction::Close;
            }
        },
    );
}

const ACTION_ROW_HEIGHT: f32 = 30.0;

pub(super) fn notices(
    ui: &mut egui::Ui,
    state: &SettingsState,
    errors: &HashMap<FieldKey, FieldError>,
    text: &Text,
) {
    if state.validated && !errors.is_empty() {
        let count = errors.len().to_string();
        ui.add_space(space::TIGHT);
        ui.label(
            RichText::new(fill(
                &text.gloss_settings_invalid_summary,
                &[("count", &count)],
            ))
            .size(size::CAPTION)
            .color(palette(ui).danger),
        );
    }
    if let Some(notice) = &state.notice {
        ui.add_space(space::TIGHT);
        ui.label(
            RichText::new(notice.message(text))
                .size(size::CAPTION)
                .color(palette(ui).danger),
        );
    }
}

pub(super) fn section(ui: &mut egui::Ui, title: &str, content: impl FnOnce(&mut egui::Ui)) {
    caption(ui, title);
    ui.add_space(space::TIGHT);
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(egui::CornerRadius::same(radius::CARD.round() as u8))
        .inner_margin(egui::Margin::same(space::CARD_PADDING.round() as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            content(ui);
        });
}

fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(size::CAPTION)
            .color(ui.visuals().weak_text_color()),
    );
}

pub(super) fn add_input<'t>(
    ui: &mut egui::Ui,
    text: &'t mut String,
    build: impl FnOnce(egui::TextEdit<'t>) -> egui::TextEdit<'t>,
) -> egui::Response {
    ui.scope(|ui| {
        ui.visuals_mut().widgets.inactive.bg_stroke = palette(ui).input_border;
        ui.add(build(egui::TextEdit::singleline(text)))
    })
    .inner
}

pub(super) fn underline_if_error(
    ui: &mut egui::Ui,
    response: &egui::Response,
    errors: &HashMap<FieldKey, FieldError>,
    key: FieldKey,
) {
    if errors.contains_key(&key) {
        let rect = response.rect;
        ui.painter().line_segment(
            [
                egui::pos2(rect.left(), rect.bottom()),
                egui::pos2(rect.right(), rect.bottom()),
            ],
            Stroke::new(1.5, palette(ui).danger),
        );
    }
}

pub(super) fn error_text(
    ui: &mut egui::Ui,
    errors: &HashMap<FieldKey, FieldError>,
    key: FieldKey,
    text: &Text,
) {
    if let Some(error) = errors.get(&key) {
        ui.label(
            RichText::new(error.message(text))
                .size(size::CAPTION)
                .color(palette(ui).danger),
        );
    }
}

const TOGGLE_TRACK: egui::Vec2 = egui::vec2(36.0, 20.0);
const TOGGLE_KNOB_RADIUS: f32 = 7.0;
const TOGGLE_KNOB_INSET: f32 = 3.0;
const TOGGLE_KNOB_ON: egui::Color32 = egui::Color32::WHITE;

pub(super) fn toggle_switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let (rect, mut response) = ui.allocate_exact_size(TOGGLE_TRACK, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, "")
    });
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }

    let visuals = ui.visuals();
    let (track, knob) = if *on {
        (visuals.selection.bg_fill, TOGGLE_KNOB_ON)
    } else {
        (visuals.extreme_bg_color, visuals.weak_text_color())
    };
    let painter = ui.painter();
    painter.rect_filled(rect, f32::INFINITY, track);
    if !(*on) {
        painter.rect_stroke(
            rect,
            f32::INFINITY,
            visuals.widgets.noninteractive.bg_stroke,
            egui::StrokeKind::Inside,
        );
    }
    let knob_x = if *on {
        rect.right() - TOGGLE_KNOB_INSET - TOGGLE_KNOB_RADIUS
    } else {
        rect.left() + TOGGLE_KNOB_INSET + TOGGLE_KNOB_RADIUS
    };
    painter.circle_filled(
        egui::pos2(knob_x, rect.center().y),
        TOGGLE_KNOB_RADIUS,
        knob,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(super) fn choice_row<R>(
    ui: &mut egui::Ui,
    label: &str,
    control: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control)
            .inner
    })
    .inner
}

pub(super) fn choice_hint(ui: &mut egui::Ui, text: &str) {
    caption(ui, text);
}
