//! 代码语言采纳与语法着色适配：语言名归一化、语法库（syntect）语法映射与
//! 六类色板主题。
//!
//! 语言的唯一来源是任务产物的疏证代码语言，由大语言模型（LLM）判定、经采纳入口
//! 归一；归一后的名字映射到编辑器（Sublime）语法名，着色输出交扩展库（egui_extras）
//! 渲染。着色类别与色板在色板模块。
//!
//! 扩展库的代码主题只暴露内置主题、字段私有，自定义六类色板
//! 只能顶替其中按名字索引的两个主题条目（见本模块的主题装配）。

mod fallback;
mod palette;

use std::str::FromStr;
use std::sync::OnceLock;

use egui::FontId;
use egui::text::LayoutJob;
use egui_extras::syntax_highlighting::{CodeTheme, SyntectSettings, highlight_with};
use gloss_core::log::{debug, thread};
use syntect::highlighting::{
    Color, FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSet, ThemeSettings,
};

use crate::ui::style::{DARK, LIGHT, SYNTAX_THEME_DARK, SYNTAX_THEME_LIGHT, SyntaxPalette};

pub(crate) use fallback::fallback_job;

const LANGUAGE_ALIASES: &[(&str, &str)] = &[
    ("rs", "rust"),
    ("py", "python"),
    ("js", "javascript"),
    ("node", "javascript"),
    ("nodejs", "javascript"),
    ("mjs", "javascript"),
    ("cjs", "javascript"),
    ("jsx", "javascript"),
    ("ts", "typescript"),
    ("tsx", "typescript"),
    ("golang", "go"),
    ("sh", "bash"),
    ("shell", "bash"),
    ("zsh", "bash"),
    ("yml", "yaml"),
    ("c++", "cpp"),
    ("cxx", "cpp"),
    ("cc", "cpp"),
    ("hpp", "cpp"),
    ("cs", "csharp"),
    ("c#", "csharp"),
    ("kt", "kotlin"),
    ("kts", "kotlin"),
    ("rb", "ruby"),
    ("pl", "perl"),
    ("pm", "perl"),
    ("hs", "haskell"),
    ("ex", "elixir"),
    ("exs", "elixir"),
    ("jl", "julia"),
    ("clj", "clojure"),
    ("cljs", "clojure"),
    ("cljc", "clojure"),
    ("edn", "clojure"),
    ("objective-c", "objc"),
    ("objectivec", "objc"),
    ("mm", "objc"),
    ("htm", "html"),
    ("xhtml", "html"),
];

pub(crate) fn normalize_language(name: &str) -> Option<String> {
    let lower = name.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return None;
    }
    let canonical = LANGUAGE_ALIASES
        .iter()
        .find_map(|(alias, canonical)| (*alias == lower).then_some(*canonical))
        .unwrap_or(lower.as_str());
    Some(canonical.to_owned())
}

pub(crate) fn adopt(verdict: Option<&str>) -> Option<String> {
    verdict
        .map(str::trim)
        .filter(|verdict| {
            !verdict.is_empty()
                && verdict
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '#' | '-' | '.'))
        })
        .and_then(normalize_language)
}

const SYNTAX_NAMES: &[(&str, &str)] = &[
    ("rust", "Rust"),
    ("c", "C"),
    ("cpp", "C++"),
    ("objc", "Objective-C"),
    ("java", "Java"),
    ("csharp", "C#"),
    ("go", "Go"),
    ("typescript", "TypeScript"),
    ("kotlin", "Kotlin"),
    ("swift", "Swift"),
    ("dart", "Dart"),
    ("scala", "Scala"),
    ("php", "PHP"),
    ("perl", "Perl"),
    ("ruby", "Ruby"),
    ("bash", "Bash"),
    ("elixir", "Elixir"),
    ("r", "R"),
    ("julia", "Julia"),
    ("clojure", "Clojure"),
    ("haskell", "Haskell"),
    ("lua", "Lua"),
    ("sql", "SQL"),
    ("python", "Python"),
    ("javascript", "JavaScript"),
    ("yaml", "YAML"),
    ("toml", "TOML"),
    ("json", "JSON"),
    ("html", "HTML"),
    ("xml", "XML"),
    ("css", "CSS"),
];

pub(crate) fn syntect_token(lang: Option<&str>) -> Option<&'static str> {
    let name = normalize_language(lang?)?;
    SYNTAX_NAMES
        .iter()
        .find(|(canonical, _)| *canonical == name.as_str())
        .map(|(_, token)| *token)
}

pub(crate) fn highlight(
    ui: &egui::Ui,
    font_id: FontId,
    source: &str,
    lang: Option<&str>,
) -> Option<LayoutJob> {
    let colors = crate::ui::style::palette(ui);
    let token = syntect_token(lang)?;
    let theme = if colors.is_dark {
        CodeTheme::dark(font_id.size)
    } else {
        CodeTheme::light(font_id.size)
    }
    .with_font_id(font_id);
    let mut job = highlight_with(ui.ctx(), ui.style(), &theme, source, token, settings());
    for section in &mut job.sections {
        section.format.underline = egui::Stroke::NONE;
    }
    Some(job)
}

fn settings() -> &'static SyntectSettings {
    static SETTINGS: OnceLock<SyntectSettings> = OnceLock::new();
    SETTINGS.get_or_init(|| SyntectSettings {
        ps: two_face::syntax::extra_newlines(),
        ts: theme_set(),
    })
}

pub(crate) fn warm_up() {
    if std::thread::Builder::new()
        .name("gloss-highlight-warmup".to_owned())
        .spawn(|| {
            let _ = settings();
        })
        .is_err()
    {
        debug!(
            thread = thread::UI,
            "highlight warmup thread not started, first code card pays the cost"
        );
    }
}

fn theme_set() -> ThemeSet {
    let mut themes = ThemeSet::new();
    themes
        .themes
        .insert(SYNTAX_THEME_DARK.to_owned(), gloss_theme(&DARK.syntax));
    themes
        .themes
        .insert(SYNTAX_THEME_LIGHT.to_owned(), gloss_theme(&LIGHT.syntax));
    themes
}

const SCOPE_CLASSES: &[(&str, palette::Class)] = &[
    ("comment", palette::Class::Comment),
    ("string", palette::Class::String),
    ("constant.numeric", palette::Class::Number),
    ("constant.other.color", palette::Class::Number),
    ("constant.language", palette::Class::Keyword),
    ("keyword.control", palette::Class::Keyword),
    ("keyword.operator.logical.python", palette::Class::Keyword),
    ("keyword.operator.logical.ruby", palette::Class::Keyword),
    ("keyword.operator.logical.lua", palette::Class::Keyword),
    ("keyword.operator.word", palette::Class::Keyword),
    ("keyword.other", palette::Class::Keyword),
    ("keyword.declaration", palette::Class::Keyword),
    ("storage", palette::Class::Keyword),
    ("storage.type", palette::Class::Keyword),
    ("storage.modifier", palette::Class::Keyword),
    ("variable.language.rust", palette::Class::Keyword),
    ("variable.language.ruby", palette::Class::Keyword),
    ("variable.language.this", palette::Class::Keyword),
    ("variable.language.super", palette::Class::Keyword),
    ("entity.name.tag", palette::Class::Keyword),
    ("entity.name.tag.yaml", palette::Class::Type),
    ("entity.name.tag.toml", palette::Class::Type),
    ("entity.name.type", palette::Class::Type),
    ("entity.name.class", palette::Class::Type),
    ("support.type", palette::Class::Type),
    ("support.type.property-name", palette::Class::Type),
    (
        "meta.mapping.key.json string.quoted.double.json",
        palette::Class::Type,
    ),
    ("entity.name.function", palette::Class::Function),
    ("support.function", palette::Class::Function),
    ("variable.function", palette::Class::Function),
    ("entity.other.attribute-name", palette::Class::Function),
    ("entity.name.section", palette::Class::Function),
    ("entity.name.table.toml", palette::Class::Function),
];

fn gloss_theme(syntax: &SyntaxPalette) -> Theme {
    Theme {
        name: Some("gloss".to_owned()),
        author: None,
        settings: ThemeSettings {
            foreground: Some(syntect_color(syntax.plain)),
            ..ThemeSettings::default()
        },
        scopes: SCOPE_CLASSES
            .iter()
            .map(|&(scope, class)| theme_item(scope, class, syntax))
            .collect(),
    }
}

fn theme_item(scope: &str, class: palette::Class, syntax: &SyntaxPalette) -> ThemeItem {
    let scope = ScopeSelectors::from_str(scope).unwrap_or_else(|error| {
        debug_assert!(false, "static theme scope failed: {error:?}");
        ScopeSelectors::default()
    });
    ThemeItem {
        scope,
        style: StyleModifier {
            foreground: Some(syntect_color(class.pick(syntax))),
            font_style: class.italic().then_some(FontStyle::ITALIC),
            ..StyleModifier::default()
        },
    }
}

fn syntect_color(color: egui::Color32) -> Color {
    Color {
        r: color.r(),
        g: color.g(),
        b: color.b(),
        a: color.a(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adopt_keeps_plausible_verdicts_and_normalizes_aliases() {
        assert_eq!(adopt(Some("rust")), Some("rust".to_owned()));
        assert_eq!(adopt(Some("Rust")), Some("rust".to_owned()));
        assert_eq!(adopt(Some("py")), Some("python".to_owned()));
        assert_eq!(adopt(Some("objective-c")), Some("objc".to_owned()));
        assert_eq!(adopt(Some("c++")), Some("cpp".to_owned()));
    }

    #[test]
    fn adopt_rejects_prose_placeholders_and_missing_verdicts() {
        assert_eq!(adopt(None), None, "no verdict, no language");
        assert_eq!(adopt(Some("")), None);
        assert_eq!(
            adopt(Some("   ")),
            None,
            "a blank verdict is no verdict after trim"
        );
        assert_eq!(
            adopt(Some("…或 null")),
            None,
            "the placeholder string the model may echo is not a language"
        );
        assert_eq!(
            adopt(Some("可能是 rust")),
            None,
            "prose never becomes a badge"
        );
    }

    #[test]
    fn adopt_trims_surrounding_whitespace_before_the_gate() {
        assert_eq!(
            adopt(Some(" python\n")),
            Some("python".to_owned()),
            "padded verdicts are trimmed, not rejected"
        );
    }

    #[test]
    fn normalize_language_collapses_aliases_to_canonical_names() {
        assert_eq!(normalize_language("rs").as_deref(), Some("rust"));
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
    fn canonical_languages_resolve_in_the_default_syntax_set() {
        let syntaxes = two_face::syntax::extra_newlines();
        let missing: Vec<_> = SYNTAX_NAMES
            .iter()
            .filter(|(_, token)| {
                syntaxes
                    .find_syntax_by_name(token)
                    .or_else(|| syntaxes.find_syntax_by_extension(token))
                    .is_none()
            })
            .map(|(canonical, token)| format!("{canonical} ({token})"))
            .collect();
        assert!(missing.is_empty(), "missing syntaxes: {missing:?}");
    }

    #[test]
    fn unmapped_languages_have_no_syntect_token() {
        assert_eq!(syntect_token(None), None);
        assert_eq!(syntect_token(Some("fortran")), None);
        assert_eq!(syntect_token(Some("  ")), None);
        assert_eq!(syntect_token(Some("rs")), Some("Rust"));
    }

    #[test]
    fn aliases_always_resolve_through_the_syntax_whitelist() {
        for (alias, canonical) in LANGUAGE_ALIASES {
            let token = syntect_token(Some(alias));
            assert!(token.is_some(), "{alias} -> {canonical} must be colorable");
            assert_eq!(
                token,
                syntect_token(Some(canonical)),
                "{alias} and its canonical name must share a syntax"
            );
        }
    }

    fn sections<'a>(
        job: &'a LayoutJob,
        source: &'a str,
    ) -> Vec<(std::ops::Range<usize>, &'a str, &'a egui::TextFormat)> {
        job.sections
            .iter()
            .map(|section| {
                let range = section.byte_range.start.0..section.byte_range.end.0;
                (range.clone(), &source[range], &section.format)
            })
            .collect()
    }

    fn colors_of(job: &LayoutJob, source: &str, token: &str) -> Vec<egui::Color32> {
        let start = source.find(token).expect("token is in the fixture");
        let end = start + token.len();
        sections(job, source)
            .into_iter()
            .filter(|(range, _, _)| range.start >= start && range.end <= end)
            .map(|(_, _, format)| format.color)
            .collect()
    }

    fn assert_token_color(job: &LayoutJob, source: &str, token: &str, expected: egui::Color32) {
        let colors = colors_of(job, source, token);
        assert!(!colors.is_empty(), "{token:?} has no section");
        assert!(
            colors.iter().all(|color| *color == expected),
            "{token:?} must be {expected:?}, got {colors:?}"
        );
    }

    fn syntax_of(dark: bool) -> &'static SyntaxPalette {
        if dark { &DARK.syntax } else { &LIGHT.syntax }
    }

    fn highlight_in_theme(dark: bool, source: &str, lang: Option<&str>) -> Option<LayoutJob> {
        let theme = if dark {
            gloss_core::config::Theme::Dark
        } else {
            gloss_core::config::Theme::Light
        };
        let ctx = crate::ui::context::new_context(theme);
        let mut job = None;
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            job = highlight(ui, egui::FontId::monospace(12.0), source, lang);
        });
        output.drop_without_applying_deltas();
        job
    }

    #[test]
    fn scope_mapping_keeps_keyword_string_comment_number() {
        let source = "fn main() {\n    // note\n    let s = \"hi\";\n    let n = 42;\n}";
        for dark in [false, true] {
            let syntax = syntax_of(dark);
            let job = highlight_in_theme(dark, source, Some("rust")).expect("rust is mapped");
            for (token, class) in [
                ("fn", palette::Class::Keyword),
                ("\"hi\"", palette::Class::String),
                ("42", palette::Class::Number),
            ] {
                assert_token_color(&job, source, token, class.pick(syntax));
            }
            let comment_start = source.find("// note").expect("the fixture has a comment");
            let comment = sections(&job, source)
                .into_iter()
                .find(|(range, _, _)| range.start == comment_start)
                .map(|(_, _, format)| format)
                .expect("the comment has a section");
            assert_eq!(
                comment.color,
                palette::Class::Comment.pick(syntax),
                "the comment keeps its palette color"
            );
            assert!(comment.italics, "comments stay italic");
            assert_eq!(
                comment.underline,
                egui::Stroke::NONE,
                "the upstream italic-as-underline bug is cleared"
            );
        }
    }

    const SCOPED_FIXTURES: &[(&str, &str, &str, palette::Class)] = &[
        ("key: value\n", "yaml", "key", palette::Class::Type),
        (
            "[gloss]\nkey = 1\n",
            "toml",
            "gloss",
            palette::Class::Function,
        ),
        ("[gloss]\nkey = 1\n", "toml", "key", palette::Class::Type),
        (
            "a { color: #fff; }\n",
            "css",
            "#fff",
            palette::Class::Number,
        ),
        (
            "{\"key\": \"value\"}\n",
            "json",
            "\"key\"",
            palette::Class::Type,
        ),
        (
            "{\"key\": \"value\"}\n",
            "json",
            "\"value\"",
            palette::Class::String,
        ),
        ("x = a and b\n", "python", "and", palette::Class::Keyword),
        ("x = a and b\n", "ruby", "and", palette::Class::Keyword),
        ("local x = a and b\n", "lua", "and", palette::Class::Keyword),
        (
            "if x\n  self.y\nend\n",
            "ruby",
            "self",
            palette::Class::Keyword,
        ),
        (
            "fn f() { self.value }\n",
            "rust",
            "self",
            palette::Class::Keyword,
        ),
        (
            "const f = function () { return this.x; };\n",
            "javascript",
            "this",
            palette::Class::Keyword,
        ),
        (
            "class A { m() { return super.m(); } }\n",
            "javascript",
            "super",
            palette::Class::Keyword,
        ),
    ];

    const UNSCOPED_FIXTURES: &[(&str, &str, &str)] = &[
        ("x = self.value\n", "python", "self"),
        ("echo $@\n", "bash", "$@"),
        ("fn f(&self) {}\n", "rust", "self"),
    ];

    #[test]
    fn data_format_and_keyword_scopes_keep_their_classes() {
        for dark in [false, true] {
            let syntax = syntax_of(dark);
            for &(source, lang, token, class) in SCOPED_FIXTURES {
                let job = highlight_in_theme(dark, source, Some(lang))
                    .unwrap_or_else(|| panic!("{lang} is mapped"));
                assert_token_color(&job, source, token, class.pick(syntax));
            }
            for &(source, lang, token) in UNSCOPED_FIXTURES {
                let job = highlight_in_theme(dark, source, Some(lang))
                    .unwrap_or_else(|| panic!("{lang} is mapped"));
                assert_token_color(&job, source, token, syntax.plain);
            }
        }
    }
    #[test]
    fn custom_themes_sit_under_the_keys_egui_extras_indexes() {
        let settings = settings();
        assert!(settings.ts.themes.contains_key(SYNTAX_THEME_DARK));
        assert!(settings.ts.themes.contains_key(SYNTAX_THEME_LIGHT));
    }

    #[test]
    fn highlight_keeps_the_source_and_emits_sections() {
        let job = highlight_in_theme(false, "fn main() { let value = 1; }", Some("rs"))
            .expect("rust is mapped");
        assert_eq!(job.text, "fn main() { let value = 1; }");
        assert!(!job.sections.is_empty());
        assert!(
            job.sections
                .iter()
                .any(|section| section.format.color != job.sections[0].format.color),
            "at least one token must differ from the first section's color"
        );
    }
}
