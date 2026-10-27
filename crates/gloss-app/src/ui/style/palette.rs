//! 语义调色板：明暗两份解析完毕的颜色数据、透明度档位与语法主题键。
//!
//! 不透明色值从成对常量加使用点分支收敛为两份数据：新增外观不再改遍
//! 使用点分支，语义色清单在此一份。取档走出口模块的解析点（全仓唯一
//! 读取明暗态的地方），视图层只认数据不认明暗。

use egui::{Color32, Stroke};

pub(crate) struct Palette {
    pub is_dark: bool,
    pub accent: Color32,
    pub seal: Color32,
    pub paper: Color32,
    pub code_bg: Color32,
    pub code_border: Color32,
    pub danger: Color32,
    pub on_accent: Color32,
    pub input_border: Stroke,
    pub syntax: SyntaxPalette,
}

pub(crate) struct SyntaxPalette {
    pub plain: Color32,
    pub keyword: Color32,
    pub string: Color32,
    pub comment: Color32,
    pub number: Color32,
    pub r#type: Color32,
    pub function: Color32,
}

pub(crate) const LIGHT: Palette = Palette {
    is_dark: false,
    accent: SEAL_ZHU_LIGHT,
    seal: SEAL_ZHU_LIGHT,
    paper: Color32::from_rgb(0xFF, 0xF6, 0xF2),
    code_bg: Color32::from_rgb(0xF5, 0xF7, 0xF9),
    code_border: Color32::from_rgba_premultiplied(2, 2, 3, 26),
    danger: DANGER,
    on_accent: Color32::WHITE,
    input_border: Stroke {
        width: 1.0,
        color: Color32::from_gray(0xE2),
    },
    syntax: SyntaxPalette {
        plain: Color32::from_rgb(0x24, 0x29, 0x2E),
        keyword: Color32::from_rgb(0x8E, 0x44, 0xAD),
        string: Color32::from_rgb(0x1E, 0x7E, 0x34),
        comment: Color32::from_rgb(0x9A, 0xA1, 0xAC),
        number: Color32::from_rgb(0xB4, 0x53, 0x09),
        r#type: Color32::from_rgb(0x0E, 0x7C, 0x8C),
        function: Color32::from_rgb(0x2B, 0x6C, 0xB0),
    },
};

pub(crate) const DARK: Palette = Palette {
    is_dark: true,
    accent: SEAL_ZHU_DARK,
    seal: SEAL_ZHU_DARK,
    paper: Color32::from_rgb(0xF2, 0xE9, 0xE4),
    code_bg: Color32::from_rgb(0x16, 0x18, 0x1E),
    code_border: Color32::from_rgba_premultiplied(23, 23, 23, 23),
    danger: DANGER,
    on_accent: Color32::WHITE,
    input_border: Stroke::NONE,
    syntax: SyntaxPalette {
        plain: Color32::from_rgb(0xE6, 0xE6, 0xE6),
        keyword: Color32::from_rgb(0xC7, 0x92, 0xEA),
        string: Color32::from_rgb(0xA5, 0xD6, 0xA7),
        comment: Color32::from_rgb(0x6D, 0x76, 0x83),
        number: Color32::from_rgb(0xF0, 0xB4, 0x52),
        r#type: Color32::from_rgb(0x66, 0xC7, 0xD4),
        function: Color32::from_rgb(0x7C, 0xB8, 0xEC),
    },
};

const SEAL_ZHU_LIGHT: Color32 = Color32::from_rgb(0xB8, 0x43, 0x2F);

const SEAL_ZHU_DARK: Color32 = Color32::from_rgb(0xE0, 0x6A, 0x52);

const DANGER: Color32 = Color32::from_rgb(0xC6, 0x28, 0x28);

pub(crate) mod alpha {
    pub const SEAL_LINE: f32 = 0.55;
    pub const SHIMMER_MIN: f32 = 0.15;
    pub const SHIMMER_MAX: f32 = 0.375;
    pub const DOT_MIN: f32 = 0.35;
    pub const DOT_MAX: f32 = 0.7;
}

pub(crate) const SYNTAX_THEME_DARK: &str = "base16-mocha.dark";

pub(crate) const SYNTAX_THEME_LIGHT: &str = "Solarized (light)";

pub(crate) fn with_alpha(color: Color32, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        color.r(),
        color.g(),
        color.b(),
        (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}
