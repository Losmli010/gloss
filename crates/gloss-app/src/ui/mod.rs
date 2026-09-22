//! 浮层与设置窗口的 UI：内容渲染模板与字体。

pub mod fonts;
pub mod popup;
pub mod settings;
pub mod style;

use gloss_core::task::TaskKind;

use crate::i18n::Text;

/// 任务类型 → 界面标签（结果卡头部与设置页共用一张表，改一处两处同步）。
pub(crate) fn kind_label(kind: TaskKind, text: &Text) -> &str {
    match kind {
        TaskKind::TranslateWord => &text.kinds.translate_word,
        TaskKind::TranslateSentence => &text.kinds.translate_sentence,
        TaskKind::ExplainCode => &text.kinds.explain_code,
        TaskKind::ImageOcr => &text.kinds.image_ocr,
        TaskKind::ImageExplain => &text.kinds.image_explain,
    }
}
