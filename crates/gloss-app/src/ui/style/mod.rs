//! 样式统一出口与解析点：浮层与设置窗共用的设计档位——颜色在此按明暗
//! 解析成两份数据（全仓唯一读取明暗态的地方），字号与角色构造器在字体
//! 排印模块、间距与描边阶梯在档位模块汇出。
//!
//! 空间尺寸的唯一设计单位是根相对字号（下称空间单位），解析根在此一处
//! 落成像素：将来接用户字号或缩放设置时，这里 是唯一改动点。

pub(crate) mod geometry;
pub(crate) mod motion;
mod palette;
mod tokens;
mod typography;

pub(crate) use palette::{
    DARK, LIGHT, Palette, SYNTAX_THEME_DARK, SYNTAX_THEME_LIGHT, SyntaxPalette, alpha, with_alpha,
};
pub(crate) use tokens::{radius, space, stroke};
pub(crate) use typography::{apply_kaiti, line_height, mono, sans, serif, size};

const REM_ROOT_PX: f32 = 12.0;

const fn rem(units: f32) -> f32 {
    units * REM_ROOT_PX
}

pub(crate) fn palette(ui: &egui::Ui) -> &'static Palette {
    if ui.visuals().dark_mode {
        &palette::DARK
    } else {
        &palette::LIGHT
    }
}
