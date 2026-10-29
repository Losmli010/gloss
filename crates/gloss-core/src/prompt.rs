//! 提示词模板注册表：任务类别、输入与语言环境 → 开放接口（OpenAI）兼容的
//! 消息序列。
//!
//! 任务侧一个任务一份自包含模板（词卡、句译、代码解释、图像解读、通用）：
//! 任务说明与纯格式（JSON）输出契约（含一份“中性占位”的输出示例）都在
//! 同一个文件里，改一个任务的任务书不会牵动其它任务。任务书按五段骨架
//! 组织（角色与任务、输出形状、字段规则、任务约定）；渲染拼装为任务书、
//! 共享目标语言条款、共享输出纪律块三段顺接，纪律块垫底。契约按经注疏与
//! 说文解字的层次组织，与核心层契约模块的三行同构表同源：注（义）恒为
//! 首字段，按类别的疏证字段收敛在疏证对象里——词卡
//! 是音标与例句（音、例，字头即选区原文），句译、讲解、
//! 图像解读与通用任务是展开条目，代码另带由大语言模型（LLM）判定的
//! 代码语言（界面角标与高亮使用）。
//! 模板里的输出示例就是解析侧（任务校验器）所吃形状
//! 的唯一描述，两处改一须改二（完整性门禁测试锁定：每个模板的示例对象
//! 必须通过对应校验）。模板与提示都不要求围栏——模型回复按
//! 严格裸格式（JSON）校验（契约模块），围栏输出即违约进修复轮。模态矩阵
//! 之外的输入一律以模态不支持错误上抛，不发出注定无效的请求。
//!
//! 分类与修复的指令同样自包含：分类系统提示由骨架（分类模板，只留引言
//! 与输出形状）加类别段（措辞注册表按类别渲染）两段拼装，兜底类别由
//! 分类校验器映射兜底；修复指令全部由措辞注册表组合渲染——三段修复轮
//! （任务、分类、语言）共用一份骨架（头、行、尾），任务与分类段的行是
//! 违约级或细节措辞行，语言段的行由聚合、排序与封顶在组合层落成
//! （同字段同类违约合并为一行、按字段序排序、封顶附截断注），回指句只
//! 指称上一条回复的身份。注册表的键与枚举变体的双向集合相等由完整性
//! 测试锁定；运行时缺键有痕降级，回落固定措辞，不中断运行。模板文本
//! 是编译期嵌入的文件资源（核心层内嵌的各语言模板文件与措辞注册表），
//! 渲染是显式的双花括号占位符替换（可选行语义见渲染助手）。
//! 不注入模态提示行：源语言与代码语言都由模型从原文自行判断，代码语言经
//! 产物对象的代码语言字段回传界面。
//!
//! 语言降级的界面批注行与本文件同源：公开的批注行清单函数用同一行模板
//! 与同一聚合算法渲染批注行，只换注入的词——位置词与规则名由调用方注
//! 入（位置词是界面文案，规则名取注册表规则名区，与语言违约区同册），
//! 动作措辞复用语言违约区原文，超限附注册表截断注；注册表缺席或查键未
//! 中得空清单，由界面回落泛化文案。批注行只描述字段位置与规则，不引回
//! 复原文。人机两册在此分道：修复轮的行给模型，批注行给人，行式与聚合
//! 保持单一事实源。
//!
//! 本模块只产“域形态”的消息（角色与消息部件），
//! 不做传输序列化，也不携带内联数据——把部件数组映成开放接口（OpenAI）
//! 兼容内容、把图像字节编码成数据地址前缀都归传输层，
//! 即大语言模型（LLM）客户端的请求体组装。
//!
//! 模板内容面向模型，用各语言环境的语言书写，不受“日志一律英文”门禁约束
//! （约束门禁只查日志宏实参）。
//!
//! 参数缺省：目标语言缺省中文；提示词语言
//! 缺省中文模板。

use std::borrow::Cow;
use std::sync::Arc;

use crate::briefs::{aggregate_groups, joined_example_indices};
use crate::lang_gate::{LangField, LangViolation};
use crate::model::{GlossError, Lang, Locale};
use crate::schema::{FormatDetail, Violation};
use crate::task::{Task, TaskInput, TaskKind, TaskOptions};
use crate::wordings;

pub use crate::briefs::{ViolationBrief, ViolationPositions, violation_briefs};

pub(crate) const DEFAULT_TARGET: Lang = Lang::Zh;

const LANG_REPAIR_ROW_CAP: usize = 12;

#[derive(Debug, Clone, Copy)]
struct Templates {
    word_card: &'static str,
    sentence: &'static str,
    code: &'static str,
    image_explain: &'static str,
    general: &'static str,
    target_lang: &'static str,
    discipline: &'static str,
    classify: &'static str,
}

impl Locale {
    fn templates(self) -> Templates {
        match self {
            Locale::Zh => Templates {
                word_card: include_str!("../prompts/zh/word_card.md"),
                sentence: include_str!("../prompts/zh/sentence.md"),
                code: include_str!("../prompts/zh/code.md"),
                image_explain: include_str!("../prompts/zh/image_explain.md"),
                general: include_str!("../prompts/zh/general.md"),
                target_lang: include_str!("../prompts/zh/target_lang.md"),
                discipline: include_str!("../prompts/zh/discipline.md"),
                classify: include_str!("../prompts/zh/classify.md"),
            },
            Locale::En => Templates {
                word_card: include_str!("../prompts/en/word_card.md"),
                sentence: include_str!("../prompts/en/sentence.md"),
                code: include_str!("../prompts/en/code.md"),
                image_explain: include_str!("../prompts/en/image_explain.md"),
                general: include_str!("../prompts/en/general.md"),
                target_lang: include_str!("../prompts/en/target_lang.md"),
                discipline: include_str!("../prompts/en/discipline.md"),
                classify: include_str!("../prompts/en/classify.md"),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentPart {
    ImagePng(Arc<[u8]>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: Role,
    pub content: MessageContent,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: MessageContent::Text(content.into()),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: MessageContent::Text(content.into()),
        }
    }

    pub fn user_parts(parts: Vec<ContentPart>) -> Self {
        Self {
            role: Role::User,
            content: MessageContent::Parts(parts),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: MessageContent::Text(content.into()),
        }
    }
}

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
        let system = format!(
            "{}\n\n{}",
            render_template(templates.classify, &[]),
            classify_category_section(locale),
        );
        vec![ChatMessage::system(system), ChatMessage::user(text)]
    }

    pub fn render_repair(
        &self,
        locale: Locale,
        violation: Violation,
        detail: Option<FormatDetail>,
    ) -> String {
        let key = detail
            .map(FormatDetail::key)
            .unwrap_or_else(|| violation.key());
        let frame = wordings::frame(locale);
        let row = repair_row(frame, wordings::format_row(locale, key), violation);
        match frame {
            Some(frame) => repair_instruction(frame, &[row], false),
            None => row,
        }
    }

    pub fn render_language_repair(&self, locale: Locale, violations: &[LangViolation]) -> String {
        let frame = wordings::frame(locale);
        let (rows, truncated) = language_repair_rows(locale, frame, violations);
        let Some(frame) = frame else {
            return rows.join("\n");
        };
        let mut instruction = String::new();
        instruction.push_str(frame.back_reference.trim_end());
        instruction.push('\n');
        instruction.push_str(&repair_instruction(frame, &rows, truncated));
        instruction
    }
}

fn classify_category_section(locale: Locale) -> String {
    let (header, separator) = match locale {
        Locale::Zh => ("### 类别", "："),
        Locale::En => ("### Categories", ": "),
    };
    let entries = crate::schema::CLASSIFY_CATEGORY_NAMES.map(|name| {
        match wordings::classify_definition(locale, name) {
            Some(definition) => format!("- {name}{separator}{definition}"),
            None => format!("- {name}"),
        }
    });
    format!("{header}\n\n{}", entries.join("\n\n"))
}

fn repair_instruction(frame: &wordings::FrameWording, rows: &[String], truncated: bool) -> String {
    let mut out = String::new();
    out.push_str(frame.header.trim_end());
    for row in rows {
        out.push('\n');
        out.push_str(row);
    }
    if truncated {
        out.push('\n');
        out.push_str(frame.truncated_note.trim_end());
    }
    out.push('\n');
    out.push_str(frame.closing.trim_end());
    out
}

fn repair_row(
    frame: Option<&wordings::FrameWording>,
    wording: Option<&wordings::RowWording>,
    violation: Violation,
) -> String {
    let Some((frame, wording)) = frame.zip(wording) else {
        return violation.to_string();
    };
    render_template(
        &frame.row,
        &[
            ("position", wording.position.as_str()),
            ("rule", violation.key()),
            ("action", wording.action.as_str()),
        ],
    )
}

fn lang_position(locale: Locale, group: &crate::briefs::LangGroup<'_>) -> String {
    match group.field {
        LangField::Note => "note".to_owned(),
        LangField::Phonetic => "phonetic".to_owned(),
        LangField::Example(_) => {
            let joined = joined_example_indices(locale, group);
            match locale {
                Locale::Zh => format!("examples 第 {joined} 条"),
                Locale::En if group.example_indices.len() == 1 => {
                    format!("examples item {joined}")
                }
                Locale::En => format!("examples items {joined}"),
            }
        }
    }
}

fn group_row(
    locale: Locale,
    frame: Option<&wordings::FrameWording>,
    group: &crate::briefs::LangGroup<'_>,
) -> String {
    let rule = group.category.rule_id();
    let Some((frame, action)) = frame.zip(wordings::lang_action(locale, rule)) else {
        return group.first.line();
    };
    render_template(
        &frame.row,
        &[
            ("position", lang_position(locale, group).as_str()),
            ("rule", rule),
            ("action", action),
        ],
    )
}

fn language_repair_rows(
    locale: Locale,
    frame: Option<&wordings::FrameWording>,
    violations: &[LangViolation],
) -> (Vec<String>, bool) {
    let mut rows = Vec::new();
    let mut truncated = false;
    for group in aggregate_groups(violations) {
        if rows.len() == LANG_REPAIR_ROW_CAP {
            truncated = true;
            break;
        }
        rows.push(group_row(locale, frame, &group));
    }
    (rows, truncated)
}

pub(crate) fn repair_conversation(
    base: &[ChatMessage],
    bad: &str,
    instruction: String,
) -> Vec<ChatMessage> {
    let mut messages = Vec::with_capacity(base.len() + 2);
    messages.extend_from_slice(base);
    messages.push(ChatMessage::assistant(bad));
    messages.push(ChatMessage::user(instruction));
    messages
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
    let display = lang_display(
        options.target_lang.as_ref().unwrap_or(&DEFAULT_TARGET),
        locale,
    );
    if display.trim().is_empty() {
        lang_display(&DEFAULT_TARGET, locale)
    } else {
        display
    }
}

fn lang_display(lang: &Lang, locale: Locale) -> Cow<'_, str> {
    match locale {
        Locale::Zh => match lang {
            Lang::Zh => Cow::Borrowed("中文"),
            Lang::En => Cow::Borrowed("英语"),
            Lang::Ja => Cow::Borrowed("日语"),
            Lang::Ko => Cow::Borrowed("韩语"),
            Lang::Fr => Cow::Borrowed("法语"),
            Lang::Other(name) => Cow::Borrowed(name.as_str()),
        },
        Locale::En => match lang {
            Lang::Zh => Cow::Borrowed("Chinese"),
            Lang::En => Cow::Borrowed("English"),
            Lang::Ja => Cow::Borrowed("Japanese"),
            Lang::Ko => Cow::Borrowed("Korean"),
            Lang::Fr => Cow::Borrowed("French"),
            Lang::Other(name) => Cow::Borrowed(name.as_str()),
        },
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
    use crate::lang_gate::LangCategory;

    const ALL_LANG_CATEGORIES: [LangCategory; 11] = [
        LangCategory::ForeignRun,
        LangCategory::TraditionalChar,
        LangCategory::PunctuationMismatch,
        LangCategory::MissingSpace,
        LangCategory::EmojiForbidden,
        LangCategory::MarkdownForbidden,
        LangCategory::UnclosedContainer,
        LangCategory::ContainerMixing,
        LangCategory::SentenceInParens,
        LangCategory::SourceOverlap,
        LangCategory::PhoneticCharset,
    ];

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

    fn example_object(template: &str) -> &str {
        template
            .lines()
            .find(|line| line.trim_start().starts_with('{'))
            .expect("the output example must be a JSON object line")
    }

    fn assert_structured_keys(locale: Locale, name: &str, object: &str, keys: &[&str]) {
        let value: serde_json::Value = serde_json::from_str(object).expect("example must parse");
        let structured = value
            .get("structured")
            .and_then(|value| value.as_object())
            .unwrap_or_else(|| {
                panic!("{locale:?}/{name}: the contract nests fields under a structured object")
            });
        for key in keys {
            assert!(
                structured.contains_key(*key),
                "{locale:?}/{name}: structured must carry {key:?}: {object}"
            );
        }
    }

    fn structured_keys(name: &str) -> &'static [&'static str] {
        match name {
            "word_card.md" => &["phonetic", "examples"],
            "code.md" => &["examples", "code_language"],
            _ => &["examples"],
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

    #[test]
    fn output_example_carries_the_note_field() {
        for locale in [Locale::Zh, Locale::En] {
            let templates = locale.templates();
            for (name, text) in [
                ("word_card.md", templates.word_card),
                ("sentence.md", templates.sentence),
                ("code.md", templates.code),
                ("image_explain.md", templates.image_explain),
                ("general.md", templates.general),
            ] {
                let object = example_object(text);
                let value: serde_json::Value = serde_json::from_str(object)
                    .unwrap_or_else(|err| panic!("{locale:?}/{name}: example must parse: {err}"));
                let keys = value.as_object().expect("example object");
                assert!(
                    keys.contains_key("note"),
                    "{locale:?}/{name}: the example must carry the note field"
                );
            }
        }
    }

    #[test]
    fn every_task_template_example_passes_validate_task() {
        use crate::schema::validate_task;

        for locale in [Locale::Zh, Locale::En] {
            let templates = locale.templates();
            for (name, kind, text) in [
                ("word_card.md", TaskKind::TranslateWord, templates.word_card),
                (
                    "sentence.md",
                    TaskKind::TranslateSentence,
                    templates.sentence,
                ),
                ("code.md", TaskKind::ExplainCode, templates.code),
                (
                    "image_explain.md",
                    TaskKind::ImageExplain,
                    templates.image_explain,
                ),
                ("general.md", TaskKind::General, templates.general),
            ] {
                let object = example_object(text);
                assert_eq!(
                    validate_task(kind, object)
                        .map(|(note, _)| note)
                        .map_err(|violation| violation.to_string()),
                    Ok("<string>".to_owned()),
                    "{locale:?}/{name}: the example object must pass validate_task"
                );
                assert_structured_keys(locale, name, object, structured_keys(name));
            }
        }
    }

    #[test]
    fn every_template_mentions_json() {
        for locale in [Locale::Zh, Locale::En] {
            let templates = locale.templates();
            for (name, text) in [
                ("word_card.md", templates.word_card),
                ("sentence.md", templates.sentence),
                ("code.md", templates.code),
                ("image_explain.md", templates.image_explain),
                ("general.md", templates.general),
                ("classify.md", templates.classify),
                ("discipline.md", templates.discipline),
            ] {
                assert!(
                    text.to_lowercase().contains("json"),
                    "{locale:?}/{name}: the prompt must mention json (the endpoint's \
                     JSON-mode gate reads it)"
                );
            }
        }
    }

    #[test]
    fn repair_instruction_renders_without_leftover_placeholders() {
        let registry = PromptRegistry::new();
        let details = [
            None,
            Some(FormatDetail::Fenced),
            Some(FormatDetail::NotAnObject),
            Some(FormatDetail::Truncated),
            Some(FormatDetail::NoteKeyMissing),
            Some(FormatDetail::NoteNotString),
        ];
        for locale in [Locale::Zh, Locale::En] {
            for violation in [
                Violation::EmptyReply,
                Violation::NotJson,
                Violation::MissingNote,
                Violation::KindNotAllowed,
            ] {
                for detail in details {
                    let instruction = registry.render_repair(locale, violation, detail);
                    assert!(
                        !instruction.contains("{{"),
                        "{locale:?} x {violation:?} x {detail:?} leaves a placeholder: \
                         {instruction}"
                    );
                    assert!(
                        !instruction.is_empty(),
                        "{locale:?} x {violation:?} x {detail:?}: the wording keeps the \
                         instruction non-empty"
                    );
                    assert!(
                        instruction.to_lowercase().contains("json"),
                        "{locale:?} x {violation:?} x {detail:?}: the repair prompt must \
                         mention json (the endpoint's JSON-mode gate reads it): {instruction}"
                    );
                }
            }
        }
    }

    const DETAIL_WORDING_CASES: [(FormatDetail, &str, &str, &str, &str); 5] = [
        (
            FormatDetail::Fenced,
            "回复被代码围栏包裹",
            "去掉围栏",
            "wrapped in a code fence",
            "Remove the fence",
        ),
        (
            FormatDetail::NotAnObject,
            "不是对象",
            "改为以花括号开头结尾的单个对象",
            "not an object",
            "single object",
        ),
        (
            FormatDetail::Truncated,
            "对象未闭合",
            "精简内容后完整输出",
            "not closed",
            "Trim the content",
        ),
        (
            FormatDetail::NoteKeyMissing,
            "缺少 `note` 键",
            "补上 `note` 键",
            "missing the `note` key",
            "Add the `note` key",
        ),
        (
            FormatDetail::NoteNotString,
            "不是非空字符串",
            "改为非空字符串",
            "non-empty string",
            "non-empty string",
        ),
    ];

    #[test]
    fn repair_instruction_composes_the_detail_wording_onto_the_skeleton() {
        let registry = PromptRegistry::new();
        for locale in [Locale::Zh, Locale::En] {
            for (detail, zh_position, zh_action, en_position, en_action) in DETAIL_WORDING_CASES {
                let instruction = registry.render_repair(locale, Violation::NotJson, Some(detail));
                let (needle, action) = match locale {
                    Locale::Zh => (zh_position, zh_action),
                    Locale::En => (en_position, en_action),
                };
                assert!(
                    instruction.contains(needle) && instruction.contains(action),
                    "{locale:?} x {detail:?}: the detail position and action ride the \
                     instruction: {instruction}"
                );
                assert!(
                    instruction.contains("NotJson"),
                    "{locale:?} x {detail:?}: the row names the violated rule id: \
                     {instruction}"
                );
                for marker in ["MARKER", "{\"note\":\"", "选区原文"] {
                    assert!(
                        !instruction.contains(marker),
                        "{locale:?} x {detail:?}: the detail wording carries structural \
                         facts only, never reply content ({marker}): {instruction}"
                    );
                }
            }
        }
    }

    #[test]
    fn repair_instruction_without_detail_rides_the_violation_level_row() {
        let registry = PromptRegistry::new();
        for locale in [Locale::Zh, Locale::En] {
            let prose = registry.render_repair(Locale::Zh, Violation::NotJson, None);
            assert!(
                prose.contains("回复不是裸（JSON）对象：NotJson——"),
                "the violation-level row carries position, rule id and action: {prose}"
            );
            let en = registry.render_repair(Locale::En, Violation::NotJson, None);
            assert!(
                en.contains("not a bare JSON object: NotJson — "),
                "the violation-level row carries position, rule id and action: {en}"
            );
            for violation in [Violation::EmptyReply, Violation::KindNotAllowed] {
                let instruction = registry.render_repair(locale, violation, None);
                assert!(
                    instruction.contains(violation.key()),
                    "{locale:?} x {violation:?}: every violation maps into the instruction: \
                     {instruction}"
                );
                assert!(
                    !instruction.contains("{{"),
                    "{locale:?} x {violation:?}: no leftover placeholders: {instruction}"
                );
            }
        }
        let zh = registry.render_repair(Locale::Zh, Violation::KindNotAllowed, None);
        assert!(
            !zh.contains("{\"kind\"") && !zh.contains("{\"note\""),
            "the repair instruction no longer quotes a contract schema line: {zh}"
        );
    }

    #[test]
    fn repair_row_falls_back_to_the_display_wording_when_the_registry_is_missing() {
        let violation = Violation::NotJson;
        assert_eq!(
            repair_row(None, None, violation),
            violation.to_string(),
            "without the frame the row degrades to the fixed violation wording"
        );
        let frame = crate::wordings::frame(Locale::Zh).expect("the embedded registry parses");
        assert_eq!(
            repair_row(Some(frame), None, violation),
            violation.to_string(),
            "a missing wording key degrades to the fixed violation wording"
        );
    }

    #[test]
    fn language_repair_instruction_carries_the_row_list_only() {
        use crate::lang_gate::{LangField, LangViolation};

        let registry = PromptRegistry::new();
        let violations = [
            LangViolation::new(LangField::Note, LangCategory::MissingSpace),
            LangViolation::new(LangField::Example(1), LangCategory::ForeignRun),
        ];
        for locale in [Locale::Zh, Locale::En] {
            let instruction = registry.render_language_repair(locale, &violations);
            assert!(
                !instruction.contains("{{"),
                "{locale:?} leaves a placeholder: {instruction}"
            );
            assert!(
                instruction.contains("ForeignRun") && instruction.contains("MissingSpace"),
                "every rule id rides the list: {instruction}"
            );
            assert!(
                !instruction.contains("中文word"),
                "the instruction carries positions and rule ids, never reply text: {instruction}"
            );
            assert!(
                instruction.to_lowercase().contains("json"),
                "the language repair prompt must mention json (the endpoint's JSON-mode gate \
                 reads it): {instruction}"
            );
        }
        let zh = registry.render_language_repair(Locale::Zh, &violations);
        assert!(
            zh.starts_with("你在上一条回复中的输出即反例。"),
            "the back reference names the previous reply's identity only: {zh}"
        );
        assert!(
            zh.contains("note：MissingSpace——"),
            "the row format is field position, rule id, action: {zh}"
        );
        assert!(
            zh.contains("examples 第 2 条：ForeignRun——"),
            "the row format is field position, rule id, action: {zh}"
        );
        let en = registry.render_language_repair(Locale::En, &violations);
        assert!(
            en.starts_with("The output in your previous reply is the counterexample."),
            "the back reference names the previous reply's identity only: {en}"
        );
        assert!(en.contains("examples item 2: ForeignRun — "), "{en}");
    }

    #[test]
    fn language_repair_aggregates_same_field_and_category_and_sorts_by_field_order() {
        use crate::lang_gate::{LangField, LangViolation};

        let registry = PromptRegistry::new();
        let violations = [
            LangViolation::new(LangField::Example(4), LangCategory::ForeignRun),
            LangViolation::new(LangField::Note, LangCategory::MissingSpace),
            LangViolation::new(LangField::Example(1), LangCategory::ForeignRun),
            LangViolation::new(LangField::Phonetic, LangCategory::PhoneticCharset),
            LangViolation::new(LangField::Example(6), LangCategory::ForeignRun),
            LangViolation::new(LangField::Note, LangCategory::ForeignRun),
        ];
        let zh = registry.render_language_repair(Locale::Zh, &violations);
        let examples_row = zh
            .find("examples 第 2、5、7 条：ForeignRun")
            .unwrap_or_else(|| {
                panic!(
                    "same-field same-category rows merge with sorted \
                                       indices: {zh}"
                )
            });
        let phonetic_row = zh
            .find("phonetic：PhoneticCharset")
            .unwrap_or_else(|| panic!("every rule id survives the aggregation: {zh}"));
        let note_foreign_row = zh
            .find("note：ForeignRun——")
            .unwrap_or_else(|| panic!("the note field keeps its own row: {zh}"));
        let note_missing_row = zh
            .find("note：MissingSpace——")
            .unwrap_or_else(|| panic!("the note field keeps its own row: {zh}"));
        assert!(
            note_foreign_row < phonetic_row
                && phonetic_row < examples_row
                && note_missing_row < phonetic_row,
            "rows follow the schema field order (note, phonetic, examples): {zh}"
        );
        let en = registry.render_language_repair(Locale::En, &violations);
        assert!(
            en.contains("examples items 2, 5, 7: ForeignRun — "),
            "merged indices ride the localized position: {en}"
        );
    }

    #[test]
    fn language_repair_caps_rows_and_appends_the_truncated_note() {
        use crate::lang_gate::{LangField, LangViolation};

        let registry = PromptRegistry::new();
        let mut over: Vec<LangViolation> = ALL_LANG_CATEGORIES
            .map(|category| LangViolation::new(LangField::Note, category))
            .into_iter()
            .collect();
        over.push(LangViolation::new(
            LangField::Phonetic,
            LangCategory::PhoneticCharset,
        ));
        over.push(LangViolation::new(
            LangField::Example(0),
            LangCategory::ForeignRun,
        ));
        let zh = registry.render_language_repair(Locale::Zh, &over);
        let rows = zh
            .lines()
            .skip_while(|line| !line.contains("违约清单如下"))
            .skip(1)
            .take_while(|line| !line.contains("请逐条修正后") && !line.contains("其余为同类违约"))
            .count();
        assert_eq!(
            rows,
            super::LANG_REPAIR_ROW_CAP,
            "the list caps at the budget after aggregation: {zh}"
        );
        assert!(
            zh.contains("其余为同类违约，一并修正。"),
            "the truncated note rides the capped list: {zh}"
        );
        assert!(
            !zh.contains("examples 第 1 条：ForeignRun"),
            "the capped-off group is dropped, not silently overflowed: {zh}"
        );
    }

    #[test]
    fn language_repair_within_the_cap_carries_no_truncated_note() {
        use crate::lang_gate::{LangField, LangViolation};

        let registry = PromptRegistry::new();
        let mut within: Vec<LangViolation> = ALL_LANG_CATEGORIES
            .map(|category| LangViolation::new(LangField::Note, category))
            .into_iter()
            .collect();
        within.push(LangViolation::new(
            LangField::Phonetic,
            LangCategory::PhoneticCharset,
        ));
        let zh = registry.render_language_repair(Locale::Zh, &within);
        assert!(
            !zh.contains("其余为同类违约"),
            "a list within the cap carries no truncated note: {zh}"
        );
    }

    #[test]
    fn language_repair_degrades_to_position_and_rule_id_lines_without_the_registry() {
        use crate::lang_gate::{LangField, LangViolation};

        let violations = [
            LangViolation::new(LangField::Example(0), LangCategory::ForeignRun),
            LangViolation::new(LangField::Note, LangCategory::MissingSpace),
        ];
        let (rows, truncated) = super::language_repair_rows(Locale::Zh, None, &violations);
        assert!(!truncated);
        assert_eq!(
            rows,
            vec![
                "note: MissingSpace".to_owned(),
                "examples[0]: ForeignRun".to_owned(),
            ],
            "without the frame each group degrades to the position and rule id line"
        );
    }

    fn brief_positions(locale: Locale) -> super::ViolationPositions {
        match locale {
            Locale::Zh => super::ViolationPositions {
                note: "注".into(),
                phonetic: "音标".into(),
                example: "疏·例句 {{n}}".into(),
            },
            Locale::En => super::ViolationPositions {
                note: "NOTE".into(),
                phonetic: "Phonetics".into(),
                example: "EXP item {{n}}".into(),
            },
        }
    }

    #[test]
    fn violation_briefs_render_human_position_and_rule_names_on_the_shared_row() {
        use crate::lang_gate::{LangField, LangViolation};

        let violations = [
            LangViolation::new(LangField::Note, LangCategory::MissingSpace),
            LangViolation::new(LangField::Example(1), LangCategory::ForeignRun),
            LangViolation::new(LangField::Example(4), LangCategory::ForeignRun),
        ];
        let zh = super::violation_briefs(Locale::Zh, &brief_positions(Locale::Zh), &violations, 12);
        assert_eq!(
            zh.rows.len(),
            2,
            "same-field same-category violations merge into one row: {:?}",
            zh.rows
        );
        assert!(
            zh.rows[0].starts_with("注：缺空格——"),
            "the row keeps the three-part shape with human words: {:?}",
            zh.rows
        );
        assert!(
            zh.rows[0].contains("在中文与西文、数字之间补一个半角空格"),
            "the action rides the registry wording verbatim: {:?}",
            zh.rows
        );
        assert!(
            zh.rows[1].starts_with("疏·例句 2、5：夹外文——"),
            "the example position fills the placeholder with the merged indices: {:?}",
            zh.rows
        );
        assert!(
            zh.rows[1].contains("将外文词移入括号并补目标语译名"),
            "the action rides the registry wording verbatim: {:?}",
            zh.rows
        );
        assert!(!zh.truncated);

        let en = super::violation_briefs(Locale::En, &brief_positions(Locale::En), &violations, 12);
        assert!(
            en.rows[0].starts_with("NOTE: spacing — "),
            "the human rule name rides the en registry: {:?}",
            en.rows
        );
        assert!(
            en.rows[1].starts_with("EXP item 2, 5: foreign text — "),
            "merged indices ride the localized position: {:?}",
            en.rows
        );
    }

    #[test]
    fn violation_briefs_carry_structural_facts_only_without_leftover_placeholders() {
        use crate::lang_gate::{LangField, LangViolation};

        let violations = [
            LangViolation::new(LangField::Note, LangCategory::ForeignRun),
            LangViolation::new(LangField::Phonetic, LangCategory::PhoneticCharset),
            LangViolation::new(LangField::Example(0), LangCategory::TraditionalChar),
        ];
        for locale in [Locale::Zh, Locale::En] {
            let briefs = super::violation_briefs(locale, &brief_positions(locale), &violations, 12);
            assert_eq!(briefs.rows.len(), 3, "{locale:?}");
            for row in &briefs.rows {
                assert!(
                    !row.contains("{{"),
                    "{locale:?}: no placeholder survives the render: {row}"
                );
                for marker in ["MARKER", "中文word", "{\"note\""] {
                    assert!(
                        !row.contains(marker),
                        "{locale:?}: the brief carries positions, rule names and actions \
                         only, never reply content ({marker}): {row}"
                    );
                }
            }
        }
    }

    #[test]
    fn violation_briefs_cap_rows_and_append_the_truncated_note() {
        use crate::lang_gate::{LangField, LangViolation};

        let mut violations: Vec<LangViolation> = ALL_LANG_CATEGORIES
            .map(|category| LangViolation::new(LangField::Note, category))
            .into_iter()
            .collect();
        violations.push(LangViolation::new(
            LangField::Phonetic,
            LangCategory::PhoneticCharset,
        ));
        let zh = super::violation_briefs(Locale::Zh, &brief_positions(Locale::Zh), &violations, 5);
        assert_eq!(
            zh.rows.len(),
            6,
            "the cap holds five rows and the truncated note rides as the last row: {:?}",
            zh.rows
        );
        assert!(zh.truncated);
        assert_eq!(
            zh.rows[5], "其余为同类违约，一并修正。",
            "the truncated note is the registry wording, not a ui copy: {:?}",
            zh.rows
        );
        assert!(
            !zh.rows.join("\n").contains("记号未闭合"),
            "the capped-off group is dropped, not silently overflowed: {:?}",
            zh.rows
        );

        let within =
            super::violation_briefs(Locale::Zh, &brief_positions(Locale::Zh), &violations, 12);
        assert_eq!(
            within.rows.len(),
            12,
            "a list within the cap carries every row without the note: {:?}",
            within.rows
        );
        assert!(!within.truncated);
    }

    #[test]
    fn violation_briefs_of_an_empty_list_stay_empty_without_a_truncated_note() {
        let briefs = super::violation_briefs(Locale::Zh, &brief_positions(Locale::Zh), &[], 5);
        assert!(briefs.rows.is_empty());
        assert!(!briefs.truncated);
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
    fn task_templates_avoid_forbidden_legacy_markers() {
        let zh_markers = [
            "经注疏",
            "注—疏",
            "为体例",
            "注不破经",
            "训释",
            "书证",
            "按断",
            "按语",
            "典源",
            "故实",
            "互文",
            "义训",
        ];
        let en_markers = [
            "commentary—subcommentary",
            "zhu—shu",
            "classical chinese",
            "commentary",
            "subcommentary",
            "does not violate the classic",
            "follow the traditional chinese",
            "textual evidence",
            "sources of allusions",
            "historical references",
            "cross-references",
            "critical judgments",
        ];
        for locale in [Locale::Zh, Locale::En] {
            let templates = locale.templates();
            for (name, text) in [
                ("word_card.md", templates.word_card),
                ("sentence.md", templates.sentence),
                ("code.md", templates.code),
                ("image_explain.md", templates.image_explain),
                ("general.md", templates.general),
            ] {
                let markers = match locale {
                    Locale::Zh => &zh_markers[..],
                    Locale::En => &en_markers[..],
                };
                for marker in markers {
                    assert!(
                        !text.to_lowercase().contains(marker),
                        "{locale:?}/{name}: the legacy marker {marker:?} must not return"
                    );
                }
            }
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
        task.options_mut().target_lang = Some(Lang::Ja);
        let messages = registry.render(&task).expect("render");
        assert!(text_of(&messages[0].content).contains("日语"));
    }

    #[test]
    fn empty_target_language_name_falls_back_to_default() {
        let registry = PromptRegistry::new();
        for locale in [Locale::Zh, Locale::En] {
            for name in ["", " "] {
                let mut task = text_task(TaskKind::TranslateSentence, "hello");
                task.options_mut().target_lang = Some(Lang::Other(name.into()));
                task.options_mut().prompt_locale = Some(locale);
                let messages = registry.render(&task).expect("render");
                let expected = lang_display(&DEFAULT_TARGET, locale);
                let instruction = match locale {
                    Locale::Zh => "翻译专家",
                    Locale::En => "translation expert",
                };
                assert!(
                    text_of(&messages[0].content).contains(instruction),
                    "{locale:?}/{name:?}: a blank target name must not swallow the instruction \
                     line: {}",
                    text_of(&messages[0].content)
                );
                assert!(text_of(&messages[0].content).contains(expected.as_ref()));
                assert!(text_of(&messages[0].content).contains("\"note\""));
            }
        }
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
    fn image_explain_example_is_a_single_json_object_with_note_first() {
        for locale in [Locale::Zh, Locale::En] {
            let template = locale.templates().image_explain;
            let object = template
                .lines()
                .find(|line| line.trim_start().starts_with('{'))
                .unwrap_or_else(|| panic!("{locale:?}: example must be a JSON object line"));
            assert!(
                object.trim_start().starts_with("{\"note\""),
                "{locale:?}: the contract fixes note as the first field: {object}"
            );
            let value: serde_json::Value = serde_json::from_str(object.trim())
                .unwrap_or_else(|err| panic!("{locale:?}: example must parse: {err}"));
            let keys = value
                .as_object()
                .expect("{locale:?}: example object")
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            assert_eq!(
                keys,
                ["note".to_owned(), "structured".to_owned()],
                "{locale:?}: the contract has exactly note and structured, in that order: {keys:?}"
            );
            let structured = value
                .get("structured")
                .and_then(|value| value.as_object())
                .expect("{locale:?}: structured object");
            assert_eq!(
                structured.keys().cloned().collect::<Vec<_>>(),
                ["examples".to_owned()],
                "{locale:?}: the plain image contract carries examples only (the old \
                 interpretation name is retired): {structured:?}"
            );
        }
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
    fn repair_conversation_replays_the_history_with_the_bad_reply() {
        use super::repair_conversation;

        let base = vec![ChatMessage::system("契约"), ChatMessage::user("gloss 原文")];
        let repaired = repair_conversation(&base, "上一轮的坏回复", "纠错指令".to_owned());
        let roles: Vec<Role> = repaired.iter().map(|m| m.role).collect();
        assert_eq!(
            roles,
            [Role::System, Role::User, Role::Assistant, Role::User],
            "[system][user 原文][assistant 坏回复][user 纠错指令]"
        );
        assert_eq!(repaired[0], base[0], "the system prompt rides verbatim");
        assert_eq!(
            repaired[1], base[1],
            "the original user turn rides verbatim"
        );
        assert_eq!(
            repaired[2],
            ChatMessage::assistant("上一轮的坏回复"),
            "the bad reply rides as history"
        );
    }

    #[test]
    fn every_template_placeholder_is_declared() {
        let declared = ["target"];
        for locale in [Locale::Zh, Locale::En] {
            let templates = locale.templates();
            let files = [
                ("word_card.md", templates.word_card),
                ("sentence.md", templates.sentence),
                ("code.md", templates.code),
                ("image_explain.md", templates.image_explain),
                ("general.md", templates.general),
                ("target_lang.md", templates.target_lang),
                ("discipline.md", templates.discipline),
                ("classify.md", templates.classify),
            ];
            for (name, text) in files {
                for (index, _) in text.match_indices("{{") {
                    assert!(
                        text[index + 2..].contains("}}"),
                        "{locale:?}/{name}: unclosed placeholder brace"
                    );
                }
                for chunk in text.split("{{").skip(1) {
                    if let Some(end) = chunk.find("}}") {
                        let key = chunk[..end].trim();
                        assert!(
                            declared.contains(&key),
                            "{locale:?}/{name}: undeclared placeholder {key:?} sits on an \
                             optional-line candidate and can be dropped without any render \
                             output showing it"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn output_example_values_are_neutral_placeholders() {
        for locale in [Locale::Zh, Locale::En] {
            let templates = locale.templates();
            for (name, text) in [
                ("word_card.md", templates.word_card),
                ("sentence.md", templates.sentence),
                ("code.md", templates.code),
                ("image_explain.md", templates.image_explain),
                ("general.md", templates.general),
            ] {
                assert!(
                    text.contains("\"<string>\""),
                    "{locale:?}/{name}: the example values must be neutral placeholders"
                );
                for proper_noun in ["serendipity", "glossary", "pangram"] {
                    assert!(
                        !text.contains(proper_noun),
                        "{locale:?}/{name}: a concrete example value biases the output: {proper_noun}"
                    );
                }
            }
        }
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
