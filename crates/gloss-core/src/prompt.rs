//! Prompt 模板注册表：kind + input + locale → OpenAI 兼容 messages。
//!
//! 任务**一个任务一份自包含模板**（词卡/句译/代码解释/图像解读/通用）：
//! 任务说明与纯 JSON 输出契约（含一份**中性占位**的输出示例）都在同一个
//! 文件里，改一个任务的任务书不会牵动其它任务。契约按经注疏/说文解字的
//! 层次组织，与 [`crate::schema`] 的三行同构表同源：`note`（义）恒为首
//! 字段，按 kind 的疏证字段收敛在 `structured` 对象里——词卡
//! 是 structured.phonetic/examples（音/例，字头即选区原文），句译、讲解、
//! 图像解读与通用任务是 structured.examples（展开条目），代码另带
//! structured.code_language（LLM 判定，UI 角标与高亮使用）。
//! 模板里的输出示例就是解析侧（`crate::schema::validate_task`）所吃形状
//! 的唯一描述，两处改一须改二（完整性门禁测试锁定：每个模板的示例对象
//! 必须通过对应 `validate_task`）。模板与提示都不要求围栏——模型回复按
//! 严格裸 JSON 校验（`crate::schema`），围栏输出即违约进修复轮。模态矩阵
//! 之外的输入一律 [`GlossError::UnsupportedModality`]，不发出注定无效的请求。
//!
//! 分类与修复的指令同样自包含：classify 模板自带类别清单与判别规则（类别
//! `Fallback` 由 [`crate::schema::validate_classify`] 映射兜底 kind），
//! repair 模板只携带违约固定措辞（`{{reason}}` 回灌）。模板文本是编译期
//! 嵌入的文件资源（`crates/gloss-core/prompts/{locale}/*.md`，`include_str!`），
//! 渲染是显式的 `{{占位符}}` 替换（可选行语义见 [`render_template`]）。
//! 不注入模态提示行：源语言与代码语言都由模型从原文自行判断，代码语言经
//! 产物 JSON 的 `structured.code_language` 回传 UI。
//!
//! 本模块只产**域形态**的消息（[`ChatMessage`]：role + [`MessageContent`]），
//! 不做 wire 序列化，也不携带 base64——把部件数组映成 OpenAI 兼容
//! content、把图像字节编码成 data URL 都归传输层（`engine::llm` 的请求体
//! 组装）。
//!
//! 模板内容面向模型，用各 locale 的语言书写，不受「日志一律英文」门禁约束
//! （`just constraints` 只查日志宏实参）。
//!
//! 参数缺省：`options.target_lang` 缺省中文；`options.prompt_locale`
//! 缺省中文模板。

use std::borrow::Cow;
use std::sync::Arc;

use crate::model::{GlossError, Lang, Locale};
use crate::task::{Task, TaskInput, TaskKind, TaskOptions};

/// 目标语言缺省值（`TaskOptions::target_lang` 文档：缺省中文）。
const DEFAULT_TARGET: Lang = Lang::Zh;

/// 一个 locale 的模板文件集：各 kind 的自包含任务书（说明 + 输出契约与
/// 示例都在文件内）+ 分类指令 + 修复轮纠错指令。
#[derive(Debug, Clone, Copy)]
struct Templates {
    word_card: &'static str,
    sentence: &'static str,
    code: &'static str,
    image_explain: &'static str,
    general: &'static str,
    classify: &'static str,
    repair: &'static str,
}

impl Locale {
    /// 本 locale 的模板文件集（编译期嵌入，无 IO）。
    fn templates(self) -> Templates {
        match self {
            Locale::Zh => Templates {
                word_card: include_str!("../prompts/zh/word_card.md"),
                sentence: include_str!("../prompts/zh/sentence.md"),
                code: include_str!("../prompts/zh/code.md"),
                image_explain: include_str!("../prompts/zh/image_explain.md"),
                general: include_str!("../prompts/zh/general.md"),
                classify: include_str!("../prompts/zh/classify.md"),
                repair: include_str!("../prompts/zh/repair.md"),
            },
            Locale::En => Templates {
                word_card: include_str!("../prompts/en/word_card.md"),
                sentence: include_str!("../prompts/en/sentence.md"),
                code: include_str!("../prompts/en/code.md"),
                image_explain: include_str!("../prompts/en/image_explain.md"),
                general: include_str!("../prompts/en/general.md"),
                classify: include_str!("../prompts/en/classify.md"),
                repair: include_str!("../prompts/en/repair.md"),
            },
        }
    }
}

/// OpenAI 兼容消息角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// 系统指令。
    System,
    /// 用户输入。
    User,
    /// 模型回复（修复轮带回上一次的坏回复）。
    Assistant,
}

/// 消息正文的域形态：纯文本，或多模态部件数组。**不携带 base64**——
/// 部件数组映成 OpenAI 兼容 content、图像字节编码成 data URL 都归传输层
/// （`engine::llm` 的请求体组装），本枚举只搬原始载荷。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageContent {
    /// 纯文本正文。
    Text(String),
    /// 多模态部件数组（图像任务的用户消息）。
    Parts(Vec<ContentPart>),
}

/// 多模态内容部件：域形态只携带原始载荷（PNG 保持字节，不预编码）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentPart {
    /// PNG 图像字节。
    ImagePng(Arc<[u8]>),
}

/// OpenAI 兼容消息的域形态：`role` + `content`（wire 形状由传输层组装）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    /// 消息角色。
    pub role: Role,
    /// 消息正文。
    pub content: MessageContent,
}

impl ChatMessage {
    /// 系统指令消息（纯文本正文）。
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: MessageContent::Text(content.into()),
        }
    }

    /// 用户消息（纯文本正文）。
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: MessageContent::Text(content.into()),
        }
    }

    /// 用户消息（多模态部件数组，如图像任务）。
    pub fn user_parts(parts: Vec<ContentPart>) -> Self {
        Self {
            role: Role::User,
            content: MessageContent::Parts(parts),
        }
    }

    /// 模型回复消息（纯文本正文）：修复轮用它带回上一次的坏回复。
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: MessageContent::Text(content.into()),
        }
    }
}

/// Prompt 模板注册表：内置文本任务模板，无状态可按需构造。
///
/// 模板来自编译期嵌入的文件资源（见模块文档）。
#[derive(Debug, Default, Clone, Copy)]
pub struct PromptRegistry;

impl PromptRegistry {
    pub fn new() -> Self {
        Self
    }

    /// 渲染任务的完整 messages。模态约束已在 [`Task::new`] 判定，这里只按
    /// 输入形状分派。模板语言取任务自带的 `options.prompt_locale`（缺省
    /// 中文）——与模型、目标语言一样，一次任务只认触发时定下的那一份。
    /// 系统指令是模板渲染结果；用户消息按输入模态分派：文本任务只有原文
    /// （取材不携带模态提示，语言线索由模型从原文判断），图像任务是图像
    /// 部件（编码归传输层）。
    pub fn render(&self, task: &Task) -> Result<Vec<ChatMessage>, GlossError> {
        let locale = task.options().prompt_locale.unwrap_or_default();
        let target = target_display(task.options(), locale);
        match task.input() {
            TaskInput::Text { text, .. } => {
                let templates = locale.templates();
                let system = render_template(
                    instruction_template(templates, task.kind()),
                    &[("target", &target)],
                );
                Ok(vec![
                    ChatMessage::system(system),
                    ChatMessage::user(text.to_owned()),
                ])
            }
            // 图像任务：模板只有 image_explain（模态闸门在构造时已保证
            // 图像输入只配 ImageExplain），这里直接按唯一合法的图像 kind
            // 取模板。
            TaskInput::Image { png } => {
                let templates = locale.templates();
                let system = render_template(templates.image_explain, &[("target", &target)]);
                Ok(vec![
                    ChatMessage::system(system),
                    ChatMessage::user_parts(vec![ContentPart::ImagePng(Arc::clone(png))]),
                ])
            }
        }
    }

    /// 渲染**分类请求**的 messages：系统指令来自 classify 模板（类别清单、
    /// 判别规则与输出契约都在模板内——模板是类别集的唯一事实源），用户
    /// 消息只有待分类原文。与任务渲染的分工：分类不走 `render`（那是任务
    /// 的路径），输出契约见 [`crate::schema::validate_classify`]。
    pub fn render_classify(&self, locale: Locale, text: &str) -> Vec<ChatMessage> {
        let templates = locale.templates();
        vec![
            ChatMessage::system(templates.classify),
            ChatMessage::user(text),
        ]
    }

    /// 渲染**修复轮**的纠错指令（修复对话里最后一条 user 消息的正文）：
    /// 违约固定措辞（[`crate::schema::Violation`]，只记类别不携带回复原文）
    /// 作为 `{{reason}}` 回灌模板。
    pub fn render_repair(&self, locale: Locale, violation: crate::schema::Violation) -> String {
        render_template(
            locale.templates().repair,
            &[("reason", &violation.to_string())],
        )
    }
}

/// 修复轮对话：`[system][user 原文/图][assistant 上一次坏回复]
/// [user 纠错指令]`。首轮消息原样在前：图像任务的图像部件（`Arc` 字节）
/// 随 base 零拷贝重发，修复轮历史里首轮图片随完整对话走。（EmptyReply
/// 无内容可纠，不经本函数——原样重发的决策在 [`crate::repair::next_round`]。）
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

/// 指令模板：按 kind 取本 locale 对应文件（全 kind 全覆盖；`render` 只对
/// 文本输入调本函数——图像分支直接取 image_explain，非法模态组合在渲染前
/// 已被约束表拦截）。兜底 kind [`TaskKind::General`] 的落点就是 general
/// 模板——分类判 `Fallback` 时任务走这一份。
fn instruction_template(templates: Templates, kind: TaskKind) -> &'static str {
    match kind {
        TaskKind::TranslateWord => templates.word_card,
        TaskKind::TranslateSentence => templates.sentence,
        TaskKind::ExplainCode => templates.code,
        TaskKind::ImageExplain => templates.image_explain,
        TaskKind::General => templates.general,
    }
}

/// 目标语言显示名：缺省中文，且**显示名不得为空**——指令模板里 `{{target}}`
/// 独占一行语义、周围全是静态文字，空值会让整条指令被当作可选行删掉，
/// 只剩契约。空名或纯空白名（`Lang::Other` 由配置文件手填得来，可能带空白）
/// 回落缺省。
///
/// 语言名随 prompt locale——英文模板里写 "Chinese" 而不是「中文」，否则
/// 模型拿到的是半汉半英的指令。`Cow` 承载：常用语种与自填名都借自
/// 静态表或原配置，渲染不复制字符串。
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

/// 语言显示名（模板面向模型，用各 locale 的语言书写；`Lang::Other` 是
/// 用户自填的名字，原样透传）。
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

/// `{{占位符}}` 渲染：逐行替换全部已声明占位符；**一行里的占位符全部
/// 替换为空**时整行（含行内的静态文字与换行）删除——「可选行」因此在模板
/// 里就是一行，代码不必为可选片段做条件拼接（提示行就是这么写的：标签在
/// 模板里，值为空时整行消失）。
///
/// 未声明的占位符与未闭合的 `{{` 原样保留：模板打错字不会静默吞内容，
/// 由完整性测试（`just test`）抓出来。
fn render_template(template: &str, values: &[(&str, &str)]) -> String {
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

/// 单行渲染结果。
struct RenderedLine {
    /// 替换后的文本。
    text: String,
    /// 该行有已声明占位符、且它们的值全是空串（该行是可选行）。
    /// 没有已声明占位符的行恒为 `false`：不带占位符的空行是模板的结构。
    all_empty: bool,
}

/// 单行替换：声明的占位符按值替换，未声明的原样保留（连同一处未闭合的
/// `{{`，其后内容照原样跟在后面）。
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
            (TaskKind::General, "经注疏"),
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
                ("repair.md", templates.repair),
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
        for locale in [Locale::Zh, Locale::En] {
            for violation in [
                crate::schema::Violation::EmptyReply,
                crate::schema::Violation::NotJson,
                crate::schema::Violation::MissingNote,
                crate::schema::Violation::KindNotAllowed,
            ] {
                let instruction = registry.render_repair(locale, violation);
                assert!(
                    !instruction.contains("{{"),
                    "{locale:?} x {violation:?} leaves a placeholder: {instruction}"
                );
                assert!(
                    !instruction.is_empty(),
                    "{locale:?} x {violation:?}: the violation wording keeps the \
                     instruction non-empty"
                );
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
            system.contains("JSON 格式") || system.contains("JSON format"),
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
    fn repair_instruction_carries_the_violation_reason_only() {
        let registry = PromptRegistry::new();
        let violation = crate::schema::Violation::NotJson;

        let zh = registry.render_repair(Locale::Zh, violation);
        assert!(
            zh.contains("reply is not a bare JSON object"),
            "the fixed violation wording rides the instruction as the reason: {zh}"
        );
        assert!(
            !zh.contains("{{"),
            "{locale:?} renders without leftover placeholders: {zh}",
            locale = Locale::Zh
        );
        assert!(
            !zh.contains("{\"kind\"") && !zh.contains("{\"note\""),
            "the repair template no longer quotes a contract schema line: {zh}"
        );

        let en = registry.render_repair(Locale::En, crate::schema::Violation::EmptyReply);
        assert!(en.contains("empty reply"));
        assert!(!en.contains("{{"), "{en}");
        assert!(
            en.to_lowercase().contains("json"),
            "the repair prompt must mention json (the endpoint's JSON-mode gate reads it)"
        );

        let kind_not_allowed =
            registry.render_repair(Locale::Zh, crate::schema::Violation::KindNotAllowed);
        assert!(
            kind_not_allowed.contains("reply kind is not one of the classify categories"),
            "every violation wording maps into the instruction: {kind_not_allowed}"
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
        let declared = ["target", "reason"];
        for locale in [Locale::Zh, Locale::En] {
            let templates = locale.templates();
            let files = [
                ("word_card.md", templates.word_card),
                ("sentence.md", templates.sentence),
                ("code.md", templates.code),
                ("image_explain.md", templates.image_explain),
                ("general.md", templates.general),
                ("classify.md", templates.classify),
                ("repair.md", templates.repair),
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
