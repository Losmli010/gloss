//! 代码语言探测、归一化与语法着色（demo 同款架构：单趟正则分组捕获）。
//!
//! 规则集由共享词法积木组装——行/块注释、字符串（含转义与三引号）、
//! 数字（含十六进制/浮点）、大写驼峰类型、函数形——每语言只补关键字表；
//! JSON/YAML/TOML/SQL/HTML/XML/CSS 出特化规则（键/标签/选择器）。未知
//! 语言走通用启发集（字符串/两类注释/数字/类型/函数形仍着色），探测
//! 不出的语言角标不显示。正则按语言惰性编译一次（`OnceLock` 全表），
//! 逐帧的匹配是单趟线性扫描；流式重排由 egui 的 galley 缓存兜住，本
//! 模块不自建缓存。
//!
//! 语言有两级来源：平台 hint（[`gloss_core::task::InputHint::CodeLanguage`]）
//! 优先，缺失时对文本内容探测（shebang、doctype、语言签名形）。判定在
//! 视图创建时做一次，不进渲染热路径；结果同时供语言角标文字与高亮规则
//! 集选择消费。归一化把别名收拢到规范名（rs→rust、py→python…），角标
//! 与规则集都只认规范名。

use std::ops::Range;
use std::sync::OnceLock;

use egui::Color32;
use gloss_core::log::{thread, warn};
use regex::Regex;

/// 着色类别（demo 的 tok 六分类）：注释恒斜体，其余直立。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    /// 关键字（demo --k）。
    Keyword,
    /// 字符串字面量（demo --s）。
    String,
    /// 行/块注释（demo --c，恒斜体）。
    Comment,
    /// 数字（demo --n）。
    Number,
    /// 类型/键/标签（demo --t）。
    Type,
    /// 函数形/属性/选择器（demo --f）。
    Function,
}

impl Class {
    /// 类别着色（demo token 明暗两套）。
    pub(crate) fn color(self, dark: bool) -> Color32 {
        match (self, dark) {
            (Class::Keyword, true) => Color32::from_rgb(0xC7, 0x92, 0xEA),
            (Class::String, true) => Color32::from_rgb(0xA5, 0xD6, 0xA7),
            (Class::Comment, true) => Color32::from_rgb(0x6D, 0x76, 0x83),
            (Class::Number, true) => Color32::from_rgb(0xF0, 0xB4, 0x52),
            (Class::Type, true) => Color32::from_rgb(0x66, 0xC7, 0xD4),
            (Class::Function, true) => Color32::from_rgb(0x7C, 0xB8, 0xEC),
            (Class::Keyword, false) => Color32::from_rgb(0x8E, 0x44, 0xAD),
            (Class::String, false) => Color32::from_rgb(0x1E, 0x7E, 0x34),
            (Class::Comment, false) => Color32::from_rgb(0x9A, 0xA1, 0xAC),
            (Class::Number, false) => Color32::from_rgb(0xB4, 0x53, 0x09),
            (Class::Type, false) => Color32::from_rgb(0x0E, 0x7C, 0x8C),
            (Class::Function, false) => Color32::from_rgb(0x2B, 0x6C, 0xB0),
        }
    }

    /// 注释恒斜体（demo .tok.c 的 font-style: italic）。
    pub(crate) fn italic(self) -> bool {
        matches!(self, Class::Comment)
    }
}

/// 单趟着色：按语言的规则集扫一遍文本，交出各类别的字节区间（互不重
/// 叠、按出现顺序）。`lang` 经归一化；未知语言与 `None` 都落通用启发
/// 集。分组名到类别的映射见 [`class_of`]。
pub(crate) fn tokenize(text: &str, lang: Option<&str>) -> Vec<(Range<usize>, Class)> {
    let Some(re) = regex_for(lang) else {
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

/// 定界符跟随匹配的类别（函数形的 `(`、键的 `:`/`=`）把定界符一并吃进
/// 匹配（rust regex 无 lookaround），着色时裁掉、只留语法本体；其余
/// 类别原样。
fn trimmed_range(matched: regex::Match<'_>, class: Class) -> Range<usize> {
    let mut range = matched.range();
    if matches!(class, Class::Function | Class::Type) {
        let trimmed = matched
            .as_str()
            .trim_end_matches(['(', '=', ':'])
            .trim_end();
        range.end = range.start + trimmed.len();
    }
    range
}

/// 分组名 → 类别（b 块注释与 c 行注释同为 Comment）。
fn class_of(group: &str) -> Option<Class> {
    match group {
        "b" | "c" => Some(Class::Comment),
        "s" => Some(Class::String),
        "k" => Some(Class::Keyword),
        "n" => Some(Class::Number),
        "t" => Some(Class::Type),
        "f" => Some(Class::Function),
        _ => None,
    }
}

/// 语言词法配置：注释/字符串形态 + 关键字表（其余积木共用）。
struct LangSpec {
    name: &'static str,
    line_comments: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    double_quoted: bool,
    single_quoted: bool,
    backtick: bool,
    triple_quoted: bool,
    sql_quoted: bool,
    case_insensitive_keywords: bool,
    keywords: &'static [&'static str],
}

const fn lex(
    name: &'static str,
    line_comments: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    keywords: &'static [&'static str],
) -> LangSpec {
    LangSpec {
        name,
        line_comments,
        block_comment,
        double_quoted: true,
        single_quoted: true,
        backtick: false,
        triple_quoted: false,
        sql_quoted: false,
        case_insensitive_keywords: false,
        keywords,
    }
}

const RUST_KW: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use",
    "where", "while",
];
const C_KW: &[&str] = &[
    "auto", "break", "case", "char", "const", "continue", "default", "do", "double", "else",
    "enum", "extern", "float", "for", "goto", "if", "int", "long", "register", "return", "short",
    "signed", "sizeof", "static", "struct", "switch", "typedef", "union", "unsigned", "void",
    "volatile", "while",
];
const CPP_KW: &[&str] = &[
    "alignas",
    "auto",
    "bool",
    "break",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "constexpr",
    "continue",
    "default",
    "delete",
    "do",
    "double",
    "else",
    "enum",
    "explicit",
    "export",
    "extern",
    "false",
    "float",
    "for",
    "friend",
    "goto",
    "if",
    "inline",
    "int",
    "long",
    "mutable",
    "namespace",
    "new",
    "noexcept",
    "nullptr",
    "operator",
    "override",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "signed",
    "sizeof",
    "static",
    "struct",
    "switch",
    "template",
    "this",
    "throw",
    "true",
    "try",
    "typedef",
    "typename",
    "union",
    "unsigned",
    "using",
    "virtual",
    "void",
    "volatile",
    "while",
];
const JAVA_KW: &[&str] = &[
    "abstract",
    "assert",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "final",
    "finally",
    "float",
    "for",
    "goto",
    "if",
    "implements",
    "import",
    "instanceof",
    "int",
    "interface",
    "long",
    "native",
    "new",
    "package",
    "private",
    "protected",
    "public",
    "record",
    "return",
    "short",
    "static",
    "strictfp",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "try",
    "var",
    "void",
    "volatile",
    "while",
    "false",
    "true",
    "null",
];
const CSHARP_KW: &[&str] = &[
    "abstract",
    "as",
    "base",
    "bool",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "checked",
    "class",
    "const",
    "continue",
    "decimal",
    "default",
    "delegate",
    "do",
    "double",
    "else",
    "enum",
    "event",
    "explicit",
    "extern",
    "false",
    "finally",
    "fixed",
    "float",
    "for",
    "foreach",
    "get",
    "goto",
    "if",
    "implicit",
    "in",
    "init",
    "int",
    "interface",
    "internal",
    "is",
    "lock",
    "long",
    "namespace",
    "new",
    "null",
    "object",
    "operator",
    "out",
    "override",
    "params",
    "private",
    "protected",
    "public",
    "readonly",
    "record",
    "ref",
    "return",
    "sbyte",
    "sealed",
    "set",
    "short",
    "sizeof",
    "stackalloc",
    "static",
    "string",
    "struct",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "uint",
    "ulong",
    "unchecked",
    "unsafe",
    "ushort",
    "using",
    "var",
    "virtual",
    "void",
    "volatile",
    "while",
    "with",
];
const GO_KW: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "false",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "nil",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "true",
    "type",
    "var",
];
const JAVASCRIPT_KW: &[&str] = &[
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "null",
    "of",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "undefined",
    "var",
    "void",
    "while",
    "with",
    "yield",
];
const TYPESCRIPT_KW: &[&str] = &[
    "abstract",
    "any",
    "as",
    "async",
    "await",
    "boolean",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "declare",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "from",
    "function",
    "get",
    "if",
    "implements",
    "import",
    "in",
    "infer",
    "instanceof",
    "interface",
    "keyof",
    "let",
    "namespace",
    "never",
    "new",
    "null",
    "number",
    "of",
    "private",
    "protected",
    "public",
    "readonly",
    "return",
    "set",
    "static",
    "string",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "type",
    "typeof",
    "undefined",
    "unknown",
    "var",
    "void",
    "while",
    "yield",
];
const KOTLIN_KW: &[&str] = &[
    "abstract",
    "actual",
    "annotation",
    "as",
    "break",
    "by",
    "catch",
    "class",
    "companion",
    "const",
    "constructor",
    "continue",
    "crossinline",
    "data",
    "do",
    "dynamic",
    "else",
    "enum",
    "expect",
    "external",
    "false",
    "final",
    "finally",
    "for",
    "fun",
    "get",
    "if",
    "import",
    "in",
    "infix",
    "init",
    "interface",
    "internal",
    "is",
    "lateinit",
    "null",
    "object",
    "open",
    "operator",
    "out",
    "override",
    "package",
    "private",
    "protected",
    "public",
    "reified",
    "return",
    "sealed",
    "set",
    "super",
    "suspend",
    "this",
    "throw",
    "true",
    "try",
    "typealias",
    "val",
    "var",
    "vararg",
    "when",
    "where",
    "while",
];
const SWIFT_KW: &[&str] = &[
    "actor",
    "any",
    "as",
    "associatedtype",
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "continue",
    "default",
    "defer",
    "deinit",
    "do",
    "else",
    "enum",
    "extension",
    "false",
    "fileprivate",
    "final",
    "for",
    "func",
    "get",
    "guard",
    "if",
    "import",
    "init",
    "inout",
    "internal",
    "is",
    "lazy",
    "let",
    "nil",
    "open",
    "operator",
    "private",
    "protocol",
    "public",
    "repeat",
    "rethrows",
    "return",
    "self",
    "set",
    "static",
    "struct",
    "subscript",
    "super",
    "switch",
    "throw",
    "throws",
    "true",
    "try",
    "typealias",
    "var",
    "where",
    "while",
    "willSet",
];
const DART_KW: &[&str] = &[
    "abstract",
    "as",
    "assert",
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "covariant",
    "default",
    "deferred",
    "do",
    "dynamic",
    "else",
    "enum",
    "export",
    "extends",
    "extension",
    "external",
    "factory",
    "false",
    "final",
    "finally",
    "for",
    "get",
    "hide",
    "if",
    "implements",
    "import",
    "in",
    "interface",
    "is",
    "late",
    "library",
    "mixin",
    "new",
    "null",
    "on",
    "operator",
    "part",
    "required",
    "rethrow",
    "return",
    "sealed",
    "set",
    "show",
    "static",
    "super",
    "switch",
    "sync",
    "this",
    "throw",
    "true",
    "try",
    "typedef",
    "var",
    "void",
    "while",
    "with",
    "yield",
];
const PHP_KW: &[&str] = &[
    "abstract",
    "and",
    "array",
    "as",
    "break",
    "callable",
    "case",
    "catch",
    "class",
    "clone",
    "const",
    "continue",
    "declare",
    "default",
    "do",
    "echo",
    "else",
    "elseif",
    "enum",
    "extends",
    "false",
    "final",
    "finally",
    "fn",
    "for",
    "foreach",
    "function",
    "global",
    "if",
    "implements",
    "include",
    "include_once",
    "instanceof",
    "interface",
    "isset",
    "list",
    "match",
    "namespace",
    "new",
    "null",
    "or",
    "print",
    "private",
    "protected",
    "public",
    "readonly",
    "require",
    "require_once",
    "return",
    "static",
    "switch",
    "throw",
    "trait",
    "true",
    "try",
    "unset",
    "use",
    "var",
    "while",
    "xor",
    "yield",
];
const SCALA_KW: &[&str] = &[
    "abstract",
    "case",
    "catch",
    "class",
    "def",
    "do",
    "else",
    "end",
    "enum",
    "extends",
    "false",
    "final",
    "finally",
    "for",
    "forSome",
    "given",
    "if",
    "implicit",
    "import",
    "lazy",
    "match",
    "new",
    "null",
    "object",
    "override",
    "package",
    "private",
    "protected",
    "return",
    "sealed",
    "super",
    "this",
    "throw",
    "trait",
    "true",
    "try",
    "type",
    "using",
    "val",
    "var",
    "while",
    "with",
    "yield",
];
const PERL_KW: &[&str] = &[
    "and",
    "break",
    "caller",
    "case",
    "chomp",
    "cmp",
    "continue",
    "die",
    "do",
    "each",
    "else",
    "elsif",
    "eq",
    "exists",
    "for",
    "foreach",
    "ge",
    "given",
    "goto",
    "grep",
    "gt",
    "if",
    "keys",
    "last",
    "lc",
    "le",
    "local",
    "lt",
    "map",
    "my",
    "ne",
    "next",
    "no",
    "not",
    "or",
    "our",
    "package",
    "pop",
    "print",
    "printf",
    "push",
    "redo",
    "ref",
    "require",
    "return",
    "scalar",
    "shift",
    "sort",
    "splice",
    "split",
    "sub",
    "substr",
    "unless",
    "unshift",
    "until",
    "use",
    "values",
    "wantarray",
    "when",
    "while",
    "xor",
];
const RUBY_KW: &[&str] = &[
    "alias",
    "attr_accessor",
    "attr_reader",
    "attr_writer",
    "begin",
    "break",
    "case",
    "class",
    "def",
    "defined?",
    "do",
    "else",
    "elsif",
    "end",
    "ensure",
    "false",
    "for",
    "if",
    "in",
    "module",
    "next",
    "nil",
    "not",
    "or",
    "raise",
    "redo",
    "require",
    "require_relative",
    "rescue",
    "retry",
    "return",
    "self",
    "super",
    "then",
    "true",
    "undef",
    "unless",
    "until",
    "when",
    "while",
    "yield",
];
const PYTHON_KW: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "case", "class", "continue", "def", "del",
    "elif", "else", "except", "False", "finally", "for", "from", "global", "if", "import", "in",
    "is", "lambda", "match", "None", "nonlocal", "not", "or", "pass", "raise", "return", "True",
    "while", "with", "yield",
];
const BASH_KW: &[&str] = &[
    "alias", "break", "case", "continue", "declare", "do", "done", "echo", "elif", "else", "esac",
    "exit", "export", "fi", "for", "function", "if", "in", "local", "readonly", "return", "select",
    "set", "shift", "source", "then", "time", "trap", "true", "false", "type", "unset", "until",
    "while",
];
const LUA_KW: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in",
    "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
];
const HASKELL_KW: &[&str] = &[
    "case", "class", "data", "default", "deriving", "do", "else", "foreign", "if", "import", "in",
    "infix", "infixl", "infixr", "instance", "let", "module", "newtype", "of", "then", "type",
    "where",
];
const ELIXIR_KW: &[&str] = &[
    "alias",
    "case",
    "cond",
    "def",
    "defdelegate",
    "defexception",
    "defguard",
    "defimpl",
    "defmodule",
    "defmacro",
    "defp",
    "defprotocol",
    "defstruct",
    "do",
    "else",
    "end",
    "false",
    "fn",
    "for",
    "if",
    "import",
    "nil",
    "raise",
    "receive",
    "require",
    "true",
    "try",
    "unless",
    "use",
    "with",
];
const R_KW: &[&str] = &[
    "break", "else", "FALSE", "for", "function", "if", "in", "Inf", "NA", "NaN", "next", "NULL",
    "repeat", "return", "TRUE", "while",
];
const JULIA_KW: &[&str] = &[
    "begin", "break", "catch", "const", "continue", "do", "else", "elseif", "end", "export",
    "false", "finally", "for", "function", "global", "if", "import", "in", "isa", "let", "local",
    "macro", "module", "mutable", "new", "quote", "return", "struct", "switch", "true", "try",
    "type", "using", "while",
];
const OBJC_KW: &[&str] = &[
    "@interface",
    "@implementation",
    "@property",
    "@end",
    "@class",
    "@protocol",
    "@implementation",
    "@try",
    "@catch",
    "@finally",
    "@throw",
    "@synchronized",
    "BOOL",
    "Class",
    "id",
    "IMP",
    "nil",
    "self",
    "super",
    "_cmd",
    "YES",
    "NO",
    "dispatch_async",
    "dispatch_get_main_queue",
];
const CLOJURE_KW: &[&str] = &[
    "def",
    "defn",
    "defmacro",
    "defmulti",
    "defmethod",
    "defprotocol",
    "defrecord",
    "deftype",
    "fn",
    "let",
    "letfn",
    "loop",
    "recur",
    "if",
    "if-let",
    "when",
    "when-let",
    "do",
    "cond",
    "case",
    "ns",
    "require",
    "import",
    "use",
    "quote",
    "var",
    "set!",
    "true",
    "false",
    "nil",
    "map",
    "filter",
    "reduce",
    "apply",
    "assoc",
    "conj",
];
const SQL_KW: &[&str] = &[
    "add",
    "all",
    "alter",
    "and",
    "as",
    "asc",
    "between",
    "by",
    "case",
    "create",
    "cross",
    "delete",
    "desc",
    "distinct",
    "drop",
    "else",
    "end",
    "exists",
    "foreign",
    "from",
    "full",
    "group",
    "having",
    "in",
    "index",
    "inner",
    "insert",
    "into",
    "is",
    "join",
    "key",
    "left",
    "like",
    "limit",
    "not",
    "null",
    "offset",
    "on",
    "or",
    "order",
    "outer",
    "primary",
    "references",
    "right",
    "select",
    "set",
    "table",
    "then",
    "union",
    "unique",
    "update",
    "values",
    "view",
    "when",
    "where",
];
const JSON_KW: &[&str] = &["true", "false", "null"];
const DATA_BOOL_KW: &[&str] = &["true", "false", "null", "yes", "no", "on", "off"];

/// 全语言表：规范名 → 词法配置，[`pattern_for`] 组装；json/yaml/toml/
/// html/xml/css 由独立特化模式在 [`regex_for`] 组表，未知语言落
/// [`generic_pattern`]。javascript 条目带反引号模板串（末条 override）。
const LANGS: &[LangSpec] = &[
    lex("rust", &["//"], Some(("/*", "*/")), RUST_KW),
    lex("c", &["//"], Some(("/*", "*/")), C_KW),
    lex("cpp", &["//"], Some(("/*", "*/")), CPP_KW),
    lex("objc", &["//"], Some(("/*", "*/")), OBJC_KW),
    lex("java", &["//"], Some(("/*", "*/")), JAVA_KW),
    lex("csharp", &["//"], Some(("/*", "*/")), CSHARP_KW),
    lex("go", &["//"], Some(("/*", "*/")), GO_KW),
    lex("typescript", &["//"], Some(("/*", "*/")), TYPESCRIPT_KW),
    lex("kotlin", &["//"], Some(("/*", "*/")), KOTLIN_KW),
    lex("swift", &["//"], Some(("/*", "*/")), SWIFT_KW),
    lex("dart", &["//"], Some(("/*", "*/")), DART_KW),
    lex("scala", &["//"], Some(("/*", "*/")), SCALA_KW),
    lex("php", &["//", "#"], Some(("/*", "*/")), PHP_KW),
    lex("perl", &["#"], None, PERL_KW),
    lex("ruby", &["#"], None, RUBY_KW),
    lex("bash", &["#"], None, BASH_KW),
    lex("elixir", &["#"], None, ELIXIR_KW),
    lex("r", &["#"], None, R_KW),
    lex("julia", &["#"], None, JULIA_KW),
    lex("clojure", &[";"], None, CLOJURE_KW),
    lex("haskell", &["--"], Some(("{-", "-}")), HASKELL_KW),
    lex("lua", &["--"], Some(("--[[", "]]")), LUA_KW),
    LangSpec {
        sql_quoted: true,
        single_quoted: false,
        case_insensitive_keywords: true,
        ..lex("sql", &["--"], Some(("/*", "*/")), SQL_KW)
    },
    LangSpec {
        triple_quoted: true,
        ..lex("python", &["#"], None, PYTHON_KW)
    },
    LangSpec {
        backtick: true,
        ..lex("javascript", &["//"], Some(("/*", "*/")), JAVASCRIPT_KW)
    },
];

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
            None => false,
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

/// 规则集正则：按语言惰性编译一次（首帧全表编译，之后查表零成本）。
/// 未知语言（表外规范名与 `None`）共用 `generic` 一套。
/// 已编译规则集：generic 兜底一套 + 规范名查表（generic 编译失败时为
/// `None`，整体无着色）。
type RegexTable = (Option<Regex>, Vec<(&'static str, Regex)>);

fn regex_for(lang: Option<&str>) -> Option<&'static Regex> {
    static REGEXES: OnceLock<RegexTable> = OnceLock::new();
    let (generic, table) = REGEXES.get_or_init(|| {
        let mut table: Vec<(&'static str, Regex)> = LANGS
            .iter()
            .filter_map(|spec| compile(&pattern_for(spec)).map(|re| (spec.name, re)))
            .collect();
        for (name, pattern) in [
            ("yaml", data_pattern(false)),
            ("toml", data_pattern(true)),
            ("json", json_pattern()),
            ("html", markup_pattern()),
            ("xml", markup_pattern()),
            ("css", css_pattern()),
        ] {
            if let Some(re) = compile(&pattern) {
                table.push((name, re));
            }
        }
        (compile(&generic_pattern()), table)
    });
    let name = lang.and_then(normalize_language);
    table
        .iter()
        .find(|(key, _)| Some(*key) == name.as_deref())
        .map(|(_, re)| re)
        .or(generic.as_ref())
}

/// 编译静态组装出的模式。失败只可能是模式表笔误：该语言整体失去着色
/// （隔离降级），debug 构建里第一时间显形，release 留一条告警痕。
fn compile(pattern: &str) -> Option<Regex> {
    match Regex::new(pattern) {
        Ok(re) => Some(re),
        Err(error) => {
            debug_assert!(false, "static highlight pattern failed: {error}");
            warn!(
                thread = thread::UI,
                error = %error,
                "highlight pattern failed to compile, language renders uncolored"
            );
            None
        }
    }
}

/// 一个语言规则集的完整模式：块注释 → 行注释 → 字符串 → 关键字 →
/// 数字 → 类型 → 函数形（同位起配时靠次序定优先级）。
fn pattern_for(spec: &LangSpec) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some((open, close)) = spec.block_comment {
        parts.push(block_comment(open, close));
    }
    if !spec.line_comments.is_empty() {
        parts.push(line_comment(spec.line_comments));
    }
    let quotes = string_alternatives(spec);
    parts.push(format!(r"(?P<s>{quotes})"));
    parts.push(keyword_group(spec.keywords, spec.case_insensitive_keywords));
    parts.push(numbers());
    parts.push(r"(?P<t>\b[A-Z][A-Za-z0-9_]*\b)".to_owned());
    parts.push(r"(?P<f>\b[a-z_][A-Za-z0-9_]*\s*\()".to_owned());
    parts.join("|")
}

/// 通用启发集（未知语言的兜底）：字符串、两类注释、数字、类型与函数形。
fn generic_pattern() -> String {
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

/// JSON 特化：键（冒号前的字符串）→ Type，字符串 → String，布尔/空 →
/// Keyword，数字 → Number；无注释。
fn json_pattern() -> String {
    [
        format!(r#"(?P<t>{dq}\s*:)"#, dq = double_quoted()),
        format!(r#"(?P<s>{dq})"#, dq = double_quoted()),
        keyword_group(JSON_KW, false),
        numbers(),
    ]
    .join("|")
}

/// YAML / TOML 特化（`section_headers` 为 TOML 开节头）：行首键 →
/// Type，字符串 → String，布尔 → Keyword，数字 → Number。无块注释。
fn data_pattern(section_headers: bool) -> String {
    let mut parts = vec![
        line_comment(&["#"]),
        r#"(?P<t>(?m:^[ \t]*(?:- )?[\w.\-/]+ *:))"#.to_owned(),
        format!(
            r#"(?P<s>{dq}|{sq})"#,
            dq = double_quoted(),
            sq = single_quoted()
        ),
    ];
    if section_headers {
        parts.push(r"(?P<f>(?m:^\[[^\]\n]*\]))".to_owned());
    }
    parts.push(keyword_group(DATA_BOOL_KW, false));
    parts.push(numbers());
    parts.join("|")
}

/// HTML/XML 特化：注释 → Comment，标签名与尖括号/doctype/PI → Keyword，
/// 属性名（等号前）→ Function，字符串 → String。
fn markup_pattern() -> String {
    [
        block_comment("<!--", "-->"),
        r"(?P<k></?[A-Za-z][\w:.-]*|/?>|<![Dd][Oo][Cc][Tt][Yy][Pp][Ee][^>]*>|<\?[\w.-]*)"
            .to_owned(),
        format!(
            r#"(?P<s>{dq}|{sq})"#,
            dq = double_quoted(),
            sq = single_quoted()
        ),
        r"(?P<f>[A-Za-z_][\w:.-]*\s*=)".to_owned(),
    ]
    .join("|")
}

/// CSS 特化：注释 → Comment，@规则 → Keyword，属性名（冒号前）→ Type，
/// 类/ID 选择器 → Function，数字与色值 → Number，字符串 → String。
fn css_pattern() -> String {
    [
        block_comment("/*", "*/"),
        format!(
            r#"(?P<s>{dq}|{sq})"#,
            dq = double_quoted(),
            sq = single_quoted()
        ),
        r"(?P<k>@[A-Za-z-]+)".to_owned(),
        r"(?P<t>[A-Za-z-]+\s*:)".to_owned(),
        r"(?P<n>#[0-9a-fA-F]{3,8}\b|\b\d[\d.]*(?:px|em|rem|vh|vw|%|s|ms|fr)?\b)".to_owned(),
        r"(?P<f>[.#][A-Za-z][\w-]*)".to_owned(),
    ]
    .join("|")
}

/// 词法积木：块注释（非贪婪跨行）。
fn block_comment(open: &str, close: &str) -> String {
    format!(
        r"(?P<b>{}[\s\S]*?{})",
        regex::escape(open),
        regex::escape(close)
    )
}

/// 词法积木：行注释（前缀集任一，吃到行尾）。
fn line_comment(prefixes: &[&str]) -> String {
    let prefixes = prefixes
        .iter()
        .map(|prefix| regex::escape(prefix))
        .collect::<Vec<_>>()
        .join("|");
    format!(r"(?P<c>(?:{prefixes})[^\n]*)")
}

/// 词法积木：双引号字符串（转义豁免，不跨行）。
fn double_quoted() -> String {
    r#""(?:[^"\\\n]|\\.)*""#.to_owned()
}

/// 词法积木：单引号字符串（转义豁免，不跨行）。
fn single_quoted() -> String {
    r#"'(?:[^'\\\n]|\\.)*'"#.to_owned()
}

/// 词法积木：反引号字符串（转义豁免，不跨行）。
fn backtick_quoted() -> String {
    r"`(?:[^`\\\n]|\\.)*`".to_owned()
}

/// 词法积木：三引号字符串（可跨行，Python）。
fn triple_quoted() -> String {
    r#""{3}(?:\\.|[^\\])*?"{3}|'{3}(?:\\.|[^\\])*?'{3}"#.to_owned()
}

/// 词法积木：数字（十进制含下划线分隔/浮点/指数，十六进制在前防 `0` 截胡）。
fn numbers() -> String {
    r"(?P<n>\b(?:0[xX][0-9a-fA-F_]+|\d[\d_]*(?:\.[\d_]+)?(?:[eE][+-]?\d+)?)\b)".to_owned()
}

/// 语言配置下的字符串备选串（按三引号 → 反引号 → 双引号 → 单引号序）。
fn string_alternatives(spec: &LangSpec) -> String {
    let mut alternatives: Vec<String> = Vec::new();
    if spec.triple_quoted {
        alternatives.push(triple_quoted());
    }
    if spec.backtick {
        alternatives.push(backtick_quoted());
    }
    if spec.double_quoted {
        alternatives.push(double_quoted());
    }
    if spec.sql_quoted {
        alternatives.push(r"'(?:[^']|'')*'".to_owned());
    } else if spec.single_quoted {
        alternatives.push(single_quoted());
    }
    alternatives.join("|")
}

/// 词法积木：关键字表（可选大小写不敏感，如 SQL）。首尾都是词字符的
/// 关键字加 `\b` 边界；含非词边缘的关键字（objc 的 `@interface`、ruby
/// 的 `defined?`、clojure 的 `set!`）裸匹配——`\b` 在 `@` 前、`?`/`!`
/// 后不存在，加了反而永不命中。
fn keyword_group(keywords: &[&str], case_insensitive: bool) -> String {
    let (mut worded, mut bare) = (Vec::new(), Vec::new());
    for keyword in keywords {
        let word_char = |ch: Option<char>| ch.is_some_and(|ch| ch.is_alphanumeric() || ch == '_');
        if word_char(keyword.chars().next()) && word_char(keyword.chars().last()) {
            worded.push(regex::escape(keyword));
        } else {
            bare.push(regex::escape(keyword));
        }
    }
    let flag = if case_insensitive { "i" } else { "" };
    let mut alternatives = Vec::new();
    if !bare.is_empty() {
        alternatives.push(format!(r"(?{flag}:{})", bare.join("|")));
    }
    if !worded.is_empty() {
        alternatives.push(format!(r"(?{flag}:\b(?:{})\b)", worded.join("|")));
    }
    format!(r"(?P<k>{})", alternatives.join("|"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classes_of(tokens: &[(Range<usize>, Class)]) -> Vec<Class> {
        tokens.iter().map(|(_, class)| *class).collect()
    }

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
    fn known_languages_color_keywords_and_shapes() {
        let tokens = tokenize("fn main() {}", Some("rs"));
        let text = "fn main() {}";
        assert_eq!(
            classes_of(&tokens),
            vec![Class::Keyword, Class::Function],
            "rust `fn` is a keyword and `main(` a function shape: {tokens:?}"
        );
        let start = text.find("fn").expect("fn present");
        assert_eq!(tokens[0].0, start..start + 2);

        let tokens = tokenize("def greet(name):\n    return name", Some("py"));
        assert_eq!(
            classes_of(&tokens)
                .iter()
                .filter(|class| **class == Class::Keyword)
                .count(),
            2,
            "python def/return are keywords: {tokens:?}"
        );
    }

    #[test]
    fn unknown_languages_still_color_strings_and_comments() {
        let tokens = tokenize("s = \"hi\"  -- note", Some("fortran"));
        assert_eq!(
            classes_of(&tokens),
            vec![Class::String, Class::Comment],
            "the generic heuristic set colors strings and line comments: {tokens:?}"
        );
        assert_eq!(
            classes_of(&tokenize("", Some("fortran"))),
            Vec::<Class>::new(),
            "empty input yields no tokens"
        );
    }

    #[test]
    fn missing_language_falls_to_the_generic_set() {
        let tokens = tokenize("if (x) { return 'v'; } // end", None);
        assert!(
            classes_of(&tokens).contains(&Class::String),
            "no hint still colors strings: {tokens:?}"
        );
        assert!(
            classes_of(&tokens).contains(&Class::Comment),
            "no hint still colors comments: {tokens:?}"
        );
    }

    #[test]
    fn json_keys_specialize_ahead_of_strings() {
        let source = "{\"key\": 1}";
        let specialized = tokenize(source, Some("json"));
        let start = source.find("\"key\"").expect("key present");
        assert_eq!(
            specialized[0],
            (start..start + 5, Class::Type),
            "a colon-following string is a JSON key: {specialized:?}"
        );

        let generic = tokenize(source, None);
        assert_eq!(
            generic[0].1,
            Class::String,
            "the same text under the generic set is a plain string: {generic:?}"
        );
    }

    #[test]
    fn markup_tags_and_attributes_specialize() {
        let tokens = tokenize("<div class=\"a\">x</div>", Some("html"));
        let classes = classes_of(&tokens);
        assert!(
            classes.contains(&Class::Keyword),
            "tag names are keywords: {tokens:?}"
        );
        assert!(
            classes.contains(&Class::Function),
            "attribute names are functions: {tokens:?}"
        );
    }

    #[test]
    fn sql_keywords_are_case_insensitive() {
        let tokens = tokenize("select id from users where x = 1", Some("sql"));
        let keywords = tokens
            .iter()
            .filter(|(_, class)| *class == Class::Keyword)
            .count();
        assert_eq!(
            keywords, 3,
            "select/from/where color in any case: {tokens:?}"
        );
    }

    #[test]
    fn block_comments_span_lines_and_triples_span_lines() {
        let tokens = tokenize("/* a\nb */ fn", Some("rust"));
        assert_eq!(tokens[0].1, Class::Comment);
        assert_eq!(
            tokens[0].0.end, 9,
            "a block comment token spans to the closing star-slash"
        );

        let tokens = tokenize("s = '''\nab\n'''", Some("py"));
        assert_eq!(
            classes_of(&tokens),
            vec![Class::String],
            "a triple-quoted string is one token across lines: {tokens:?}"
        );
    }

    #[test]
    fn hex_numbers_color_whole() {
        let source = "let a = 0xFF_00;";
        let tokens = tokenize(source, Some("rust"));
        let start = source.find("0xFF_00").expect("hex present");
        assert!(
            tokens
                .iter()
                .any(|(range, class)| *class == Class::Number && range.start == start),
            "hex literal is one number token: {tokens:?}"
        );
    }

    #[test]
    fn js_template_strings_color_with_the_backtick_ruleset() {
        let tokens = tokenize("const s = `hi ${x}`;", Some("js"));
        assert_eq!(
            classes_of(&tokens),
            vec![Class::Keyword, Class::String],
            "js alias reaches the backtick-enabled ruleset: {tokens:?}"
        );
    }

    #[test]
    fn keywords_with_non_word_edges_still_color() {
        assert!(
            classes_of(&tokenize("@interface Foo : NSObject", Some("objc")))
                .contains(&Class::Keyword),
            "objc @-keywords have no leading word edge and must still match"
        );
        assert!(
            classes_of(&tokenize("x = defined? y", Some("ruby"))).contains(&Class::Keyword),
            "ruby defined? has no trailing word edge"
        );
        assert!(
            classes_of(&tokenize("(set! x 1)", Some("clojure"))).contains(&Class::Keyword),
            "clojure set! has no trailing word edge"
        );
    }

    #[test]
    fn toml_section_headers_color_on_every_line() {
        let source = "[dependencies]\nserde = 1\n\n[dev-dependencies]";
        let tokens = tokenize(source, Some("toml"));
        let headers = tokens
            .iter()
            .filter(|(_, class)| *class == Class::Function)
            .count();
        assert_eq!(
            headers, 2,
            "a section header colors on any line, not just the first: {tokens:?}"
        );
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

    #[test]
    fn css_hex_colors_color_as_numbers_not_selectors() {
        let tokens = tokenize("a { color: #fff; } #wrap { top: 0; }", Some("css"));
        let hexes = tokens
            .iter()
            .filter(|(range, class)| {
                *class == Class::Number
                    && range.start
                        == "a { color: #fff; } #wrap"
                            .find("#fff")
                            .expect("hex present")
            })
            .count();
        assert_eq!(hexes, 1, "a short hex color is a number: {tokens:?}");
    }

    #[test]
    fn every_class_has_its_own_color_per_theme() {
        let classes = [
            Class::Keyword,
            Class::String,
            Class::Comment,
            Class::Number,
            Class::Type,
            Class::Function,
        ];
        for dark in [true, false] {
            let colors: Vec<_> = classes.iter().map(|class| class.color(dark)).collect();
            for (index, left) in colors.iter().enumerate() {
                for right in colors.iter().skip(index + 1) {
                    assert_ne!(left, right, "classes must stay visually distinct");
                }
            }
        }
        assert!(Class::Comment.italic());
        assert!(!Class::Keyword.italic());
    }
}
