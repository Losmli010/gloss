//! 样式统一出口与解析点：浮层与设置窗共用的设计档位——颜色在此按明暗
//! 解析成两份数据（全仓唯一读取明暗态的地方），字号、间距、圆角与线宽
//! 阶梯在档位模块汇出。

mod palette;
mod tokens;

pub(crate) use palette::{
    DARK, LIGHT, Palette, SYNTAX_THEME_DARK, SYNTAX_THEME_LIGHT, SyntaxPalette, alpha, with_alpha,
};
pub(crate) use tokens::{font, radius, space, stroke};

pub(crate) fn palette(ui: &egui::Ui) -> &'static Palette {
    if ui.visuals().dark_mode {
        &palette::DARK
    } else {
        &palette::LIGHT
    }
}
