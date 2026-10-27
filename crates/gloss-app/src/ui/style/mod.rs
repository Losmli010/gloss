//! 样式统一出口：浮层与设置窗共用的设计档位——颜色语义在此定稿，
//! 字号、间距、圆角与线宽阶梯在档位模块汇出。

mod tokens;

pub(crate) use tokens::{font, radius, space, stroke};

pub mod color {
    use egui::Color32;

    pub const ACCENT_LIGHT: Color32 = SEAL_ZHU_LIGHT;

    pub const ACCENT_DARK: Color32 = SEAL_ZHU_DARK;

    pub const DANGER: Color32 = Color32::from_rgb(0xC6, 0x28, 0x28);

    pub const SEAL_ZHU_LIGHT: Color32 = Color32::from_rgb(0xB8, 0x43, 0x2F);

    pub const SEAL_ZHU_DARK: Color32 = Color32::from_rgb(0xE0, 0x6A, 0x52);

    pub const CODE_BG_LIGHT: Color32 = Color32::from_rgb(0xF5, 0xF7, 0xF9);

    pub const CODE_BG_DARK: Color32 = Color32::from_rgb(0x16, 0x18, 0x1E);

    pub const CODE_BORDER_LIGHT: Color32 = Color32::from_rgba_premultiplied(2, 2, 3, 26);

    pub const CODE_BORDER_DARK: Color32 = Color32::from_rgba_premultiplied(23, 23, 23, 23);
}
