//! 浮层与设置窗口的界面：内容渲染模板、字体与上下文的统一装入点。

pub mod code_hl;
pub mod context;
pub mod fonts;
pub mod i18n;
pub mod popup;
pub mod settings;
pub mod style;

pub(crate) fn pointing_hand(response: egui::Response) -> egui::Response {
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
