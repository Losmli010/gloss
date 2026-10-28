//! 语言体例违约的类别、字段位置与行式渲染：判定器、修复指令与日志的
//! 共同词汇表。类别标识是稳定字符串（进日志与修复清单）；动作措辞住
//! 提示词库的措辞注册表，本模块只产结构事实——违约行只带字段位置与
//! 规则标识，显示形态只带规则标识与字段键，都不带回复原文。

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LangField {
    Note,
    Phonetic,
    Example(usize),
}

impl LangField {
    pub(crate) fn key(self) -> String {
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

    pub fn line(self) -> String {
        format!("{}: {}", self.field.key(), self.category.rule_id())
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
