//! 浮层与设置窗口的 UI：内容渲染模板与字体。

pub mod fonts;
pub mod popup;
pub mod settings;

use gloss_core::task::TaskKind;

/// 任务类型 → 中文标签（结果卡头部与设置页共用一张表，改一处两处同步）。
pub(crate) fn kind_label(kind: TaskKind) -> &'static str {
    match kind {
        TaskKind::TranslateWord => "词卡",
        TaskKind::TranslateSentence => "翻译",
        TaskKind::ExplainCode => "代码解释",
        TaskKind::ImageOcr => "提取结果",
        TaskKind::ImageExplain => "图片解释",
    }
}
