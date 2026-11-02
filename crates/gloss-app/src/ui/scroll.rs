//! 两窗口滚动区共用的键盘滚动基元：滚轮与拖滚动条由界面库的滚动区
//! 原生承接，这里只补方向键一翼——上下方向键按档位推视口，输入框持焦
//! 时让位给光标移动（界面库的文本编辑不消费按键事件，不守卫会光标与
//! 视口双触发）。步长档位与焦点守卫的取舍见记录文件。

pub(crate) fn arrow_key_scroll(ui: &egui::Ui) {
    if ui.memory(|mem| mem.focused()).is_some() {
        return;
    }
    let step = ui.input(|input| {
        if !input.modifiers.any() {
            if input.key_pressed(egui::Key::ArrowDown) {
                return Some(-super::style::geometry::scroll::ARROW_SCROLL_STEP);
            }
            if input.key_pressed(egui::Key::ArrowUp) {
                return Some(super::style::geometry::scroll::ARROW_SCROLL_STEP);
            }
        }
        None
    });
    if let Some(dy) = step {
        ui.scroll_with_delta(egui::vec2(0.0, dy));
    }
}
