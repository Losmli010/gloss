//! 原文重合判定：容器外片段与任务输入原样重合，连续长度达到
//! 阈值（12 字符或 6 个西文词，任一达标即比中）判违约。比对把双方空白
//! 折叠为单空格（词项口径）并另做去空白比对（字符口径）——盘古之白补
//! 的空格不得使重合检测失效。图片任务无文本输入，整体跳过。

use std::collections::HashSet;

use super::violation::{LangCategory, LangField, LangViolation};

const OVERLAP_MIN_CHARS: usize = 12;
const OVERLAP_MIN_WORDS: usize = 6;

pub(crate) fn check_overlap(
    input: &str,
    bodies: &[(LangField, String)],
    violations: &mut Vec<LangViolation>,
) {
    let input_stripped: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    let input_folded = fold(input);
    let char_grams = char_grams(&input_stripped);
    let word_grams = word_grams(&input_folded);
    if char_grams.is_empty() && word_grams.is_empty() {
        return;
    }
    for (field, body) in bodies {
        let stripped: String = body.chars().filter(|c| !c.is_whitespace()).collect();
        if window_hits(&stripped, OVERLAP_MIN_CHARS, |gram| {
            char_grams.contains(gram)
        }) {
            violations.push(LangViolation::new(*field, LangCategory::SourceOverlap));
            continue;
        }
        let folded = fold(body);
        if word_window_hits(&folded, &word_grams) {
            violations.push(LangViolation::new(*field, LangCategory::SourceOverlap));
        }
    }
}

fn fold(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn char_grams(stripped: &str) -> HashSet<String> {
    let chars: Vec<char> = stripped.chars().collect();
    if chars.len() < OVERLAP_MIN_CHARS {
        return HashSet::new();
    }
    (0..=chars.len() - OVERLAP_MIN_CHARS)
        .map(|start| chars[start..start + OVERLAP_MIN_CHARS].iter().collect())
        .collect()
}

fn word_grams(folded: &str) -> HashSet<String> {
    let words: Vec<&str> = words_of(folded).collect();
    if words.len() < OVERLAP_MIN_WORDS {
        return HashSet::new();
    }
    (0..=words.len() - OVERLAP_MIN_WORDS)
        .map(|start| words[start..start + OVERLAP_MIN_WORDS].join(" "))
        .collect()
}

fn window_hits(stripped: &str, width: usize, hits: impl Fn(&str) -> bool) -> bool {
    let chars: Vec<char> = stripped.chars().collect();
    if chars.len() < width {
        return false;
    }
    (0..=chars.len() - width).any(|start| {
        let gram: String = chars[start..start + width].iter().collect();
        hits(&gram)
    })
}

fn word_window_hits(folded: &str, word_grams: &HashSet<String>) -> bool {
    if word_grams.is_empty() {
        return false;
    }
    let words: Vec<&str> = words_of(folded).collect();
    if words.len() < OVERLAP_MIN_WORDS {
        return false;
    }
    (0..=words.len() - OVERLAP_MIN_WORDS)
        .any(|start| word_grams.contains(&words[start..start + OVERLAP_MIN_WORDS].join(" ")))
}

fn words_of(folded: &str) -> impl Iterator<Item = &str> {
    folded
        .split(|c: char| !c.is_alphanumeric() || is_cjk_like(c))
        .filter(|word| !word.is_empty())
}

fn is_cjk_like(c: char) -> bool {
    matches!(
        c as u32,
        0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF
    )
}
