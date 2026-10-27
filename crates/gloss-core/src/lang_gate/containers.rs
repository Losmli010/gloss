//! 容器切分与容器内规则：括号组（按目标括号对，可嵌套）、引文组（弯引
//! 号按字形配对，直引号按出现次数两两配对，不嵌套）的单趟栈式扫描，及
//! 其上的闭合判定、组内单一语言判定与句译短语级判定。

use super::tables::{LangPolicy, ScriptClass, script_class};
use super::violation::{LangCategory, LangField, LangViolation};
use crate::task::TaskKind;

pub(crate) struct Container {
    pub(crate) open: usize,
    pub(crate) close: usize,
    pub(crate) parent_open: Option<usize>,
    pub(crate) is_quote: bool,
}

pub(crate) struct Scan {
    pub(crate) containers: Vec<Container>,
    pub(crate) unclosed: bool,
}

const CURLY_QUOTES: [char; 2] = ['“', '”']; // i18n:allow 判定常量
const SENTENCE_FINALS: [char; 6] = ['。', '！', '？', '.', '!', '?']; // i18n:allow 判定常量

enum Open {
    Paren(usize),
    Quote,
    Straight,
}

fn paren_pair_of(c: char, pairs: &[(char, char)]) -> Option<usize> {
    pairs.iter().position(|&(open, _)| open == c)
}

fn paren_close_of(c: char, pairs: &[(char, char)]) -> Option<usize> {
    pairs.iter().position(|&(_, close)| close == c)
}

pub(crate) fn scan_containers(
    chars: &[char],
    masked: &[bool],
    policy: &LangPolicy,
    quotes_enabled: bool,
) -> Scan {
    let mut containers = Vec::new();
    let mut stack: Vec<(Open, usize)> = Vec::new();
    let mut unclosed = false;
    for (index, &c) in chars.iter().enumerate() {
        if masked[index] {
            continue;
        }
        if let Some(pair) = paren_pair_of(c, policy.paren_pairs) {
            stack.push((Open::Paren(pair), index));
        } else if let Some(pair) = paren_close_of(c, policy.paren_pairs) {
            let closed = close_container(
                &mut stack,
                &mut containers,
                index,
                |open| matches!(open, Open::Paren(p) if *p == pair),
            );
            unclosed |= !closed;
        } else if CURLY_QUOTES[0] == c && quotes_enabled {
            stack.push((Open::Quote, index));
        } else if CURLY_QUOTES[1] == c && quotes_enabled {
            let closed = close_container(&mut stack, &mut containers, index, |open| {
                matches!(open, Open::Quote)
            });
            unclosed |= !closed;
        } else if c == '"' && quotes_enabled {
            let closed = close_container(&mut stack, &mut containers, index, |open| {
                matches!(open, Open::Straight)
            });
            if !closed {
                stack.push((Open::Straight, index));
            }
        }
    }
    unclosed |= !stack.is_empty();
    Scan {
        containers,
        unclosed,
    }
}

fn close_container(
    stack: &mut Vec<(Open, usize)>,
    containers: &mut Vec<Container>,
    close: usize,
    matches: impl Fn(&Open) -> bool,
) -> bool {
    let Some((open, open_at)) = stack.pop_if(|(open, _)| matches(open)) else {
        return false;
    };
    let parent_open = stack.last().map(|&(_, index)| index);
    containers.push(Container {
        open: open_at,
        close: close + 1,
        parent_open,
        is_quote: !matches!(open, Open::Paren(_)),
    });
    true
}

pub(crate) fn direct_ranges(containers: &[Container], index: usize) -> Vec<(usize, usize)> {
    let container = &containers[index];
    let mut children: Vec<(usize, usize)> = containers
        .iter()
        .filter(|other| other.parent_open == Some(container.open))
        .map(|other| (other.open, other.close))
        .collect();
    children.sort_unstable();
    let mut ranges = Vec::new();
    let mut cursor = container.open + 1;
    for (start, end) in children {
        if start > cursor {
            ranges.push((cursor, start));
        }
        cursor = cursor.max(end);
    }
    if container.close - 1 > cursor {
        ranges.push((cursor, container.close - 1));
    }
    ranges
}

pub(crate) struct ContainerCheck<'a> {
    pub kind: TaskKind,
    pub policy: &'a LangPolicy,
    pub field: LangField,
    pub chars: &'a [char],
}

impl Scan {
    pub(crate) fn check(&self, ctx: &ContainerCheck<'_>, violations: &mut Vec<LangViolation>) {
        for index in 0..self.containers.len() {
            let is_quote = self.containers[index].is_quote;
            let direct = direct_ranges(&self.containers, index);
            check_mixing(ctx.policy, ctx.field, ctx.chars, &direct, violations);
            if !is_quote && ctx.kind == TaskKind::TranslateSentence {
                check_sentence_parens(ctx.field, ctx.chars, &direct, violations);
            }
        }
    }
}

fn check_mixing(
    policy: &LangPolicy,
    field: LangField,
    chars: &[char],
    ranges: &[(usize, usize)],
    violations: &mut Vec<LangViolation>,
) {
    if policy.scripts.is_empty() {
        return;
    }
    let mut has_target = false;
    let mut has_foreign = false;
    for run in script_runs(chars, ranges) {
        if run.len <= 1 {
            continue;
        }
        if policy.scripts.contains(&run.class) {
            has_target = true;
        } else {
            has_foreign = true;
        }
    }
    if has_target && has_foreign {
        violations.push(LangViolation::new(field, LangCategory::ContainerMixing));
    }
}

fn check_sentence_parens(
    field: LangField,
    chars: &[char],
    ranges: &[(usize, usize)],
    violations: &mut Vec<LangViolation>,
) {
    let last = ranges
        .iter()
        .rev()
        .flat_map(|&(start, end)| chars[start..end].iter().rev())
        .find(|c| !c.is_whitespace())
        .copied();
    let sentence_final = last.is_some_and(|c| SENTENCE_FINALS.contains(&c)); // i18n:allow 判定常量
    if sentence_final {
        violations.push(LangViolation::new(field, LangCategory::SentenceInParens));
    }
}

pub(crate) struct ScriptRun {
    pub(crate) class: ScriptClass,
    pub(crate) len: usize,
    pub(crate) end: usize,
}

pub(crate) fn script_runs(chars: &[char], ranges: &[(usize, usize)]) -> Vec<ScriptRun> {
    let mut runs = Vec::new();
    for &(start, end) in ranges {
        let mut index = start;
        while index < end {
            if chars[index].is_alphabetic() {
                let class = script_class(chars[index]);
                let begin = index;
                while index < end
                    && chars[index].is_alphabetic()
                    && script_class(chars[index]) == class
                {
                    index += 1;
                }
                runs.push(ScriptRun {
                    class,
                    len: index - begin,
                    end: index,
                });
            } else {
                index += 1;
            }
        }
    }
    runs
}
