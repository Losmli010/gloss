//! 语言探测与归一化：shebang、doctype 与语言签名形的内容探测，以及
//! 别名到规范名的收拢。
//!
//! 探测在视图创建时做一次，不进渲染热路径；结果同时供语言角标文字与
//! 高亮规则集选择消费。归一化把别名收拢到规范名（rs→rust、py→python…），
//! 角标与规则集都只认规范名。

/// 把语言名归一化为规范名：别名映射收拢，未知输入原样小写返回（角标
/// 如实显示），空白视为无语言。
pub(crate) fn normalize_language(name: &str) -> Option<String> {
    let lower = name.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return None;
    }
    Some(
        match lower.as_str() {
            "rs" => "rust",
            "py" => "python",
            "js" | "node" | "nodejs" | "mjs" | "cjs" | "jsx" => "javascript",
            "ts" | "tsx" => "typescript",
            "golang" => "go",
            "sh" | "shell" | "zsh" => "bash",
            "yml" => "yaml",
            "cpp" | "c++" | "cxx" | "cc" | "hpp" => "cpp",
            "cs" | "c#" => "csharp",
            "kt" | "kts" => "kotlin",
            "rb" => "ruby",
            "pl" | "pm" => "perl",
            "hs" => "haskell",
            "ex" | "exs" => "elixir",
            "jl" => "julia",
            "clj" | "cljs" | "cljc" | "edn" => "clojure",
            "objc" | "objective-c" | "objectivec" | "mm" => "objc",
            "htm" | "xhtml" => "html",
            _ => lower.as_str(),
        }
        .to_owned(),
    )
}

/// 内容探测：依序试各签名形，首个命中即返回规范名。顺序即优先级——
/// shebang 与 doctype 这类显式声明先于语言签名形（`import ` 这类弱
/// 签名放最后兜底）。
pub(crate) fn detect_language(text: &str) -> Option<String> {
    let head = text.trim_start();
    if let Some(lang) = detect_shebang(head) {
        return Some(lang);
    }
    if head.starts_with("<!DOCTYPE html") || head.starts_with("<!doctype html") {
        return Some("html".to_owned());
    }
    if head.starts_with("<?xml") {
        return Some("xml".to_owned());
    }
    if head.starts_with("<?php") {
        return Some("php".to_owned());
    }
    if looks_like_sql(head) {
        return Some("sql".to_owned());
    }
    if has_line_start(head, "package main") {
        return Some("go".to_owned());
    }
    if head.contains("fn main") {
        return Some("rust".to_owned());
    }
    if head.contains("func main") {
        return Some("go".to_owned());
    }
    if has_line_start(head, "def ")
        || has_line_start(head, "import ")
        || has_line_start(head, "from ")
    {
        return Some("python".to_owned());
    }
    None
}

/// shebang 行（`#!` 开头）按解释器名映射语言；认不出解释器则无语言。
/// 首个空白分隔 token 是解释器路径；`env` 转发时跳过旗标取其首个参数
/// （`#!/bin/sh -e`、`#!/usr/bin/env -S python3` 这类带旗标形态）。
fn detect_shebang(head: &str) -> Option<String> {
    let first = head.lines().next()?;
    let mut tokens = first.strip_prefix("#!")?.split_whitespace();
    fn basename(path: &str) -> &str {
        path.rsplit('/').next().unwrap_or(path)
    }
    let mut path = tokens.next()?;
    let mut name = basename(path);
    if name == "env" {
        loop {
            path = tokens.next()?;
            if path.starts_with('-') {
                continue;
            }
            name = basename(path);
            break;
        }
    }
    // `python3` / `python3.12` 这类带版本尾巴的解释器名截掉数字段。
    let base = name
        .split(['0', '1', '2', '3', '4', '5', '6', '7', '8', '9'])
        .next()
        .unwrap_or(name);
    match base {
        "python" => Some("python".to_owned()),
        "bash" => Some("bash".to_owned()),
        "sh" | "zsh" => Some("bash".to_owned()),
        "ruby" => Some("ruby".to_owned()),
        "perl" => Some("perl".to_owned()),
        "node" => Some("javascript".to_owned()),
        "lua" => Some("lua".to_owned()),
        "Rscript" => Some("r".to_owned()),
        _ => None,
    }
}

/// `needle` 是否出现在某一行的行首（签名形的最弱锚定，避免命中行中间
/// 的巧合子串）。
fn has_line_start(text: &str, needle: &str) -> bool {
    text.lines()
        .any(|line| line.trim_start().starts_with(needle))
}

/// SQL 形状：某行以 SELECT 开头、另一行以 FROM 开头（均按词，大小写
/// 不敏感）。行锚定 + 词边界——「selected / fromage」与注释里的子串
/// 都不算；单行散文（"select one from many"）天然不命中。
fn looks_like_sql(head: &str) -> bool {
    fn line_starts_with_keyword(line: &str, keyword: &str) -> bool {
        let line = line.trim_start();
        let mut line_chars = line.chars();
        for keyword_char in keyword.chars() {
            if !line_chars
                .next()
                .is_some_and(|ch| ch.eq_ignore_ascii_case(&keyword_char))
            {
                return false;
            }
        }
        match line_chars.next() {
            Some(ch) => !(ch.is_alphanumeric() || ch == '_'),
            None => true,
        }
    }
    let mut has_select = false;
    let mut has_from = false;
    for line in head.lines() {
        has_select |= line_starts_with_keyword(line, "select");
        has_from |= line_starts_with_keyword(line, "from");
    }
    has_select && has_from
}

#[cfg(test)]
mod tests {
    use super::{detect_language, normalize_language};

    #[test]
    fn normalize_language_collapses_aliases_to_canonical_names() {
        assert_eq!(
            normalize_language("rs").as_deref(),
            Some("rust"),
            "alias must fold to the canonical name"
        );
        assert_eq!(normalize_language("py").as_deref(), Some("python"));
        assert_eq!(normalize_language("TS").as_deref(), Some("typescript"));
        assert_eq!(normalize_language("golang").as_deref(), Some("go"));
        assert_eq!(normalize_language("c++").as_deref(), Some("cpp"));
        assert_eq!(normalize_language("kt").as_deref(), Some("kotlin"));
        assert_eq!(normalize_language("rb").as_deref(), Some("ruby"));
        assert_eq!(normalize_language("jl").as_deref(), Some("julia"));
        assert_eq!(normalize_language("hs").as_deref(), Some("haskell"));
        assert_eq!(normalize_language("clj").as_deref(), Some("clojure"));
        assert_eq!(normalize_language("mm").as_deref(), Some("objc"));
        assert_eq!(
            normalize_language("kotlin").as_deref(),
            Some("kotlin"),
            "canonical names pass through unchanged"
        );
        assert_eq!(
            normalize_language(" Fortran ").as_deref(),
            Some("fortran"),
            "unknown names keep their trimmed lowercase form"
        );
    }

    #[test]
    fn normalize_language_rejects_blank_names() {
        assert_eq!(normalize_language(""), None);
        assert_eq!(normalize_language("   "), None);
    }

    #[test]
    fn detect_language_reads_shebang_interpreters() {
        assert_eq!(
            detect_language("#!/usr/bin/env python3\nprint('hi')").as_deref(),
            Some("python")
        );
        assert_eq!(
            detect_language("#!/bin/bash\nset -euo pipefail").as_deref(),
            Some("bash")
        );
        assert_eq!(
            detect_language("#!/usr/bin/ruby\nputs 'hi'").as_deref(),
            Some("ruby")
        );
        assert_eq!(
            detect_language("#!/usr/bin/node\nconsole.log(1)").as_deref(),
            Some("javascript")
        );
        assert_eq!(
            detect_language("#!/bin/sh -e\nx").as_deref(),
            Some("bash"),
            "a flag after the interpreter path must not shadow the name"
        );
        assert_eq!(
            detect_language("#!/usr/bin/env -S python3 -a\nx").as_deref(),
            Some("python"),
            "env flags are skipped to reach the interpreter"
        );
        assert_eq!(
            detect_language("#!/usr/bin/unknown-thing\nx").as_deref(),
            None,
            "an unrecognized interpreter yields no language"
        );
    }

    #[test]
    fn detect_language_reads_markup_declarations() {
        assert_eq!(
            detect_language("<!DOCTYPE html>\n<html>").as_deref(),
            Some("html")
        );
        assert_eq!(
            detect_language("<?xml version=\"1.0\"?>").as_deref(),
            Some("xml")
        );
        assert_eq!(detect_language("<?php\necho 'hi';").as_deref(), Some("php"));
    }

    #[test]
    fn detect_language_reads_language_signatures() {
        assert_eq!(
            detect_language("fn main() {\n    println!(\"hi\");\n}").as_deref(),
            Some("rust")
        );
        assert_eq!(
            detect_language("package main\n\nfunc main() {}").as_deref(),
            Some("go"),
            "the go package clause outranks the func signature"
        );
        assert_eq!(detect_language("func main() {}").as_deref(), Some("go"));
        assert_eq!(
            detect_language("def greet(name):\n    return name").as_deref(),
            Some("python")
        );
        assert_eq!(
            detect_language("import os").as_deref(),
            Some("python"),
            "a bare import line folds to python as the weakest signature"
        );
    }

    #[test]
    fn detect_language_reads_sql_shapes() {
        assert_eq!(
            detect_language("SELECT id\nFROM users").as_deref(),
            Some("sql"),
            "a select-from pair across lines is an SQL signature"
        );
        assert_eq!(
            detect_language("select id\nfrom users").as_deref(),
            Some("sql"),
            "the shape is case-insensitive"
        );
        assert_eq!(
            detect_language("SELECT 1").as_deref(),
            None,
            "a lone select without from is not enough"
        );
        assert_eq!(
            detect_language("you select one from many options").as_deref(),
            None,
            "single-line prose that reads like the pair is left alone"
        );
    }

    #[test]
    fn detect_language_matches_signatures_only_at_line_starts() {
        assert_eq!(
            detect_language("the def in prose is not a signature").as_deref(),
            None,
            "mid-line matches must not count"
        );
    }

    #[test]
    fn detect_language_yields_none_for_plain_text() {
        assert_eq!(detect_language(""), None);
        assert_eq!(detect_language("just an ordinary sentence"), None);
        assert_eq!(detect_language("选中的一般文本也没有语言"), None);
    }

    #[test]
    fn sql_detection_ignores_non_statement_lines() {
        assert_eq!(
            detect_language("// select x from y\nfn main() {}").as_deref(),
            Some("rust"),
            "a select/from pair inside a comment is not an SQL statement"
        );
        assert_eq!(
            detect_language("we selected options fromage the menu\nand more").as_deref(),
            None,
            "substring hits (selected/fromage) are not the keyword pair"
        );
    }
}
