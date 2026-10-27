//! 正文判定：容器内容与数学定界区之外的文本。文字系统（连续文字段按
//! 目标语言合法集判定，拉丁豁免仅单字母变量与紧跟句点的单字母缩写）、
//! 标点（目标标点集，容器定界符与数字附随符号豁免，破折号省略号家族
//! 不判）、盘古之白（仅中文）与表情符号。

use super::containers::script_runs;
use super::tables::{LangPolicy, ScriptClass, TaskPolicy};
use super::violation::{LangCategory, LangField, LangViolation};

const PERCENT_SIGNS: [char; 3] = ['%', '‰', '°']; // i18n:allow 判定常量
const CURRENCY_SIGNS: [char; 5] = ['€', '£', '¥', '￥', '₩']; // i18n:allow 判定常量
const DASH_FAMILY: [char; 4] = ['—', '–', '‥', '…']; // i18n:allow 判定常量

pub(crate) struct BodyCheck<'a> {
    pub lang: &'a LangPolicy,
    pub task: &'a TaskPolicy,
    pub field: LangField,
    pub chars: &'a [char],
    pub ranges: &'a [(usize, usize)],
}

pub(crate) fn body_ranges(
    len: usize,
    math: &[(usize, usize)],
    containers: &[(usize, usize)],
) -> Vec<(usize, usize)> {
    let mut masked: Vec<(usize, usize)> = math
        .iter()
        .copied()
        .chain(containers.iter().copied())
        .collect();
    masked.sort_unstable();
    let mut ranges = Vec::new();
    let mut cursor = 0;
    for (start, end) in masked {
        if start > cursor {
            ranges.push((cursor, start));
        }
        cursor = cursor.max(end);
    }
    if len > cursor {
        ranges.push((cursor, len));
    }
    ranges
}

impl BodyCheck<'_> {
    pub(crate) fn run(&self, violations: &mut Vec<LangViolation>) {
        self.check_runs(violations);
        self.check_punct(violations);
        if self.lang.pangu {
            self.check_pangu(violations);
        }
        self.check_emoji(violations);
    }

    fn check_runs(&self, violations: &mut Vec<LangViolation>) {
        if self.lang.scripts.is_empty() {
            return;
        }
        for run in script_runs(self.chars, self.ranges) {
            if self.lang.scripts.contains(&run.class) {
                continue;
            }
            let abbrev_dot = run.len == 1
                && run.class == ScriptClass::Latin
                && self.chars.get(run.end) == Some(&'.');
            if run.len >= 2 || abbrev_dot {
                violations.push(LangViolation::new(self.field, LangCategory::ForeignRun));
            }
        }
    }

    fn check_punct(&self, violations: &mut Vec<LangViolation>) {
        for &(start, end) in self.ranges {
            for index in start..end {
                let c = self.chars[index];
                if c.is_whitespace() || c.is_alphanumeric() || self.is_delimiter(c) {
                    continue;
                }
                if is_attached_symbol(self.chars, index) || is_dash_or_ellipsis(self.chars, index) {
                    continue;
                }
                if !self.lang.punctuation.contains(&c) {
                    violations.push(LangViolation::new(
                        self.field,
                        LangCategory::PunctuationMismatch,
                    ));
                }
            }
        }
    }

    pub(crate) fn is_delimiter(&self, c: char) -> bool {
        c == '$'
            || self
                .lang
                .paren_pairs
                .iter()
                .any(|&(open, close)| c == open || c == close)
            || (self.task.quote_container && matches!(c, '“' | '”' | '"')) // i18n:allow 判定常量
            || (self.task.markdown_allowed && c == '`')
    }

    fn check_pangu(&self, violations: &mut Vec<LangViolation>) {
        for &(start, end) in self.ranges {
            for pair in self.chars[start..end].windows(2) {
                let (left, right) = (pair[0], pair[1]);
                let mixed = (is_han(left) && right.is_ascii_alphanumeric())
                    || (left.is_ascii_alphanumeric() && is_han(right));
                if mixed {
                    violations.push(LangViolation::new(self.field, LangCategory::MissingSpace));
                    return;
                }
            }
        }
    }

    fn check_emoji(&self, violations: &mut Vec<LangViolation>) {
        let flagged = self.ranges.iter().any(|&(start, end)| {
            self.chars[start..end]
                .iter()
                .any(|&c| is_emoji(c) || matches!(c, '\u{FE0E}' | '\u{FE0F}'))
        });
        if flagged {
            violations.push(LangViolation::new(self.field, LangCategory::EmojiForbidden));
        }
    }
}

fn is_attached_symbol(chars: &[char], index: usize) -> bool {
    let c = chars[index];
    let prev = if index == 0 {
        None
    } else {
        chars.get(index - 1)
    };
    let next = chars.get(index + 1);
    if PERCENT_SIGNS.contains(&c) && prev.is_some_and(char::is_ascii_digit) {
        return true;
    }
    if CURRENCY_SIGNS.contains(&c) && next.is_some_and(char::is_ascii_digit) {
        return true;
    }
    let digits_around =
        prev.is_some_and(char::is_ascii_digit) && next.is_some_and(char::is_ascii_digit);
    digits_around && matches!(c, '.' | ',' | '-')
}

fn is_dash_or_ellipsis(chars: &[char], index: usize) -> bool {
    let c = chars[index];
    if DASH_FAMILY.contains(&c) {
        return true;
    }
    if c != '-' && c != '.' {
        return false;
    }
    let prev = if index == 0 {
        None
    } else {
        chars.get(index - 1)
    };
    let next = chars.get(index + 1);
    prev == Some(&c) || next == Some(&c)
}

fn is_han(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF)
}

fn is_emoji(c: char) -> bool {
    matches!(
        c as u32,
        0x1F000..=0x1F02B
            | 0x1F0A0..=0x1F0FF
            | 0x1F100..=0x1F1FF
            | 0x1F200..=0x1F2FF
            | 0x1F300..=0x1F5FF
            | 0x1F600..=0x1F64F
            | 0x1F680..=0x1F6FF
            | 0x1F900..=0x1F9FF
            | 0x1FA00..=0x1FAFF
    )
}
