//! 通用启发式着色：syntect 没有可用语法名时的兜底（流式期语言未定、
//! 表外语言）。只认跨语言稳定的字符串、两类注释、数字、类型与函数形，
//! 不做关键字表——关键字是各语言语法集的职责，兜底不做猜测。
//!
//! 正则按静态模式编译一次；逐帧仍要单趟线性扫描（O(n)，与旧实现一致），
//! egui 的 galley 缓存只省重排、不跳过扫描。

use std::ops::Range;
use std::sync::OnceLock;

use egui::FontId;
use egui::text::{ByteIndex, LayoutJob, LayoutSection, TextFormat, TextWrapping};
use gloss_core::log::{thread, warn};
use regex::Regex;

use super::palette::{Class, plain_color};

/// 通用启发集的着色区间（互不重叠、按出现顺序）。
pub(super) fn tokens(text: &str) -> Vec<(Range<usize>, Class)> {
    let Some(re) = compiled() else {
        return Vec::new();
    };
    let mut tokens = Vec::new();
    for captures in re.captures_iter(text) {
        let Some(matched) = captures.get(0) else {
            continue;
        };
        let class = re
            .capture_names()
            .flatten()
            .find(|name| captures.name(name).is_some())
            .and_then(class_of);
        if let Some(class) = class {
            tokens.push((trimmed_range(matched, class), class));
        }
    }
    tokens
}

/// 兜底正文的排版：字符串/注释/数字/类型/函数形按六类色板着色，其余
/// 平文；折行与行高与 syntect 路径一致。
pub(crate) fn fallback_job(
    source: &str,
    font_id: &FontId,
    dark: bool,
    wrap_width: f32,
    line_height: f32,
) -> LayoutJob {
    let plain = TextFormat {
        line_height: Some(line_height),
        ..TextFormat::simple(font_id.clone(), plain_color(dark))
    };
    let mut job = LayoutJob {
        text: source.to_owned(),
        wrap: TextWrapping {
            max_width: wrap_width,
            break_anywhere: true,
            ..TextWrapping::no_max_width()
        },
        break_on_newline: true,
        ..LayoutJob::default()
    };
    let mut cursor = 0;
    for (range, class) in tokens(source) {
        if range.start > cursor {
            job.sections.push(LayoutSection {
                leading_space: 0.0,
                byte_range: ByteIndex(cursor)..ByteIndex(range.start),
                format: plain.clone(),
            });
        }
        job.sections.push(LayoutSection {
            leading_space: 0.0,
            byte_range: ByteIndex(range.start)..ByteIndex(range.end),
            format: TextFormat {
                color: class.color(dark),
                italics: class.italic(),
                ..plain.clone()
            },
        });
        cursor = range.end;
    }
    if cursor < source.len() {
        job.sections.push(LayoutSection {
            leading_space: 0.0,
            byte_range: ByteIndex(cursor)..ByteIndex(source.len()),
            format: plain,
        });
    }
    job
}

/// 全类别共享的通用规则：块注释 → 行注释 → 字符串 → 数字 → 类型 →
/// 函数形（同位起配时靠次序定优先级）。`--` 与 `%` 分别是 Lua/SQL 系与
/// LaTeX/MATLAB 系的行注释前缀；兜底只服务表外语言，取覆盖更广的一边，
/// 代价是自减与取模会被整行染成注释。
fn pattern() -> String {
    [
        block_comment("/*", "*/"),
        line_comment(&["//", "#", "--", "%"]),
        format!(
            r#"(?P<s>{dq}|{sq}|{bt})"#,
            dq = double_quoted(),
            sq = single_quoted(),
            bt = backtick_quoted()
        ),
        numbers(),
        r"(?P<t>\b[A-Z][A-Za-z0-9_]*\b)".to_owned(),
        r"(?P<f>\b[a-z_][A-Za-z0-9_]*\s*\()".to_owned(),
    ]
    .join("|")
}

/// 规则集正则：进程内编译一次，之后查表零成本。
fn compiled() -> Option<&'static Regex> {
    static REGEX: OnceLock<Option<Regex>> = OnceLock::new();
    REGEX.get_or_init(|| compile(&pattern())).as_ref()
}

/// 编译静态模式。失败只可能是模式笔误：整体失去着色（隔离降级），debug
/// 构建里第一时间显形，release 留一条告警痕。
fn compile(pattern: &str) -> Option<Regex> {
    match Regex::new(pattern) {
        Ok(re) => Some(re),
        Err(error) => {
            debug_assert!(false, "static highlight pattern failed: {error}");
            warn!(
                thread = thread::UI,
                error = %error,
                "highlight pattern failed to compile, fallback renders uncolored"
            );
            None
        }
    }
}

/// 函数形的定界符（regex 无 lookaround）随匹配一起吃掉，着色时裁掉、只留
/// 语法本体；其余类别原样。
fn trimmed_range(matched: regex::Match<'_>, class: Class) -> Range<usize> {
    let mut range = matched.range();
    if matches!(class, Class::Function) {
        let trimmed = matched
            .as_str()
            .trim_end_matches(['(', '=', ':'])
            .trim_end();
        range.end = range.start + trimmed.len();
    }
    range
}

fn class_of(group: &str) -> Option<Class> {
    match group {
        "b" | "c" => Some(Class::Comment),
        "s" => Some(Class::String),
        "n" => Some(Class::Number),
        "t" => Some(Class::Type),
        "f" => Some(Class::Function),
        _ => None,
    }
}

fn block_comment(open: &str, close: &str) -> String {
    format!(
        r"(?P<b>{}[\s\S]*?{})",
        regex::escape(open),
        regex::escape(close)
    )
}

fn line_comment(prefixes: &[&str]) -> String {
    let prefixes = prefixes
        .iter()
        .map(|prefix| regex::escape(prefix))
        .collect::<Vec<_>>()
        .join("|");
    format!(r"(?P<c>(?:{prefixes})[^\n]*)")
}

fn double_quoted() -> String {
    r#""(?:[^"\\\n]|\\.)*""#.to_owned()
}

fn single_quoted() -> String {
    r#"'(?:[^'\\\n]|\\.)*'"#.to_owned()
}

fn backtick_quoted() -> String {
    r"`(?:[^`\\\n]|\\.)*`".to_owned()
}

fn numbers() -> String {
    r"(?P<n>\b(?:0[xX][0-9a-fA-F_]+|\d[\d_]*(?:\.[\d_]+)?(?:[eE][+-]?\d+)?)\b)".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classes_of(tokens: &[(Range<usize>, Class)]) -> Vec<Class> {
        tokens.iter().map(|(_, class)| *class).collect()
    }

    #[test]
    fn generic_tokens_color_strings_and_line_comments() {
        let found = tokens("s = \"hi\"  -- note");
        assert_eq!(
            classes_of(&found),
            vec![Class::String, Class::Comment],
            "the generic set colors strings and line comments: {found:?}"
        );
        assert_eq!(classes_of(&tokens("")), Vec::<Class>::new());
    }

    #[test]
    fn generic_tokens_color_block_comments_across_lines() {
        let tokens = tokens("/* a\nb */ fn");
        assert_eq!(tokens[0].1, Class::Comment);
        assert_eq!(
            tokens[0].0.end, 9,
            "a block comment token spans to the closing star-slash"
        );
    }

    #[test]
    fn generic_tokens_keep_hex_literals_whole() {
        let source = "let a = 0xFF_00;";
        let tokens = tokens(source);
        let start = source.find("0xFF_00").expect("hex present");
        assert!(
            tokens
                .iter()
                .any(|(range, class)| *class == Class::Number && range.start == start),
            "hex literal is one number token: {tokens:?}"
        );
    }

    #[test]
    fn generic_tokens_color_type_names_and_leave_lowercase_alone() {
        let source = "let value: Widget = build();";
        let found = tokens(source);
        assert!(
            found
                .iter()
                .any(|(range, class)| *class == Class::Type && &source[range.clone()] == "Widget"),
            "an upper-camel name is one Type token: {found:?}"
        );
        assert!(
            found
                .iter()
                .filter(|(_, class)| *class == Class::Type)
                .all(|(range, _)| source[range.clone()].starts_with(|c: char| c.is_uppercase())),
            "only upper-camel names are types: {found:?}"
        );
    }

    #[test]
    fn generic_tokens_color_function_shapes_without_the_delimiter() {
        let tokens = tokens("compute(value)");
        assert_eq!(tokens[0].1, Class::Function);
        assert_eq!(
            tokens[0].0,
            0.."compute(value)".find('(').expect("paren present"),
            "the function token stops before the opening paren"
        );
    }
}
