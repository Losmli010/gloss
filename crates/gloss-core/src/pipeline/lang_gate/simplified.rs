//! 简体判定：目标为中文时，容器外汉字与括号组内汉字逐一比对内嵌简繁
//! 字表（裁剪自开放中文转换开源项目，见同目录字表数据模块），繁体独有
//! 字形判违约；引文容器与数学定界区豁免（古籍书证原文繁体是体例正确）。

use std::collections::HashMap;
use std::sync::OnceLock;

use super::violation::{LangCategory, LangField, LangViolation};

pub(crate) fn check_simplified(
    field: LangField,
    chars: &[char],
    ranges: &[(usize, usize)],
    violations: &mut Vec<LangViolation>,
) {
    let table = table();
    let flagged = ranges
        .iter()
        .any(|&(start, end)| chars[start..end].iter().any(|&c| table.contains_key(&c)));
    if flagged {
        violations.push(LangViolation::new(field, LangCategory::TraditionalChar));
    }
}

fn table() -> &'static HashMap<char, char> {
    static TABLE: OnceLock<HashMap<char, char>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let chars: Vec<char> = super::char_map::pairs().chars().collect();
        chars
            .chunks_exact(2)
            .map(|pair| (pair[0], pair[1]))
            .collect()
    })
}
