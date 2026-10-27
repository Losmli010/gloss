//! 浮层文案整形：水印等展示文案的纯函数。
//!
//! 不依赖界面库（egui）绘制侧：字体句柄只是字体数据，不碰界面状态与上下文。
//! 经注疏的字号规格与宋楷字体助手已收编进样式排印模块（样式统一阶段 4）。

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
