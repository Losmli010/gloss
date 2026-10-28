//! 语言体例违约的类别、字段位置与行式渲染：判定器、修复指令与日志的
//! 共同词汇表。类别标识是稳定字符串（进日志与修复清单），动作措辞按
//! 提示词语言本地化；违约信息只带字段位置与规则标识，不带回复原文。

use crate::model::Locale;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum LangCategory {
    ForeignRun,
    TraditionalChar,
    PunctuationMismatch,
    MissingSpace,
    EmojiForbidden,
    MarkdownForbidden,
    UnclosedContainer,
    ContainerMixing,
    SentenceInParens,
    SourceOverlap,
    PhoneticCharset,
}

impl LangCategory {
    pub fn rule_id(self) -> &'static str {
        match self {
            Self::ForeignRun => "ForeignRun",
            Self::TraditionalChar => "TraditionalChar",
            Self::PunctuationMismatch => "PunctuationMismatch",
            Self::MissingSpace => "MissingSpace",
            Self::EmojiForbidden => "EmojiForbidden",
            Self::MarkdownForbidden => "MarkdownForbidden",
            Self::UnclosedContainer => "UnclosedContainer",
            Self::ContainerMixing => "ContainerMixing",
            Self::SentenceInParens => "SentenceInParens",
            Self::SourceOverlap => "SourceOverlap",
            Self::PhoneticCharset => "PhoneticCharset",
        }
    }

    pub fn action(self, locale: Locale) -> &'static str {
        match (self, locale) {
            (Self::ForeignRun, Locale::Zh) => "将外文词移入括号并补目标语译名，或直接译出", // i18n:allow 修复动作措辞，面向模型的指令行，非界面文案
            (Self::ForeignRun, Locale::En) => {
                "Move the foreign words into parentheses with a target-language gloss, or \
                 translate them outright"
            }
            (Self::TraditionalChar, Locale::Zh) => "转为简体字", // i18n:allow 修复动作措辞，面向模型的指令行，非界面文案
            (Self::TraditionalChar, Locale::En) => "Convert to simplified characters",
            (Self::PunctuationMismatch, Locale::Zh) => "换成目标语言的标点", // i18n:allow 修复动作措辞，面向模型的指令行，非界面文案
            (Self::PunctuationMismatch, Locale::En) => {
                "Replace with the target language's punctuation"
            }
            (Self::MissingSpace, Locale::Zh) => "在中文与西文、数字之间补一个半角空格", // i18n:allow 修复动作措辞，面向模型的指令行，非界面文案
            (Self::MissingSpace, Locale::En) => {
                "Insert a half-width space between Chinese and Latin words or digits"
            }
            (Self::EmojiForbidden, Locale::Zh) => "删除表情符号", // i18n:allow 修复动作措辞，面向模型的指令行，非界面文案
            (Self::EmojiForbidden, Locale::En) => "Delete the emoji",
            (Self::MarkdownForbidden, Locale::Zh) => "去掉 markdown 记号，改为平实文字", // i18n:allow 修复动作措辞，面向模型的指令行，非界面文案
            (Self::MarkdownForbidden, Locale::En) => {
                "Remove the markdown markers and use plain text"
            }
            (Self::UnclosedContainer, Locale::Zh) => "补全配对，或删除残缺记号", // i18n:allow 修复动作措辞，面向模型的指令行，非界面文案
            (Self::UnclosedContainer, Locale::En) => {
                "Complete the pairing, or delete the broken marker"
            }
            (Self::ContainerMixing, Locale::Zh) => "容器内只保留单一语言，译文移入内层括号", // i18n:allow 修复动作措辞，面向模型的指令行，非界面文案
            (Self::ContainerMixing, Locale::En) => {
                "Keep a single language inside the container; move the translation into an inner \
                 parenthesis"
            }
            (Self::SentenceInParens, Locale::Zh) => "译出后括号内只留短语级原文", // i18n:allow 修复动作措辞，面向模型的指令行，非界面文案
            (Self::SentenceInParens, Locale::En) => {
                "Translate it and keep only a phrase-level quotation inside the parentheses"
            }
            (Self::SourceOverlap, Locale::Zh) => "该段未翻译，重译为目标语言", // i18n:allow 修复动作措辞，面向模型的指令行，非界面文案
            (Self::SourceOverlap, Locale::En) => {
                "This passage is untranslated; retranslate it into the target language"
            }
            (Self::PhoneticCharset, Locale::Zh) => "音标改用拉丁字母与国际音标符号", // i18n:allow 修复动作措辞，面向模型的指令行，非界面文案
            (Self::PhoneticCharset, Locale::En) => {
                "Rewrite the phonetics with Latin letters and IPA symbols"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LangField {
    Note,
    Phonetic,
    Example(usize),
}

impl LangField {
    fn label(self, locale: Locale) -> String {
        match (self, locale) {
            (Self::Note, _) => "note".to_owned(),
            (Self::Phonetic, _) => "phonetic".to_owned(),
            (Self::Example(index), Locale::Zh) => format!("examples 第 {} 条", index + 1), // i18n:allow 修复动作措辞，面向模型的指令行，非界面文案
            (Self::Example(index), Locale::En) => format!("examples item {}", index + 1),
        }
    }

    fn key(self) -> String {
        match self {
            Self::Note => "note".to_owned(),
            Self::Phonetic => "phonetic".to_owned(),
            Self::Example(index) => format!("examples[{index}]"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LangViolation {
    pub field: LangField,
    pub category: LangCategory,
}

impl LangViolation {
    pub fn new(field: LangField, category: LangCategory) -> Self {
        Self { field, category }
    }

    pub fn line(self, locale: Locale) -> String {
        match locale {
            Locale::Zh => format!(
                "{}：{}——{}", // i18n:allow 修复动作措辞，面向模型的指令行，非界面文案
                self.field.label(locale),
                self.category.rule_id(),
                self.category.action(locale)
            ),
            Locale::En => format!(
                "{}: {} — {}", // i18n:allow 修复动作措辞，面向模型的指令行，非界面文案
                self.field.label(locale),
                self.category.rule_id(),
                self.category.action(locale)
            ),
        }
    }

    pub fn summary(violations: &[LangViolation]) -> String {
        let mut kinds: Vec<&'static str> = violations
            .iter()
            .map(|violation| violation.category.rule_id())
            .collect();
        kinds.sort_unstable();
        kinds.dedup();
        format!("{} violation kind(s): {}", kinds.len(), kinds.join(", "))
    }
}

impl std::fmt::Display for LangViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at {}", self.category.rule_id(), self.field.key())
    }
}
