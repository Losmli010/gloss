//! 样式 token：浮层与设置窗共用的字号、间距、圆角、线宽与颜色阶梯。
//!
//! 每个常量是一档视觉定稿——改值即改两窗口观感，快照基线随之重录；
//! 同一语义只允许引用这里的 token，不写内联数值。窗口私有量（浮层宽度
//! 档位、输入框等控件宽度、动画时长、单点绘制几何）留在各自模块，不进
//! 阶梯。

/// 字号阶梯（px，从大到小六档）。
pub mod font {
    /// 词条（词卡首行）。
    pub const WORD: f32 = 18.0;
    /// 标题（正文标题、自检强调行）。
    pub const TITLE: f32 = 15.0;
    /// 正文与释义。
    pub const BODY: f32 = 14.0;
    /// 次要说明（流式来源、失败提示、音标、自检行）。
    pub const NOTICE: f32 = 13.0;
    /// 弱化小字（动作按钮、词性、例句、头部品牌标签、设置行内提示）。
    pub const CAPTION: f32 = 12.0;
    /// 标签（任务类型标签、输入源标签）。
    pub const TAG: f32 = 11.0;
}

/// 间距阶梯（px，从大到小六档）。
pub mod space {
    /// 分区之间：浮层标题行与正文分区之间。
    pub const SECTION: f32 = 12.0;
    /// 区块之间：设置窗大区块之间、词卡词条行与释义区之间。
    pub const GROUP: f32 = 10.0;
    /// 段落之间：浮层正文分区之间、设置网格列距。
    pub const PARAGRAPH: f32 = 8.0;
    /// 条目之间：释义组之间、设置网格行距、设置提示行与滚动区之间。
    pub const ITEM: f32 = 6.0;
    /// 紧邻元素：设置子分组之间、设置提示行与动作行之间。
    pub const TIGHT: f32 = 4.0;
    /// 行内：例句行之间。
    pub const INLINE: f32 = 2.0;
    /// 浮层卡片 Frame 的内边距。
    pub const CARD_PADDING: i8 = 14;
}

/// 圆角。
pub mod radius {
    /// 浮层卡片圆角。
    pub const CARD: u8 = 8;
}

/// 线宽。
pub mod stroke {
    /// 浮层卡片框线宽。
    pub const CARD: f32 = 0.5;
}

/// 颜色语义 token。
pub mod color {
    use egui::Color32;

    /// 品牌强调色（珊瑚橙）：浮层身份圆点、设置提示文案。
    pub const ACCENT: Color32 = Color32::from_rgb(0xD8, 0x5A, 0x30);
}

#[cfg(test)]
mod tests {
    use super::{font, space};

    #[test]
    fn ladders_are_strictly_descending() {
        let font_ladder = [
            font::WORD,
            font::TITLE,
            font::BODY,
            font::NOTICE,
            font::CAPTION,
            font::TAG,
        ];
        let space_ladder = [
            space::SECTION,
            space::GROUP,
            space::PARAGRAPH,
            space::ITEM,
            space::TIGHT,
            space::INLINE,
        ];
        for (name, ladder) in [("font", font_ladder), ("space", space_ladder)] {
            for pair in ladder.windows(2) {
                assert!(
                    pair[0] > pair[1],
                    "{name} ladder must be strictly descending: {ladder:?}"
                );
            }
        }
    }
}
