//! 着色类别：六分类与斜体规则，颜色从语义调色板的语法色板取。

use egui::Color32;

use crate::ui::style::SyntaxPalette;

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
    pub(crate) fn pick(self, syntax: &SyntaxPalette) -> Color32 {
        match self {
            Class::Keyword => syntax.keyword,
            Class::String => syntax.string,
            Class::Comment => syntax.comment,
            Class::Number => syntax.number,
            Class::Type => syntax.r#type,
            Class::Function => syntax.function,
        }
    }

    pub(crate) fn italic(self) -> bool {
        matches!(self, Class::Comment)
    }
}

#[cfg(test)]
mod tests {
    use crate::ui::style::{DARK, LIGHT};

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
        for syntax in [&LIGHT.syntax, &DARK.syntax] {
            let colors: Vec<_> = classes.iter().map(|class| class.pick(syntax)).collect();
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
