//! 代码语言采纳与语法着色适配：语言名归一化、syntect 语法映射与六类色板
//! 主题。
//!
//! 语言的唯一来源是任务产物的 `structured.code_language`（LLM 判定，采纳
//! 见 [`adopt`]）；[`syntect_token`] 把它映射到 Sublime 语法名，[`highlight`]
//! 经 egui_extras 出 [`LayoutJob`]。着色类别与色板在 [`palette`]。
//!
//! egui_extras 的 `CodeTheme` 只暴露内置主题、字段私有，自定义六类色板
//! 只能顶替 `SyntectSettings.ts` 里它按名字索引的两个主题条目（见
//! [`theme_set`]）。

mod fallback;
mod palette;

use std::str::FromStr;
use std::sync::OnceLock;

use egui::FontId;
use egui::text::LayoutJob;
use egui_extras::syntax_highlighting::{CodeTheme, SyntectSettings, highlight_with};
use syntect::highlighting::{
    Color, FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSet, ThemeSettings,
};

pub(crate) use fallback::fallback_job;

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

/// 采纳产物的代码语言判定（渲染时现算）：修剪首尾空白后过词元门槛——
/// 语言标识应是单个 ASCII 词元（"rust"、"c++"、"objective-c" 这个形状，
/// 内部含空白或非 ASCII 字符的判定是散文或示例占位串）——再经归一化
/// （别名/大小写）落规范名。判定缺失或不可采纳时为 `None`：无角标、按
/// 通用启发集着色，自由文本不上角标。
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

/// 规范语言名 → Sublime 语法名。syntect 按名字精确匹配（大小写敏感），
/// 这里用语法集的实名而不是扩展名，避免 `.m` 之类多语言共用扩展名的歧义。
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

/// 规范语言名对应的 Sublime 语法名；表外语言返回 `None`——调用方落纯文本
/// 面板，不猜语言。
pub(crate) fn syntect_token(lang: Option<&str>) -> Option<&'static str> {
    let name = normalize_language(lang?)?;
    SYNTAX_NAMES
        .iter()
        .find(|(canonical, _)| *canonical == name.as_str())
        .map(|(_, token)| *token)
}

/// 代码正文着色：syntect 分词 + egui_extras 出 `LayoutJob`；语言未定或
/// 表外语言返回 `None`，由绘制侧落 [`fallback_job`] 的通用启发集（不猜
/// 具体语言，只认跨语言稳定的字符串/注释/数字/类型/函数形）。
pub(crate) fn highlight(
    ctx: &egui::Context,
    style: &egui::Style,
    dark: bool,
    font_id: FontId,
    source: &str,
    lang: Option<&str>,
) -> Option<LayoutJob> {
    let token = syntect_token(lang)?;
    let theme = if dark {
        CodeTheme::dark(font_id.size)
    } else {
        CodeTheme::light(font_id.size)
    }
    .with_font_id(font_id);
    Some(highlight_with(
        ctx,
        style,
        &theme,
        source,
        token,
        settings(),
    ))
}

/// egui_extras 的 `CodeTheme::dark/light` 在 `SyntectSettings.ts` 里按这两个
/// 键取主题；自定义色板只能顶替这两个内置条目（`CodeTheme` 字段私有，
/// 没有注入自定义 tmTheme 的公开入口）。
const DARK_THEME_KEY: &str = "base16-mocha.dark";
const LIGHT_THEME_KEY: &str = "Solarized (light)";

/// 进程内唯一的语法集与主题集。语法集用 two-face 的 bat 补集（syntect
/// 默认集缺 Kotlin/Swift/Dart/Elixir/Julia/TOML/TypeScript）。`highlight_with`
/// 的缓存按这个引用的地址作键，必须取 `OnceLock` 里的同一份，不能逐帧新建。
fn settings() -> &'static SyntectSettings {
    static SETTINGS: OnceLock<SyntectSettings> = OnceLock::new();
    SETTINGS.get_or_init(|| SyntectSettings {
        ps: two_face::syntax::extra_newlines(),
        ts: theme_set(),
    })
}

/// 只保留 egui_extras 会取用的两个键，其余内置主题不装——省掉默认主题集
/// 的解析，也让「自定义色板顶替内置条目」这件事在类型上只有一处。
fn theme_set() -> ThemeSet {
    let mut themes = ThemeSet::new();
    themes
        .themes
        .insert(DARK_THEME_KEY.to_owned(), gloss_theme(true));
    themes
        .themes
        .insert(LIGHT_THEME_KEY.to_owned(), gloss_theme(false));
    themes
}

/// 把六类色板包成 syntect 主题：scope 选择器只做「语法角色 → 类别」的
/// 映射，颜色仍由 [`palette::Class`] 单点决定。
fn gloss_theme(dark: bool) -> Theme {
    Theme {
        name: Some("gloss".to_owned()),
        author: None,
        settings: ThemeSettings {
            foreground: Some(syntect_color(palette::plain_color(dark))),
            ..ThemeSettings::default()
        },
        scopes: vec![
            theme_item("comment", palette::Class::Comment, dark),
            theme_item("string", palette::Class::String, dark),
            theme_item("constant.numeric", palette::Class::Number, dark),
            theme_item("constant.language", palette::Class::Keyword, dark),
            theme_item("keyword.control", palette::Class::Keyword, dark),
            theme_item("keyword.other", palette::Class::Keyword, dark),
            theme_item("keyword.declaration", palette::Class::Keyword, dark),
            theme_item("storage", palette::Class::Keyword, dark),
            theme_item("storage.type", palette::Class::Keyword, dark),
            theme_item("storage.modifier", palette::Class::Keyword, dark),
            theme_item("entity.name.tag", palette::Class::Keyword, dark),
            theme_item("entity.name.type", palette::Class::Type, dark),
            theme_item("entity.name.class", palette::Class::Type, dark),
            theme_item("support.type", palette::Class::Type, dark),
            theme_item("support.type.property-name", palette::Class::Type, dark),
            theme_item("entity.name.function", palette::Class::Function, dark),
            theme_item("support.function", palette::Class::Function, dark),
            theme_item("variable.function", palette::Class::Function, dark),
            theme_item(
                "entity.other.attribute-name",
                palette::Class::Function,
                dark,
            ),
            theme_item("entity.name.section", palette::Class::Function, dark),
        ],
    }
}

/// 一条主题规则。scope 全是静态字面量：解析失败只在开发期断言，release
/// 落空选择器（该条规则不生效），不让静态表的笔误把绘制路径打崩。
fn theme_item(scope: &str, class: palette::Class, dark: bool) -> ThemeItem {
    let scope = ScopeSelectors::from_str(scope).unwrap_or_else(|error| {
        debug_assert!(false, "static theme scope failed: {error:?}");
        ScopeSelectors::default()
    });
    ThemeItem {
        scope,
        style: StyleModifier {
            foreground: Some(syntect_color(class.color(dark))),
            font_style: class.italic().then_some(FontStyle::ITALIC),
            ..StyleModifier::default()
        },
    }
}

/// egui 的 Color32 是预乘 alpha；色板里的类别色都是不透明色，逐通道搬运
/// 即可。
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
    fn custom_themes_sit_under_the_keys_egui_extras_indexes() {
        let settings = settings();
        assert!(settings.ts.themes.contains_key(DARK_THEME_KEY));
        assert!(settings.ts.themes.contains_key(LIGHT_THEME_KEY));
    }

    #[test]
    fn highlight_keeps_the_source_and_emits_sections() {
        let ctx = crate::ui::context::new_context(gloss_core::config::Theme::Light);
        let style = egui::Style::default();
        let job = highlight(
            &ctx,
            &style,
            false,
            egui::FontId::monospace(12.0),
            "fn main() { let value = 1; }",
            Some("rs"),
        )
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
