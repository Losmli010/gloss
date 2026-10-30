//! 判定常量表：任务策略与目标语言策略——任务的（markdown）能力与容器
//! 开关、合法文字系统、标点集、括号对、盘古之白与简体判定开关，以及文
//! 字系统的区段归类。字符事实全部派生自语言规范数据源，本模块只按
//! 目标语言组合策略；音标字符集是任务维的机制面，仍住本模块。
//!
//! 目标语言收敛为中文与英文两变体，策略分支一一对应；假名与谚文区段
//! 仍归文字系统判定（混语识别与目标语言支持正交）。

use crate::langspec;
use crate::model::Lang;
use crate::task::TaskKind;

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
    pub(crate) pangu: bool,
    pub(crate) simplified: bool,
}

pub(crate) struct TaskPolicy {
    pub(crate) markdown_allowed: bool,
    pub(crate) quote_container: bool,
}

pub(crate) fn lang_policy(target: &Lang) -> LangPolicy {
    let spec = langspec::spec();
    match target {
        Lang::Zh => LangPolicy {
            scripts: &[ScriptClass::Han],
            punctuation: &spec.zh_allow,
            paren_pairs: &spec.brackets_zh,
            pangu: spec.pangu.zh,
            simplified: spec.simplified.zh,
        },
        Lang::En => LangPolicy {
            scripts: &[ScriptClass::Latin],
            punctuation: &spec.en_allow,
            paren_pairs: &spec.brackets_en,
            pangu: spec.pangu.en,
            simplified: spec.simplified.en,
        },
    }
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
    if c.is_ascii_alphabetic() {
        return ScriptClass::Latin;
    }
    let code = c as u32;
    let ranges = &langspec::spec().ranges;
    if langspec::range_contains(&ranges.kana, code) {
        ScriptClass::Kana
    } else if langspec::range_contains(&ranges.han, code) {
        ScriptClass::Han
    } else if langspec::range_contains(&ranges.hangul, code) {
        ScriptClass::Hangul
    } else if langspec::range_contains(&ranges.cyrillic, code) {
        ScriptClass::Cyrillic
    } else if langspec::range_contains(&ranges.greek, code) {
        ScriptClass::Greek
    } else if langspec::range_contains(&ranges.latin, code) {
        ScriptClass::Latin
    } else {
        ScriptClass::OtherLetter
    }
}
