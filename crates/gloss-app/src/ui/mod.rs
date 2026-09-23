//! 浮层与设置窗口的 UI：内容渲染模板、字体与上下文的统一装入点。

pub mod context;
pub mod fonts;
pub mod popup;
pub mod settings;
pub mod style;

use gloss_core::task::TaskKind;

use crate::i18n::Text;

/// 任务类型 → 界面标签（结果卡头部与设置页共用一张表，改一处两处同步）。
pub(crate) fn kind_label(kind: TaskKind, text: &Text) -> &str {
    match kind {
        TaskKind::TranslateWord => &text.gloss_kinds_translate_word,
        TaskKind::TranslateSentence => &text.gloss_kinds_translate_sentence,
        TaskKind::ExplainCode => &text.gloss_kinds_explain_code,
        TaskKind::ImageOcr => &text.gloss_kinds_image_ocr,
        TaskKind::ImageExplain => &text.gloss_kinds_image_explain,
    }
}
