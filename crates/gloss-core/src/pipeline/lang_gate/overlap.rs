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
    chars
        .windows(OVERLAP_MIN_CHARS)
        .map(|window| window.iter().collect())
        .collect()
}

fn word_grams(folded: &str) -> HashSet<String> {
    let words: Vec<&str> = words_of(folded).collect();
    words
        .windows(OVERLAP_MIN_WORDS)
        .map(|window| window.join(" "))
        .collect()
}

fn window_hits(stripped: &str, width: usize, hits: impl Fn(&str) -> bool) -> bool {
    stripped
        .chars()
        .collect::<Vec<_>>()
        .windows(width)
        .map(|window| window.iter().collect::<String>())
        .any(|gram| hits(&gram))
}

fn word_window_hits(folded: &str, word_grams: &HashSet<String>) -> bool {
    if word_grams.is_empty() {
        return false;
    }
    let words: Vec<&str> = words_of(folded).collect();
    words
        .windows(OVERLAP_MIN_WORDS)
        .any(|window| word_grams.contains(&window.join(" ")))
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
