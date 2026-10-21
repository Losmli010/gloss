//! 浮层文案整形：水印等展示文案的纯函数，以及经注疏的字号规格与宋楷
//! 字体助手（demo 基准的应用定稿值，不进 style 阶梯）。
//!
//! 不依赖 egui 绘制侧：`FontId` 只是字体数据，不碰
//! `Ui`/`Context`。

use egui::FontId;

use crate::ui::fonts;

/// 经注疏排版的字号（demo 基准，应用侧定稿）：经 15.5、注 15、疏 14；
/// 词条 25，音标/词性 13，印章字 12，其余小字 13。
pub(super) const JING_FONT: f32 = 15.5;
pub(super) const ZHU_FONT: f32 = 15.0;
pub(super) const SHU_FONT: f32 = 14.0;
pub(super) const WORD_FONT: f32 = 25.0;
pub(super) const PHON_FONT: f32 = 13.0;
pub(super) const SEAL_FONT: f32 = 12.0;
/// 小字档：注区 markdown 的 Small 样式与经位占位。与 [`SHU_FONT`] 解耦，
/// 疏正文调号不牵动它们。
pub(super) const SMALL_FONT: f32 = 13.0;

/// 宋体字（经/疏/印章）。
pub(super) fn serif_font(size: f32) -> FontId {
    FontId::new(size, fonts::serif_family())
}

/// 楷体字（注/疏的 markdown 正文）。
pub(super) fn kaiti_font(size: f32) -> FontId {
    FontId::new(size, fonts::zhu_family())
}

/// 页脚水印的品牌名：应用名不翻译（与窗口标题同一原则）。
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
