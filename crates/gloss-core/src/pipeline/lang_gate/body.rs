//! 正文判定：容器内容与数学定界区之外的文本。文字系统（连续文字段按
//! 目标语言合法集判定，拉丁豁免仅单字母变量与紧跟句点的单字母缩写）、
//! 标点（目标标点集，容器定界符与数字附随符号豁免，破折号省略号家族
//! 不判）、盘古之白（仅中文，出罚面以文字系统的豁免面为界：相邻西文
//! 段是纯数字或不接句点的单字母才判，其余字母段归外文判罚）与表情符
//! 号。

use super::containers::script_runs;
use super::tables::{LangPolicy, ScriptClass, TaskPolicy};
use super::violation::{LangCategory, LangField, LangViolation};
use crate::langspec;

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
                if is_attached_symbol(self.lang, self.chars, index)
                    || is_dash_or_ellipsis(self.chars, index)
                {
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
            || (self.task.quote_container
                && (langspec::spec().curly_quotes.contains(&c) || c == '"'))
            || (self.task.markdown_allowed && c == '`')
    }

    fn check_pangu(&self, violations: &mut Vec<LangViolation>) {
        for &(start, end) in self.ranges {
            for index in start..end.saturating_sub(1) {
                let (left, right) = (self.chars[index], self.chars[index + 1]);
                let (han_at, west_at) = if is_han(left) && right.is_ascii_alphanumeric() {
                    (index, index + 1)
                } else if left.is_ascii_alphanumeric() && is_han(right) {
                    (index + 1, index)
                } else {
                    continue;
                };
                if self.pangu_exempt_west_segment(west_at, han_at, start, end) {
                    violations.push(LangViolation::new(self.field, LangCategory::MissingSpace));
                    return;
                }
            }
        }
    }

    fn pangu_exempt_west_segment(
        &self,
        west_at: usize,
        han_at: usize,
        start: usize,
        end: usize,
    ) -> bool {
        let (seg_start, seg_end) = if west_at > han_at {
            let mut seg_end = west_at + 1;
            while seg_end < end && self.chars[seg_end].is_ascii_alphanumeric() {
                seg_end += 1;
            }
            (west_at, seg_end)
        } else {
            let mut seg_start = west_at;
            while seg_start > start && self.chars[seg_start - 1].is_ascii_alphanumeric() {
                seg_start -= 1;
            }
            (seg_start, west_at + 1)
        };
        let mut index = seg_start;
        while index < seg_end {
            if !self.chars[index].is_ascii_alphabetic() {
                index += 1;
                continue;
            }
            let begin = index;
            while index < seg_end && self.chars[index].is_ascii_alphabetic() {
                index += 1;
            }
            if index - begin >= 2 || self.chars.get(index) == Some(&'.') {
                return false;
            }
        }
        true
    }

    fn check_emoji(&self, violations: &mut Vec<LangViolation>) {
        let spec = langspec::spec();
        let flagged = self.ranges.iter().any(|&(start, end)| {
            self.chars[start..end].iter().any(|&c| {
                is_emoji(c) || spec.emoji_bare.contains(&c) || spec.emoji_variants.contains(&c)
            })
        });
        if flagged {
            violations.push(LangViolation::new(self.field, LangCategory::EmojiForbidden));
        }
    }
}

fn is_attached_symbol(lang: &LangPolicy, chars: &[char], index: usize) -> bool {
    let spec = langspec::spec();
    let c = chars[index];
    let prev = if index == 0 {
        None
    } else {
        chars.get(index - 1)
    };
    let next = chars.get(index + 1);
    if lang.attached_after_digit.contains(&c) && prev.is_some_and(char::is_ascii_digit) {
        return true;
    }
    if lang.attached_before_digit.contains(&c) && next.is_some_and(char::is_ascii_digit) {
        return true;
    }
    let digits_around =
        prev.is_some_and(char::is_ascii_digit) && next.is_some_and(char::is_ascii_digit);
    digits_around && spec.attached_between_digits.contains(&c)
}

fn is_dash_or_ellipsis(chars: &[char], index: usize) -> bool {
    let spec = langspec::spec();
    let c = chars[index];
    if spec.dash_ellipsis.contains(&c) {
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
    langspec::range_contains(&langspec::spec().ranges.han, c as u32)
}

fn is_emoji(c: char) -> bool {
    langspec::range_contains(&langspec::spec().emoji_ranges, c as u32)
}
