//! 设置页的控件基元：动作行、提示行、分区卡片、输入框与错误标注、开关
//! 滑块、双列行——只认设置页私有类型，不认具体配置字段；分区内容的组装
//! 在 [`super::fields`]。

use std::collections::HashMap;

use egui::{RichText, Stroke};

use super::{FieldError, FieldKey, SettingsAction, SettingsState, build_save};
use crate::ui::i18n::{Text, fill};
use crate::ui::style::{color, font, radius, space};

/// 品牌强调色：明暗主题各取朱砂档——浅底用压暗档、深底用提亮档，与印章
/// 朱丝栏、品牌划线同一色源。
fn accent(ui: &egui::Ui) -> egui::Color32 {
    if ui.visuals().dark_mode {
        color::ACCENT_DARK
    } else {
        color::ACCENT_LIGHT
    }
}

/// 动作行：取消（次按钮）+ 保存（朱砂主按钮），右对齐。
pub(super) fn action_row(
    ui: &mut egui::Ui,
    state: &mut SettingsState,
    action: &mut SettingsAction,
    text: &Text,
) {
    // 先分配固定行高再右对齐：bottom_up 里直接 with_layout(RTL) 的子区域
    // 会撑满剩余整高，按钮被垂直居中到窗口中部，滚动区被挤剩一条。
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

/// 动作行高度（按钮高 + 上下留白）。
const ACTION_ROW_HEIGHT: f32 = 30.0;

/// 提示行：校验汇总（进入校验态且有错）与壳回写提示（保存失败等）。
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
            .size(font::CAPTION)
            .color(color::DANGER),
        );
    }
    if let Some(notice) = &state.notice {
        ui.add_space(space::TIGHT);
        ui.label(
            RichText::new(notice.message(text))
                .size(font::CAPTION)
                .color(color::DANGER),
        );
    }
}

/// 分区：区块标（弱色小字）+ faint_bg 卡片。
pub(super) fn section(ui: &mut egui::Ui, title: &str, content: impl FnOnce(&mut egui::Ui)) {
    caption(ui, title);
    ui.add_space(space::TIGHT);
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(egui::CornerRadius::same(radius::CARD))
        .inner_margin(egui::Margin::same(space::CARD_PADDING))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            content(ui);
        });
}

/// 弱色小字（区块标 / 子块标 / 说明提示共用一档）。
fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(font::CAPTION)
            .color(ui.visuals().weak_text_color()),
    );
}

/// 设置页输入框统一入口：浅色主题下白底输入框与近白卡片几乎同色，
/// 补一圈浅灰描边保持可见；深色主题输入框本身比卡片暗，不加描边。
pub(super) fn add_input<'t>(
    ui: &mut egui::Ui,
    text: &'t mut String,
    build: impl FnOnce(egui::TextEdit<'t>) -> egui::TextEdit<'t>,
) -> egui::Response {
    ui.scope(|ui| {
        if !ui.visuals().dark_mode {
            ui.visuals_mut().widgets.inactive.bg_stroke =
                Stroke::new(1.0, egui::Color32::from_gray(0xE2));
        }
        ui.add(build(egui::TextEdit::singleline(text)))
    })
    .inner
}

/// 字段错误态：控件底边 DANGER 下划线。
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
            Stroke::new(1.5, color::DANGER),
        );
    }
}

/// 就地错误提示（控件正下方，CAPTION DANGER）。
pub(super) fn error_text(
    ui: &mut egui::Ui,
    errors: &HashMap<FieldKey, FieldError>,
    key: FieldKey,
    text: &Text,
) {
    if let Some(error) = errors.get(&key) {
        ui.label(
            RichText::new(error.message(text))
                .size(font::CAPTION)
                .color(color::DANGER),
        );
    }
}

/// 开关滑块的轨道尺寸与滑钮半径（demo 定稿比例的紧凑档：滑钮直径略小
/// 于轨高，上下各留一线余量）。
const TOGGLE_TRACK: egui::Vec2 = egui::vec2(36.0, 20.0);
const TOGGLE_KNOB_RADIUS: f32 = 7.0;
const TOGGLE_KNOB_INSET: f32 = 3.0;
/// 开启态滑钮色：朱砂着轨配白钮，与保存主按钮同观感（两主题通用）。
const TOGGLE_KNOB_ON: egui::Color32 = egui::Color32::WHITE;

/// 最小开关（switch）：egui 0.36 无内建 Switch（`WidgetType` 亦无 switch
/// 档），按官方 demo 的 `toggle_switch` 范式自绘——点击整块切换，无障碍
/// 以 `WidgetType::Checkbox` + 选中态上报（0.36 的角色映射里唯一的双态
/// 档，内建 `Checkbox` 也走它）。返回的响应供调用方感知变更。
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

/// 双列行：行标签左、控件推到卡片右缘（两端对齐）；返回控件自身的响应
/// （字段标红要按控件的矩形画下划线）。
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

/// 双列行的说明提示（左对齐，CAPTION 弱色）。
pub(super) fn choice_hint(ui: &mut egui::Ui, text: &str) {
    caption(ui, text);
}
