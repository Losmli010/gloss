//! 判定常量表：任务策略与目标语言策略——任务的（markdown）能力与容器
//! 开关、合法文字系统、标点集、括号对、盘古之白与简体判定开关，以及
//! 文字系统的区段归类。全部为模块内常量，无运行时配置。
//!
//! 日语、韩语与自定义目标语言的标点集按宁漏勿误口径取中西并集（第一版
//! 只判串味重灾的中西标点互串），文字系统判定照常收口。中文标点集不收
//! 角引号，英文标点集不收引号且引文容器对英文目标不启用。

use crate::model::Lang;
use crate::task::TaskKind;

// i18n:allow 判定常量

// i18n:allow 判定常量

#[rustfmt::skip]
pub(crate) const ZH_PUNCT: &[char] = &['，', '。', '、', '；', '：', '？', '！', '“', '”', '‘', '’', '《', '》', '（', '）', '·']; // i18n:allow 判定常量
#[rustfmt::skip]
pub(crate) const EN_PUNCT: &[char] = &[',', '.', ';', ':', '!', '?', '\'', '(', ')', '-'];
#[rustfmt::skip]
const JA_PUNCT: &[char] = &['，', '。', '、', '；', '：', '？', '！', '“', '”', '‘', '’', '《', '》', '（', '）', '「', '」', '『', '』', '·', 'ー', '・', '～', ',', '.', ';', ':', '!', '?', '"', '\'', '(', ')', '-']; // i18n:allow 判定常量
#[rustfmt::skip]
const KO_PUNCT: &[char] = &['，', '。', '、', '；', '：', '？', '！', '“', '”', '‘', '’', '《', '》', '（', '）', '「', '」', '『', '』', '·', ',', '.', ';', ':', '!', '?', '"', '\'', '(', ')', '-']; // i18n:allow 判定常量

const PARENS_ZH: &[(char, char)] = &[('（', '）')]; // i18n:allow 判定常量
const PARENS_EN: &[(char, char)] = &[('(', ')')];
const PARENS_BOTH: &[(char, char)] = &[('（', '）'), ('(', ')')]; // i18n:allow 判定常量

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScriptClass {
    Latin,
    Han,
    Kana,
    Hangul,
    Cyrillic,
    Greek,
    OtherLetter,
}

pub(crate) struct LangPolicy {
    pub(crate) scripts: &'static [ScriptClass],
    pub(crate) punctuation: &'static [char],
    pub(crate) paren_pairs: &'static [(char, char)],
    pub(crate) quote_container: bool,
    pub(crate) pangu: bool,
    pub(crate) simplified: bool,
}

pub(crate) struct TaskPolicy {
    pub(crate) markdown_allowed: bool,
    pub(crate) quote_container: bool,
}

pub(crate) fn lang_policy(target: &Lang) -> LangPolicy {
    match target {
        Lang::Zh => LangPolicy {
            scripts: &[ScriptClass::Han],
            punctuation: ZH_PUNCT,
            paren_pairs: PARENS_ZH,
            quote_container: true,
            pangu: true,
            simplified: true,
        },
        Lang::En | Lang::Fr => LangPolicy {
            scripts: &[ScriptClass::Latin],
            punctuation: EN_PUNCT,
            paren_pairs: PARENS_EN,
            quote_container: false,
            pangu: false,
            simplified: false,
        },
        Lang::Ja => LangPolicy {
            scripts: &[ScriptClass::Kana, ScriptClass::Han],
            punctuation: JA_PUNCT,
            paren_pairs: PARENS_BOTH,
            quote_container: true,
            pangu: false,
            simplified: false,
        },
        Lang::Ko => LangPolicy {
            scripts: &[ScriptClass::Hangul],
            punctuation: KO_PUNCT,
            paren_pairs: PARENS_BOTH,
            quote_container: true,
            pangu: false,
            simplified: false,
        },
        Lang::Other(_) => LangPolicy {
            scripts: &[],
            punctuation: KO_PUNCT,
            paren_pairs: PARENS_BOTH,
            quote_container: true,
            pangu: false,
            simplified: false,
        },
    }
}

pub(crate) fn quotes_enabled(lang: &LangPolicy, task: &TaskPolicy) -> bool {
    task.quote_container && lang.quote_container
}

pub(crate) fn task_policy(kind: TaskKind) -> TaskPolicy {
    match kind {
        TaskKind::ExplainCode => TaskPolicy {
            markdown_allowed: true,
            quote_container: false,
        },
        TaskKind::General => TaskPolicy {
            markdown_allowed: true,
            quote_container: true,
        },
        TaskKind::TranslateWord | TaskKind::TranslateSentence | TaskKind::ImageExplain => {
            TaskPolicy {
                markdown_allowed: false,
                quote_container: true,
            }
        }
    }
}

pub(crate) fn is_phonetic_char(c: char) -> bool {
    matches!(
        c as u32,
        0x41..=0x5A
            | 0x61..=0x7A
            | 0xC0..=0xFF
            | 0x100..=0x24F
            | 0x250..=0x2AF
            | 0x2B0..=0x2FF
            | 0x300..=0x36F
    ) || matches!(c, '/' | '[' | ']' | '(' | ')' | '.' | ' ' | ',')
}

pub(crate) fn script_class(c: char) -> ScriptClass {
    let code = c as u32;
    match code {
        0x3041..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9D => ScriptClass::Kana,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF => ScriptClass::Han,
        0x1100..=0x11FF | 0x3130..=0x318F | 0xAC00..=0xD7A3 => ScriptClass::Hangul,
        0x0400..=0x052F => ScriptClass::Cyrillic,
        0x0370..=0x03FF | 0x1F00..=0x1FFF => ScriptClass::Greek,
        _ if c.is_ascii_alphabetic() => ScriptClass::Latin,
        0x00C0..=0x024F | 0x1E00..=0x1EFF | 0x2C60..=0x2C7F | 0xA720..=0xA7FF => ScriptClass::Latin,
        _ if c.is_alphabetic() => ScriptClass::OtherLetter,
        _ => ScriptClass::OtherLetter,
    }
}
