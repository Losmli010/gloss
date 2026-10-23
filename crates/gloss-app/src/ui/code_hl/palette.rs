//! 着色类别与 `demo` 调色板：六分类的明暗两套色与斜体规则。

use egui::Color32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    Keyword,
    String,
    Comment,
    Number,
    Type,
    Function,
}

impl Class {
    pub(crate) fn color(self, dark: bool) -> Color32 {
        match (self, dark) {
            (Class::Keyword, true) => Color32::from_rgb(0xC7, 0x92, 0xEA),
            (Class::String, true) => Color32::from_rgb(0xA5, 0xD6, 0xA7),
            (Class::Comment, true) => Color32::from_rgb(0x6D, 0x76, 0x83),
            (Class::Number, true) => Color32::from_rgb(0xF0, 0xB4, 0x52),
            (Class::Type, true) => Color32::from_rgb(0x66, 0xC7, 0xD4),
            (Class::Function, true) => Color32::from_rgb(0x7C, 0xB8, 0xEC),
            (Class::Keyword, false) => Color32::from_rgb(0x8E, 0x44, 0xAD),
            (Class::String, false) => Color32::from_rgb(0x1E, 0x7E, 0x34),
            (Class::Comment, false) => Color32::from_rgb(0x9A, 0xA1, 0xAC),
            (Class::Number, false) => Color32::from_rgb(0xB4, 0x53, 0x09),
            (Class::Type, false) => Color32::from_rgb(0x0E, 0x7C, 0x8C),
            (Class::Function, false) => Color32::from_rgb(0x2B, 0x6C, 0xB0),
        }
    }

    pub(crate) fn italic(self) -> bool {
        matches!(self, Class::Comment)
    }
}

pub(crate) fn plain_color(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0xE6, 0xE6, 0xE6)
    } else {
        Color32::from_rgb(0x24, 0x29, 0x2E)
    }
}

#[cfg(test)]
mod tests {
    use super::Class;

    #[test]
    fn every_class_has_its_own_color_per_theme() {
        let classes = [
            Class::Keyword,
            Class::String,
            Class::Comment,
            Class::Number,
            Class::Type,
            Class::Function,
        ];
        for dark in [true, false] {
            let colors: Vec<_> = classes.iter().map(|class| class.color(dark)).collect();
            for (index, left) in colors.iter().enumerate() {
                for right in colors.iter().skip(index + 1) {
                    assert_ne!(left, right, "classes must stay visually distinct");
                }
            }
        }
        assert!(Class::Comment.italic());
        assert!(!Class::Keyword.italic());
    }
}
