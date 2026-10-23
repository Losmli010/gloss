//! 浮层文案整形：水印等展示文案的纯函数，以及经注疏的字号规格与宋楷
//! 字体助手（demo 基准的应用定稿值，不进 style 阶梯）。
//!
//! 不依赖 egui 绘制侧：`FontId` 只是字体数据，不碰 `Ui`/`Context`。

use egui::FontId;

use crate::ui::fonts;

pub(super) const JING_FONT: f32 = 15.5;
pub(super) const ZHU_FONT: f32 = 15.0;
pub(super) const SHU_FONT: f32 = 14.0;
pub(super) const WORD_FONT: f32 = 25.0;
pub(super) const PHON_FONT: f32 = 13.0;
pub(super) const SEAL_FONT: f32 = 12.0;
pub(super) const SMALL_FONT: f32 = 13.0;

pub(super) fn serif_font(size: f32) -> FontId {
    FontId::new(size, fonts::serif_family())
}

pub(super) fn kaiti_font(size: f32) -> FontId {
    FontId::new(size, fonts::zhu_family())
}

pub(super) fn watermark() -> &'static str {
    "Gloss"
}

#[cfg(test)]
mod tests {
    use super::watermark;

    #[test]
    fn watermark_is_the_untranslated_brand_name() {
        assert_eq!(watermark(), "Gloss");
    }
}
