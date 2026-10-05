//! 浮层文案整形：例句拆行、提取小记与水印等展示文案的纯函数，以及经注疏
//! 的字号规格与宋楷字体助手（demo 定稿值，不进 style 阶梯）。
//!
//! 不依赖 egui 绘制侧：`FontId`/`RichText` 只是文本数据，不碰
//! `Ui`/`Context`。

use egui::{FontId, RichText};

use crate::ui::fonts;
use crate::ui::i18n::{Text, fill};

/// 经注疏排版的字号（demo 定稿值）：经 15.5、注 15、疏 12.5；词条 25，
/// 音标/词性 13，印章字 12。
pub(super) const JING_FONT: f32 = 15.5;
pub(super) const ZHU_FONT: f32 = 15.0;
pub(super) const SHU_FONT: f32 = 12.5;
pub(super) const WORD_FONT: f32 = 25.0;
pub(super) const PHON_FONT: f32 = 13.0;
pub(super) const SEAL_FONT: f32 = 12.0;

/// 宋体字（经/疏/印章）。
pub(super) fn serif_font(size: f32) -> FontId {
    FontId::new(size, fonts::serif_family())
}

/// 楷体字（注）。
pub(super) fn kaiti_font(size: f32) -> FontId {
    FontId::new(size, fonts::zhu_family())
}

/// 提取小记（疏）：「凡 N 言 · N 行」，字数按去空白计、行数按换行计，
/// 与 demo 定稿的口径一致。
pub(super) fn extract_note(catalog: &Text, text: &str) -> RichText {
    let chars = text.chars().filter(|ch| !ch.is_whitespace()).count();
    let lines = text.lines().count().max(1);
    let chars = chars.to_string();
    let lines = lines.to_string();
    RichText::new(fill(
        &catalog.gloss_popup_seal_note,
        &[("chars", &chars), ("lines", &lines)],
    ))
    .font(serif_font(SHU_FONT))
    .weak()
}

/// 例句拆分：在首个 CJK 字形处切成「原文 / 译文」两行（demo w-ex 的
/// `.en` 行 + `.zh` 块）；没有 CJK 段的原样单行返回。
pub(super) fn example_lines(example: &str) -> (&str, Option<&str>) {
    fn is_cjk(ch: char) -> bool {
        matches!(ch as u32,
            0x3000..=0x303F // CJK 符号与标点
            | 0x3400..=0x4DBF // 扩展 A
            | 0x4E00..=0x9FFF // 基本区
            | 0xF900..=0xFAFF // 兼容表意
            | 0xFF00..=0xFFEF // 全角形式
        )
    }
    match example.char_indices().find(|(_, ch)| is_cjk(*ch)) {
        Some((byte, _)) if !example[..byte].trim().is_empty() => (
            example[..byte].trim_end(),
            Some(example[byte..].trim_start()),
        ),
        _ => (example, None),
    }
}

/// 页脚水印的品牌名：应用名不翻译（与窗口标题同一原则）。
pub(super) fn watermark() -> &'static str {
    "Gloss"
}

#[cfg(test)]
mod tests {
    use super::{example_lines, watermark};

    #[test]
    fn example_lines_split_at_the_first_cjk_glyph() {
        let (en, zh) =
            example_lines("The polished wood had a deep gloss. 那块抛光的木料泛着深沉的光泽。");
        assert_eq!(en, "The polished wood had a deep gloss.");
        assert_eq!(zh, Some("那块抛光的木料泛着深沉的光泽。"));

        let (en, zh) = example_lines("a gloss of silk");
        assert_eq!(en, "a gloss of silk");
        assert_eq!(zh, None);

        let (en, zh) = example_lines("光泽");
        assert_eq!(en, "光泽");
        assert_eq!(zh, None);

        let (en, zh) = example_lines("英译。中译");
        assert_eq!(en, "英译。中译");
        assert_eq!(zh, None, "开头即 CJK 的例句不拆");
    }

    #[test]
    fn watermark_is_the_untranslated_brand_name() {
        assert_eq!(watermark(), "Gloss");
    }
}
