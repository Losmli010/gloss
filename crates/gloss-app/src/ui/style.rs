//! 样式 token：浮层与设置窗共用的字号、间距、圆角、线宽与颜色阶梯。
//!
//! 每个常量是一档视觉定稿——改值即改两窗口观感，快照基线随之重录；
//! 同一语义只允许引用这里的 token，不写内联数值。窗口/控件私有量（浮层
//! 宽度档位、输入框等控件宽度、动画时长、单点绘制几何、头部动作图标
//! 字形尺寸、设置窗内边距与动作行高、开关钮尺寸、错误下划线宽、按钮
//! 圆角）留在各自模块，不进阶梯。

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
    /// 弱化小字（动作按钮、词性、例句、设置区块标与行内提示）。
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

    /// 经注疏印章的圆角（小圆角方块，近方而不刺）。
    pub const SEAL: u8 = 6;
}

/// 线宽。
pub mod stroke {
    /// 浮层卡片框线宽。
    pub const CARD: f32 = 0.5;
}

/// 颜色语义 token。
pub mod color {
    use egui::Color32;

    /// 品牌强调色（珊瑚橙）：设置保存主按钮与开关钮的着轨。
    pub const ACCENT: Color32 = Color32::from_rgb(0xD8, 0x5A, 0x30);

    /// 错误色：设置页校验失败的下划线与就地提示、校验汇总行。
    /// 与 ACCENT 同饱和度带的正红，明暗主题下均可读。
    pub const DANGER: Color32 = Color32::from_rgb(0xC6, 0x28, 0x28);

    /// 朱砂（浅色主题）：经注疏印章的实心印与朱丝栏的线色源。压暗到白底可读。
    pub const SEAL_ZHU_LIGHT: Color32 = Color32::from_rgb(0xB8, 0x43, 0x2F);

    /// 朱砂（深色主题）：同一色相提亮到深底可读。
    pub const SEAL_ZHU_DARK: Color32 = Color32::from_rgb(0xE0, 0x6A, 0x52);

    /// 代码面板底色（浅色主题）：经位代码面板专属底（demo code-bg #f5f7f9），
    /// 区别于卡片底的淡界栏。
    pub const CODE_BG_LIGHT: Color32 = Color32::from_rgb(0xF5, 0xF7, 0xF9);

    /// 代码面板底色（深色主题）：同一语义的深色档（demo code-bg #16181e）。
    pub const CODE_BG_DARK: Color32 = Color32::from_rgb(0x16, 0x18, 0x1E);

    /// 代码面板边色（浅色主题）：demo card-border 的 10% 墨色（预乘
    /// rgb(2,2,3)）——近隐形的定界，不与卡片框线争抢。
    pub const CODE_BORDER_LIGHT: Color32 = Color32::from_rgba_premultiplied(2, 2, 3, 26);

    /// 代码面板边色（深色主题）：demo card-border 的 9% 白（预乘）。
    pub const CODE_BORDER_DARK: Color32 = Color32::from_rgba_premultiplied(23, 23, 23, 23);
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
