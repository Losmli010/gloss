//! 浮层内容：M1 只画一张渲染自检卡。
//!
//! 真正按 `TaskKind` 分发的结果卡（原文/译文/操作栏）在 M3-T9 与 M5-T6 落地，
//! 这里先把窗口、渲染与中文字体的闭环撑起来。

use egui::{Color32, CornerRadius, Frame, Margin, RichText, Stroke, vec2};

/// 浮层宽度（UI 规范 §2）
pub const WIDTH: f32 = 380.0;
/// 卡片圆角（UI 规范 §2）
const CORNER_RADIUS: u8 = 8;
/// 内容区内边距（UI 规范 §2）
const PADDING: i8 = 14;
/// 头部身份圆点：品牌珊瑚橙（UI 规范 §〇）
const BRAND_DOT: Color32 = Color32::from_rgb(0xD8, 0x5A, 0x30);

/// 画一帧浮层。根 `Ui` 覆盖整个窗口，卡片铺满它，圆角之外由透明窗口露出桌面。
pub fn draw(ui: &mut egui::Ui) {
    let fill = ui.visuals().window_fill;
    let stroke = ui.visuals().window_stroke;
    let strong = ui.visuals().strong_text_color();
    let weak = ui.visuals().weak_text_color();

    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(0.5, stroke.color))
        .corner_radius(CornerRadius::same(CORNER_RADIUS))
        .inner_margin(Margin::same(PADDING))
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            header(ui, weak);
            ui.add_space(12.0);
            body(ui, strong, weak);
        });
}

/// 头部：身份圆点 + 标题，右侧放状态位（UI 规范 §3.1）
fn header(ui: &mut egui::Ui, weak: Color32) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 4.0, BRAND_DOT);
        ui.label(RichText::new("翻译").size(12.0).color(weak));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new("M1 渲染自检").size(11.0).color(weak));
        });
    });
}

/// 正文：次要色的原文 + 主色的译文，字号与行距按 UI 规范 §3.2
fn body(ui: &mut egui::Ui, strong: Color32, weak: Color32) {
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
