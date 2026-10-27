//! 字号档位与角色构造器：字号的角色定稿（空间单位档位）与族乘字号的
//! 唯一构造点——经与词头走宋、注与疏走楷、代码与音标走等宽、徽标走
//! 内置比例体，这条核心设计决策只写在本模块；标记文档的楷体改排
//! （界面库文本样式到楷体的映射）也在此收口。
//!
//! 角色独立靠命名不靠数值互异：通用阶梯（标题、说明、小字、标签）与
//! 经注疏阶梯（词头、经、注、疏、音标、印、代码、徽标）分表维护，
//! 改一处的档不动另一处的角色。行高比率是无单位倍率，不随空间单位
//! 缩放。

use egui::{FontId, TextStyle, Ui};

use super::rem;
use crate::ui::fonts;

pub mod size {
    use super::rem;

    pub const WORD: f32 = rem(2.0);
    pub const TITLE: f32 = rem(1.25);
    pub const JING: f32 = rem(1.25);
    pub const ZHU: f32 = rem(1.25);
    pub const SHU: f32 = rem(1.125);
    pub const NOTICE: f32 = rem(1.125);
    pub const PHON: f32 = rem(1.125);
    pub const CAPTION: f32 = rem(1.0);
    pub const SEAL: f32 = rem(1.0);
    pub const CODE: f32 = rem(1.0);
    pub const TAG: f32 = rem(0.875);
    pub const CODE_BADGE: f32 = rem(0.875);
}

pub mod line_height {
    pub const CODE: f32 = 1.65;
}

pub fn serif(size_px: f32) -> FontId {
    FontId::new(size_px, fonts::serif_family())
}

pub fn kai(size_px: f32) -> FontId {
    FontId::new(size_px, fonts::zhu_family())
}

pub fn mono(size_px: f32) -> FontId {
    FontId::new(size_px, fonts::mono_family())
}

pub fn sans(size_px: f32) -> FontId {
    FontId::new(size_px, egui::FontFamily::Proportional)
}

pub fn apply_kaiti(ui: &mut Ui, body_px: f32) {
    for (style, px) in [
        (TextStyle::Body, body_px),
        (TextStyle::Button, body_px),
        (TextStyle::Small, size::NOTICE),
        (TextStyle::Heading, size::JING),
    ] {
        ui.style_mut().text_styles.insert(style, kai(px));
    }
}

#[cfg(test)]
mod tests {
    use super::size;

    #[test]
    fn size_ladder_is_strictly_descending() {
        let size_ladder = [size::WORD, size::TITLE, size::SHU, size::CAPTION, size::TAG];
        for pair in size_ladder.windows(2) {
            assert!(
                pair[0] > pair[1],
                "size ladder must be strictly descending: {size_ladder:?}"
            );
        }
    }
}
