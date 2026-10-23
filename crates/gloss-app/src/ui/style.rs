//! 样式 `token`：浮层与设置窗共用的字号、间距、圆角、线宽与颜色阶梯。
//!
//! 每个常量是一档视觉定稿——改值即改两窗口观感，快照基线随之重录；
//! 同一语义只允许引用这里的 `token`，不写内联数值。窗口/控件私有量（浮层
//! 宽度档位、输入框等控件宽度、动画时长、单点绘制几何、头部动作图标
//! 字形尺寸、设置窗内边距与动作行高、开关钮尺寸、错误下划线宽、按钮
//! 圆角）留在各自模块，不进阶梯。

pub mod font {
    pub const TITLE: f32 = 15.0;
    pub const NOTICE: f32 = 13.0;
    pub const CAPTION: f32 = 12.0;
    pub const TAG: f32 = 11.0;
}

pub mod space {
    pub const SECTION: f32 = 12.0;
    pub const PARAGRAPH: f32 = 8.0;
    pub const ITEM: f32 = 6.0;
    pub const TIGHT: f32 = 4.0;
    pub const INLINE: f32 = 2.0;
    pub const CARD_PADDING: i8 = 14;
}

pub mod radius {
    pub const CARD: u8 = 8;
}

pub mod stroke {
    pub const CARD: f32 = 0.5;
}

pub mod color {
    use egui::Color32;

    pub const ACCENT_LIGHT: Color32 = SEAL_ZHU_LIGHT;

    pub const ACCENT_DARK: Color32 = SEAL_ZHU_DARK;

    pub const DANGER: Color32 = Color32::from_rgb(0xC6, 0x28, 0x28);

    pub const SEAL_ZHU_LIGHT: Color32 = Color32::from_rgb(0xB8, 0x43, 0x2F);

    pub const SEAL_ZHU_DARK: Color32 = Color32::from_rgb(0xE0, 0x6A, 0x52);

    pub const CODE_BG_LIGHT: Color32 = Color32::from_rgb(0xF5, 0xF7, 0xF9);

    pub const CODE_BG_DARK: Color32 = Color32::from_rgb(0x16, 0x18, 0x1E);

    pub const CODE_BORDER_LIGHT: Color32 = Color32::from_rgba_premultiplied(2, 2, 3, 26);

    pub const CODE_BORDER_DARK: Color32 = Color32::from_rgba_premultiplied(23, 23, 23, 23);
}

#[cfg(test)]
mod tests {
    use super::{font, space};

    #[test]
    fn ladders_are_strictly_descending() {
        let font_ladder = [font::TITLE, font::NOTICE, font::CAPTION, font::TAG];
        let space_ladder = [
            space::SECTION,
            space::PARAGRAPH,
            space::ITEM,
            space::TIGHT,
            space::INLINE,
        ];
        for (name, ladder) in [
            ("font", font_ladder.as_slice()),
            ("space", space_ladder.as_slice()),
        ] {
            for pair in ladder.windows(2) {
                assert!(
                    pair[0] > pair[1],
                    "{name} ladder must be strictly descending: {ladder:?}"
                );
            }
        }
    }
}
