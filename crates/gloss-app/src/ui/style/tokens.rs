//! 样式档位：浮层与设置窗共用的字号、间距、圆角与线宽阶梯。
//!
//! 每个常量是一档视觉定稿——改值即改两窗口观感，快照基线随之重录；
//! 同一语义只允许引用这里的档位常量，不写内联数值。窗口与控件私有量
//! 留在各自模块，不进阶梯。

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
