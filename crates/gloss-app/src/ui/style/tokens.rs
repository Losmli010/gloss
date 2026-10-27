//! 空间档位：间距、圆角与描边的阶梯，以空间单位定稿。
//!
//! 每个常量是一档视觉定稿——改值即改两窗口观感，快照基线随之重录；
//! 同一语义只允许引用这里的档位常量，不写内联数值。间距与圆角以空间
//! 单位表达、随字号缩放；描边与发丝线是像素例外——线宽是渲染意图，
//! 不随字号缩放，但档位同样收编在此统一登记。

use super::rem;

pub mod space {
    use super::rem;

    pub const SECTION: f32 = rem(1.0);
    pub const CARD_PADDING: f32 = rem(1.125);
    pub const PARAGRAPH: f32 = rem(0.625);
    pub const ITEM: f32 = rem(0.5);
    pub const TIGHT: f32 = rem(0.375);
    pub const INLINE: f32 = rem(0.125);
}

pub mod radius {
    use super::rem;

    pub const ICON: f32 = rem(0.125);
    pub const SEAL: f32 = rem(0.375);
    pub const SHIMMER: f32 = rem(0.375);
    pub const CARD: f32 = rem(0.625);
    pub const CODE_PANEL: f32 = rem(0.875);
}

pub mod stroke {
    pub const HAIRLINE: f32 = 0.5;
    pub const LINE: f32 = 1.0;
    pub const UNDERLINE: f32 = 1.5;
    pub const THICK: f32 = 2.0;
}

#[cfg(test)]
mod tests {
    use super::space;

    #[test]
    fn space_ladder_is_strictly_descending() {
        let space_ladder = [
            space::SECTION,
            space::PARAGRAPH,
            space::ITEM,
            space::TIGHT,
            space::INLINE,
        ];
        for pair in space_ladder.windows(2) {
            assert!(
                pair[0] > pair[1],
                "space ladder must be strictly descending: {space_ladder:?}"
            );
        }
    }
}
