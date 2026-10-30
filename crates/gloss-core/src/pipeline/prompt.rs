//! 提示词模板注册表：任务类别、输入与语言环境 → 开放接口（OpenAI）兼容的
//! 消息序列。
//!
//! 任务书按五段骨架组织（角色与任务、输出形状、字段规则、任务约定）；
//! 渲染拼装为任务书、共享目标语言条款、共享输出纪律块三段顺接，纪律块
//! 垫底。模板与提示都不要求围栏——模型回复按严格裸格式（JSON）校验
//! （契约模块），围栏输出即违约进修复轮。模态矩阵之外的输入一律以模态
//! 不支持错误上抛，不发出注定无效的请求。
//!
//! 分类指令自包含：分类系统提示由骨架（分类模板，只留引言与输出形状）
//! 加类别段（措辞注册表按类别渲染）两段拼装，兜底类别由分类校验器映射
//! 兜底。注册表的键与枚举变体的双向集合相等由完整性测试锁定；运行时缺键
//! 有痕降级，回落固定措辞，不中断运行。渲染是显式的双花括号占位符替换
//! （可选行语义见渲染助手）。
//! 不注入模态提示行：源语言与代码语言都由模型从原文自行判断，代码语言经
//! 产物对象的代码语言字段回传界面。
//!
//! 消息契约类型、模板资产嵌入与修复轮会话组装（含降级批注行）分文件
//! 承载，本文件保留注册表、任务与分类的渲染拼装和渲染助手。
//!
//! 参数缺省：目标语言缺省中文；提示词语言
//! 缺省中文模板。

use std::borrow::Cow;
use std::sync::Arc;

use crate::model::{GlossError, Lang, Locale};
use crate::task::{Task, TaskInput, TaskKind, TaskOptions};
use crate::wordings;

mod message;
mod repair;
mod templates;

pub use message::{ChatMessage, ContentPart, MessageContent, Role};
pub(crate) use repair::repair_conversation;
pub use repair::{ViolationBrief, ViolationPositions, violation_briefs};

use templates::Templates;

pub(crate) const DEFAULT_TARGET: Lang = Lang::Zh;

#[derive(Debug, Default, Clone, Copy)]
pub struct PromptRegistry;

impl PromptRegistry {
    pub fn new() -> Self {
        Self
    }

    pub fn render(&self, task: &Task) -> Result<Vec<ChatMessage>, GlossError> {
        let locale = task.options().prompt_locale.unwrap_or_default();
        let target = target_display(task.options(), locale);
        match task.input() {
            TaskInput::Text { text, .. } => {
                let templates = locale.templates();
                let system = render_task_system(templates, task.kind(), &target);
                Ok(vec![
                    ChatMessage::system(system),
                    ChatMessage::user(text.to_owned()),
                ])
            }
            TaskInput::Image { png } => {
                let templates = locale.templates();
                let system = render_task_system(templates, task.kind(), &target);
                Ok(vec![
                    ChatMessage::system(system),
                    ChatMessage::user_parts(vec![ContentPart::ImagePng(Arc::clone(png))]),
                ])
            }
        }
    }

    pub fn render_classify(&self, locale: Locale, text: &str) -> Vec<ChatMessage> {
        let templates = locale.templates();
        let skeleton = render_template(templates.classify, &[]);
        let section = classify_category_section(locale);
        let system = if section.is_empty() {
            skeleton
        } else {
            format!("{skeleton}\n\n{section}")
        };
        vec![ChatMessage::system(system), ChatMessage::user(text)]
    }
}

fn classify_category_section(locale: Locale) -> String {
    let Some(section) = wordings::classify_section(locale) else {
        return String::new();
    };
    let separator = section.separator.as_str();
    let entries = crate::schema::CLASSIFY_CATEGORY_NAMES.map(|name| {
        match wordings::classify_definition(locale, name) {
            Some(definition) => format!("- {name}{separator}{definition}"),
            None => format!("- {name}"),
        }
    });
    format!("### {}\n\n{}", section.header, entries.join("\n\n"))
}

fn instruction_template(templates: Templates, kind: TaskKind) -> &'static str {
    match kind {
        TaskKind::TranslateWord => templates.word_card,
        TaskKind::TranslateSentence => templates.sentence,
        TaskKind::ExplainCode => templates.code,
        TaskKind::ImageExplain => templates.image_explain,
        TaskKind::General => templates.general,
    }
}

fn render_task_system(templates: Templates, kind: TaskKind, target: &str) -> String {
    let values = [("target", target)];
    let mut system = render_template(instruction_template(templates, kind), &values);
    system.push_str("\n\n");
    system.push_str(&render_template(templates.target_lang, &values));
    system.push_str("\n\n");
    system.push_str(&render_template(templates.discipline, &values));
    system
}

fn target_display(options: &TaskOptions, locale: Locale) -> Cow<'_, str> {
    lang_display(
        options.target_lang.as_ref().unwrap_or(&DEFAULT_TARGET),
        locale,
    )
}

fn lang_display(lang: &Lang, locale: Locale) -> Cow<'_, str> {
    let fallback = match lang {
        Lang::Zh => "Chinese",
        Lang::En => "English",
    };
    match wordings::target_lang(locale) {
        Some(names) => match lang {
            Lang::Zh => Cow::Borrowed(names.zh.as_str()),
            Lang::En => Cow::Borrowed(names.en.as_str()),
        },
        None => Cow::Borrowed(fallback),
    }
}

pub(crate) fn render_template(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut first = true;
    for line in template.split('\n') {
        let rendered = substitute_line(line, values);
        if rendered.all_empty {
            continue;
        }
        if !first {
            out.push('\n');
        }
        first = false;
        out.push_str(&rendered.text);
    }
    out
}

struct RenderedLine {
    text: String,
    all_empty: bool,
}

fn substitute_line(line: &str, values: &[(&str, &str)]) -> RenderedLine {
    let mut text = String::with_capacity(line.len());
    let mut rest = line;
    let mut declared = 0usize;
    let mut filled = 0usize;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            break;
        };
        let key = &after[..end];
        text.push_str(&rest[..start]);
        match values.iter().find(|(name, _)| *name == key) {
            Some((_, value)) => {
                declared += 1;
                if !value.is_empty() {
                    filled += 1;
                }
                text.push_str(value);
            }
            None => {
                text.push_str("{{");
                text.push_str(key);
                text.push_str("}}");
            }
        }
        rest = &after[end + 2..];
    }
    text.push_str(rest);
    RenderedLine {
        text,
        all_empty: declared > 0 && filled == 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_task(kind: TaskKind, text: &str) -> Task {
        Task::new(
            kind,
            TaskInput::Text { text: text.into() },
            TaskOptions::default(),
        )
        .expect("a text kind with text input is legal")
    }

    fn localized_task(kind: TaskKind, text: &str, locale: Locale) -> Task {
        let mut task = text_task(kind, text);
        task.options_mut().prompt_locale = Some(locale);
        task
    }

    fn image_task(kind: TaskKind) -> Task {
        Task::new(
            kind,
            TaskInput::Image {
                png: Arc::from(&b"png"[..]),
            },
            TaskOptions::default(),
        )
        .expect("an image kind with image input is legal")
    }

    fn text_of(content: &MessageContent) -> &str {
        match content {
            MessageContent::Text(text) => text,
            MessageContent::Parts(_) => panic!("expected plain-text content"),
        }
    }

    #[test]
    fn text_kinds_render_system_and_user_with_kind_content() {
        let registry = PromptRegistry::new();
        let cases = [
            (TaskKind::TranslateWord, "词典专家"),
            (TaskKind::TranslateSentence, "翻译专家"),
            (TaskKind::ExplainCode, "编程专家"),
            (TaskKind::General, "研究专家"),
        ];
        for (kind, keyword) in cases {
            let task = text_task(kind, "hello world");
            let messages = registry.render(&task).expect("text task should render");
            assert_eq!(messages.len(), 2, "{kind:?}");
            assert_eq!(messages[0].role, Role::System);
            assert_eq!(messages[1].role, Role::User);
            assert_eq!(
                messages[1].content,
                MessageContent::Text("hello world".into())
            );
            assert!(
                text_of(&messages[0].content).contains(keyword),
                "{kind:?} system prompt should mention {keyword}: {}",
                text_of(&messages[0].content)
            );
            assert!(
                text_of(&messages[0].content).contains("\"note\""),
                "{kind:?} must carry the pure-JSON output contract: {}",
                text_of(&messages[0].content)
            );
        }
    }

    #[test]
    fn output_example_carries_the_kind_fields() {
        let registry = PromptRegistry::new();

        let word = registry
            .render(&text_task(TaskKind::TranslateWord, "gloss"))
            .expect("render");
        let word_body = text_of(&word[0].content);
        assert!(word_body.contains("\"structured\""));
        assert!(word_body.contains("\"phonetic\""));
        assert!(word_body.contains("\"examples\""));
        assert!(word_body.contains("\"note\""));

        let plain = registry
            .render(&text_task(TaskKind::ExplainCode, "fn main() {}"))
            .expect("render");
        let plain_body = text_of(&plain[0].content);
        assert!(plain_body.contains("\"structured\""));
        assert!(plain_body.contains("\"examples\""));
        assert!(plain_body.contains("\"code_language\""));
        assert!(plain_body.contains("\"note\""));

        let sentence = registry
            .render(&text_task(TaskKind::TranslateSentence, "hello"))
            .expect("render");
        let sentence_body = text_of(&sentence[0].content);
        assert!(sentence_body.contains("\"structured\""));
        assert!(sentence_body.contains("\"examples\""));
        assert!(sentence_body.contains("\"note\""));
    }

    fn assert_clause_then_discipline_tail(
        system: &str,
        clause: &str,
        discipline: &str,
        label: &str,
    ) {
        let tail = system.trim_end();
        let head = tail
            .strip_suffix(discipline.trim_end())
            .unwrap_or_else(|| {
                panic!(
                    "{label}: the shared discipline block rides verbatim at the end; tail: {:?}",
                    &tail[tail.len().saturating_sub(120)..]
                )
            })
            .trim_end();
        assert!(
            head.ends_with(clause.trim_end()),
            "{label}: the shared clause rides verbatim right before the discipline block; tail: \
             {:?}",
            &head[head.len().saturating_sub(120)..]
        );
    }

    #[test]
    fn every_task_system_prompt_ends_with_the_discipline_after_the_shared_clause() {
        let registry = PromptRegistry::new();
        for locale in [Locale::Zh, Locale::En] {
            let templates = locale.templates();
            let target = lang_display(&DEFAULT_TARGET, locale);
            let values = [("target", target.as_ref())];
            let clause = super::render_template(templates.target_lang, &values);
            let discipline = super::render_template(templates.discipline, &values);
            for kind in [
                TaskKind::TranslateWord,
                TaskKind::TranslateSentence,
                TaskKind::ExplainCode,
                TaskKind::General,
            ] {
                let mut task = text_task(kind, "gloss");
                task.options_mut().prompt_locale = Some(locale);
                let messages = registry.render(&task).expect("render");
                let label = format!("{locale:?} x {kind:?}");
                assert_clause_then_discipline_tail(
                    text_of(&messages[0].content),
                    &clause,
                    &discipline,
                    &label,
                );
            }
            let mut image = Task::new(
                TaskKind::ImageExplain,
                TaskInput::Image {
                    png: Arc::from(&b"png"[..]),
                },
                TaskOptions::default(),
            )
            .expect("image task is legal");
            image.options_mut().prompt_locale = Some(locale);
            let messages = registry.render(&image).expect("render");
            let label = format!("{locale:?} image task");
            assert_clause_then_discipline_tail(
                text_of(&messages[0].content),
                &clause,
                &discipline,
                &label,
            );
        }
    }

    #[test]
    fn missing_target_lang_defaults_to_chinese() {
        let registry = PromptRegistry::new();
        let messages = registry
            .render(&text_task(TaskKind::TranslateSentence, "hello"))
            .expect("render");
        assert!(
            text_of(&messages[0].content).contains("中文"),
            "default target: {}",
            text_of(&messages[0].content)
        );
    }

    #[test]
    fn explicit_target_lang_is_rendered() {
        let registry = PromptRegistry::new();
        let mut task = text_task(TaskKind::TranslateSentence, "hello");
        task.options_mut().target_lang = Some(Lang::En);
        let messages = registry.render(&task).expect("render");
        assert!(text_of(&messages[0].content).contains("英语"));
    }

    #[test]
    fn image_explain_renders_system_and_image_parts() {
        let registry = PromptRegistry::new();
        let png: Arc<[u8]> = Arc::from(&b"png"[..]);
        let task = Task::new(
            TaskKind::ImageExplain,
            TaskInput::Image {
                png: Arc::clone(&png),
            },
            TaskOptions::default(),
        )
        .expect("an image kind with image input is legal");
        let messages = registry.render(&task).expect("image explain should render");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, Role::System);
        assert_eq!(messages[1].role, Role::User);
        assert_eq!(
            messages[1].content,
            MessageContent::Parts(vec![ContentPart::ImagePng(png)])
        );
        let system = text_of(&messages[0].content);
        assert!(
            system.contains("（JSON）格式") || system.contains("JSON format"),
            "the image contract must demand JSON output: {system}"
        );
        assert!(system.contains("\"note\""), "{system}");
        assert!(system.contains("\"structured\""), "{system}");
        assert!(system.contains("\"examples\""), "{system}");
        assert!(!system.contains("{{"), "{system}");
    }

    #[test]
    fn modality_mismatch_is_rejected_at_construction() {
        let task = Task::new(
            TaskKind::ImageExplain,
            TaskInput::Text {
                text: "not an image".into(),
            },
            TaskOptions::default(),
        );
        assert_eq!(task, Err(GlossError::UnsupportedModality));
    }

    #[test]
    fn classify_prompt_carries_the_template_categories_and_the_text() {
        let registry = PromptRegistry::new();
        for locale in [Locale::Zh, Locale::En] {
            let messages = registry.render_classify(locale, "gloss 原文");
            assert_eq!(messages.len(), 2);
            assert_eq!(
                messages[1].content,
                MessageContent::Text("gloss 原文".into())
            );
            let system = text_of(&messages[0].content);
            for category in [
                "TranslateWord",
                "TranslateSentence",
                "ExplainCode",
                "Fallback",
            ] {
                assert!(system.contains(category), "{locale:?}: {system}");
            }
            assert!(system.contains("\"kind\""));
            assert!(!system.contains("{{"));
        }
    }

    #[test]
    fn classify_prompt_composes_the_skeleton_and_the_registry_category_section() {
        let registry = PromptRegistry::new();
        for locale in [Locale::Zh, Locale::En] {
            let messages = registry.render_classify(locale, "gloss 原文");
            let system = text_of(&messages[0].content);
            let skeleton = locale.templates().classify;
            assert!(
                system.starts_with(skeleton.trim_end()),
                "{locale:?}: the skeleton (intro and output shape) rides verbatim first: \
                 {system}"
            );
            let section = match locale {
                Locale::Zh => "### 类别",
                Locale::En => "### Categories",
            };
            assert!(
                system.contains(section),
                "{locale:?}: the category section follows the skeleton: {system}"
            );
            for name in crate::schema::CLASSIFY_CATEGORY_NAMES {
                assert!(
                    system.contains(&format!("- {name}")),
                    "{locale:?}: every template category carries a registry definition: \
                     {system}"
                );
            }
            assert!(!system.contains("{{"), "{locale:?}: {system}");
        }
        let zh_messages = registry.render_classify(Locale::Zh, "原文");
        assert!(
            text_of(&zh_messages[0].content).contains("- TranslateWord：翻译单个词汇项"),
            "the zh section renders the migrated definition: {}",
            text_of(&zh_messages[0].content)
        );
        let en_messages = registry.render_classify(Locale::En, "text");
        assert!(
            text_of(&en_messages[0].content).contains(
                "- TranslateWord: Translate a single \
                                                      lexical item"
            ),
            "the en section renders the migrated definition: {}",
            text_of(&en_messages[0].content)
        );
    }

    #[test]
    fn the_general_kind_is_never_named_by_the_classify_template() {
        let registry = PromptRegistry::new();
        for locale in [Locale::Zh, Locale::En] {
            let messages = registry.render_classify(locale, "gloss 原文");
            let system = text_of(&messages[0].content);
            assert!(
                !system.contains("General"),
                "{locale:?}: the general kind is the execution landing, the template \
                 speaks in the Fallback category: {system}"
            );
        }
    }

    #[test]
    fn render_template_substitutes_placeholders() {
        assert_eq!(
            render_template("a {{one}} b {{two}} c", &[("one", "1"), ("two", "2")]),
            "a 1 b 2 c"
        );
        assert_eq!(
            render_template("{{one}}{{one}}", &[("one", "x")]),
            "xx",
            "the same placeholder may repeat"
        );
        assert_eq!(
            render_template("{{missing}} and {{unclosed", &[("one", "1")]),
            "{{missing}} and {{unclosed",
            "undeclared or unclosed placeholders stay verbatim for the completeness test"
        );
    }

    #[test]
    fn render_template_drops_lines_whose_placeholders_are_empty() {
        let hints = "代码语言：{{code_lang}}\n源语言：{{source_lang}}\n";
        assert_eq!(
            render_template(hints, &[("code_lang", ""), ("source_lang", "")]),
            "",
            "an all-empty placeholder line takes its label and newline with it"
        );
        assert_eq!(
            render_template(hints, &[("code_lang", "rust"), ("source_lang", "")]),
            "代码语言：rust\n",
            "the filled line stays, the empty one goes"
        );

        let instruction = "使用{{target}}。\n{{hint}}\n";
        assert_eq!(
            render_template(instruction, &[("target", "中文"), ("hint", "")]),
            "使用中文。\n"
        );
        assert_eq!(
            render_template(instruction, &[("target", ""), ("hint", "H")]),
            "H\n",
            "a line whose only placeholder is empty is optional as a whole"
        );
    }

    #[test]
    fn render_template_keeps_blank_lines_without_placeholders() {
        assert_eq!(
            render_template("a\n\nb\n", &[("unused", "1")]),
            "a\n\nb\n",
            "blank lines without placeholders are structural"
        );
    }

    #[test]
    fn every_locale_renders_without_leftover_placeholders() {
        let registry = PromptRegistry::new();
        let kinds = [
            TaskKind::TranslateWord,
            TaskKind::TranslateSentence,
            TaskKind::ExplainCode,
            TaskKind::ImageExplain,
            TaskKind::General,
        ];
        for locale in [Locale::Zh, Locale::En] {
            for kind in kinds {
                let mut task = match kind {
                    TaskKind::ImageExplain => image_task(kind),
                    _ => text_task(kind, "gloss"),
                };
                task.options_mut().prompt_locale = Some(locale);
                let messages = registry.render(&task).expect("render");
                for message in &messages {
                    let text = match &message.content {
                        MessageContent::Text(text) => text.as_str(),
                        MessageContent::Parts(_) => continue,
                    };
                    assert!(
                        !text.contains("{{"),
                        "{locale:?} x {kind:?} leaves a placeholder: {text}"
                    );
                }
                assert!(
                    text_of(&messages[0].content).contains("\"note\""),
                    "{locale:?} x {kind:?} must carry the pure-JSON contract"
                );
                assert!(
                    !text_of(&messages[0].content).contains("```gloss"),
                    "{locale:?} x {kind:?}: the legacy fence marker must not appear in \
                     the current prompt"
                );
            }
        }
    }

    #[test]
    fn english_locale_renders_english_prompts() {
        let registry = PromptRegistry::new();
        let messages = registry
            .render(&localized_task(
                TaskKind::TranslateSentence,
                "bonjour",
                Locale::En,
            ))
            .expect("render");
        assert!(text_of(&messages[0].content).contains("translation expert"));
        assert!(text_of(&messages[0].content).contains("JSON format"));
        assert!(text_of(&messages[1].content).contains("bonjour"));
        assert!(!text_of(&messages[0].content).contains("翻译专家"));
    }

    #[test]
    fn prompt_locale_is_independent_of_target_language() {
        let registry = PromptRegistry::new();

        let english_prompt = registry
            .render(&localized_task(
                TaskKind::TranslateSentence,
                "hello",
                Locale::En,
            ))
            .expect("render");
        assert!(text_of(&english_prompt[0].content).contains("translation expert"));
        assert!(
            text_of(&english_prompt[0].content).contains("Chinese"),
            "template language must not move the default target: {}",
            text_of(&english_prompt[0].content)
        );

        let mut chinese_prompt = localized_task(TaskKind::TranslateSentence, "hello", Locale::Zh);
        chinese_prompt.options_mut().target_lang = Some(Lang::En);
        let messages = registry.render(&chinese_prompt).expect("render");
        assert!(text_of(&messages[0].content).contains("翻译专家"));
        assert!(text_of(&messages[0].content).contains("英语"));
    }

    #[test]
    fn prompt_locale_defaults_to_chinese() {
        let registry = PromptRegistry::new();
        let defaulted = registry
            .render(&text_task(TaskKind::TranslateWord, "gloss"))
            .expect("render");
        let explicit = registry
            .render(&localized_task(
                TaskKind::TranslateWord,
                "gloss",
                Locale::Zh,
            ))
            .expect("render");
        assert_eq!(defaulted, explicit, "no locale means the Chinese templates");
        assert!(text_of(&defaulted[0].content).contains("词典专家"));
        assert_eq!(Locale::default(), Locale::Zh);
    }
}
