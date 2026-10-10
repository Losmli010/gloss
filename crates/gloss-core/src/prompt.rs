//! Prompt 模板注册表：kind + input + locale → OpenAI 兼容 messages。
//!
//! 任务**一个任务一份自包含模板**（词卡/句译/代码解释/图像解读）：任务
//! 说明与纯 JSON 输出契约（含一份**中性占位**的输出示例）都在同一个文件
//! 里，改一个任务的任务书不会牵动其它任务。契约按经注疏/说文解字的层次
//! 组织：`note`（义，markdown 注文）+ 按 kind 的疏证字段——词卡是
//! phonetic/examples（音/例，字头即选区原文），句译与讲解是 examples
//! （展开讲解），代码另带 code_language（LLM 判定，UI 角标与高亮使用），
//! 图像解读是 interpretation（逐条内容解读，`note` 在前）。
//! 模板里的输出示例就是解析侧（`gloss_app::finalize`）所吃形状的唯一描述，
//! 两处改一须改二；示例值恒为占位（同分类契约的少样本偏置取舍）。没有
//! 模板的模态组合（`ImageOcr`）与预留的音频输入一律
//! [`GlossError::UnsupportedModality`]，不发出注定无效的请求。
//!
//! [`STRUCTURED_FENCE`] 是**旧契约的围栏标记**，只服务两处兼容位：分类
//! 回复的围栏容错提取（`classify::parse_classify_reply`）与完成态解析的
//! 围栏 fallback（模型跑偏输出旧契约时兜底）——现行任务 prompt 不再要求
//! 围栏。
//!
//! 模板文本是编译期嵌入的文件资源（`crates/gloss-core/prompts/{locale}/*.md`，
//! `include_str!`），渲染是显式的 `{{占位符}}` 替换（可选行语义见
//! [`render_template`]）。不注入模态提示行：源语言与代码语言都由模型
//! 从原文自行判断，代码语言经产物 JSON 的 `code_language` 回传 UI。
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

use std::sync::Arc;

use crate::model::{GlossError, Lang, Locale};
use crate::task::{Task, TaskInput, TaskKind, TaskOptions, validate_modality};

/// 目标语言缺省值（`TaskOptions::target_lang` 文档：缺省中文）。
const DEFAULT_TARGET: Lang = Lang::Zh;

/// 旧契约的结构化围栏标记：模型按旧契约把结构化 JSON 追加在正文之后的
/// ` ```gloss … ``` ` 块里。现行契约是纯 JSON 对象，本标记只剩兼容位——
/// 分类回复的容错提取与完成态的围栏 fallback 解析共用（单一事实源）。
pub const STRUCTURED_FENCE: &str = "```gloss";

/// 分类输出契约的 schema：只示范形状，值用中性占位——写死某个具体 kind
/// 会形成少样本偏置，模型照抄示例类别的比例随示例显著性上升。
pub(crate) const CLASSIFY_SCHEMA: &str = r#"{"kind":"…"}"#;

/// 一个 locale 的模板文件集：各 kind 的自包含任务书（说明 + 提示行 +
/// 输出契约与示例都在文件内）+ 分类指令。
#[derive(Debug, Clone, Copy)]
struct Templates {
    word_card: &'static str,
    sentence: &'static str,
    code: &'static str,
    image_explain: &'static str,
    classify: &'static str,
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
                classify: include_str!("../prompts/zh/classify.md"),
            },
            Locale::En => Templates {
                word_card: include_str!("../prompts/en/word_card.md"),
                sentence: include_str!("../prompts/en/sentence.md"),
                code: include_str!("../prompts/en/code.md"),
                image_explain: include_str!("../prompts/en/image_explain.md"),
                classify: include_str!("../prompts/en/classify.md"),
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
    /// 模型回复（当前模板不产生，预留给多轮对话）。
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
    /// 文本部件。
    Text(String),
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
}

/// Prompt 模板注册表：内置文本任务模板，无状态可按需构造。
///
/// 模板来自编译期嵌入的文件资源（见模块文档）；配置化（自定义模板）
/// 落地时在此扩展注册机制。
#[derive(Debug, Default, Clone, Copy)]
pub struct PromptRegistry;

impl PromptRegistry {
    /// 创建注册表。
    pub fn new() -> Self {
        Self
    }

    /// 渲染任务的完整 messages：先过模态约束表，非法组合
    /// 返回 [`GlossError::UnsupportedModality`]。模板语言取任务自带的
    /// `options.prompt_locale`（缺省中文）——与模型、目标语言一样，一次
    /// 任务只认触发时定下的那一份。系统指令是模板渲染结果；用户消息按
    /// 输入模态分派：文本任务只有原文（取材不携带模态提示，语言线索由
    /// 模型从原文判断），图像任务是图像部件（编码归传输层）。
    pub fn render(&self, task: &Task) -> Result<Vec<ChatMessage>, GlossError> {
        validate_modality(task.kind, &task.input)?;
        let locale = task.options.prompt_locale.unwrap_or_default();
        let target = target_display(&task.options, locale);
        match &task.input {
            TaskInput::Text { text, .. } => {
                let templates = locale.templates();
                let system = render_template(
                    instruction_template(templates, task.kind),
                    &[("target", &target)],
                );
                Ok(vec![
                    ChatMessage::system(system),
                    ChatMessage::user(text.to_owned()),
                ])
            }
            // 图像任务：模板只有 image_explain（ImageOcr 尚无模板，仍拒绝）。
            TaskInput::Image { png, .. } => {
                if task.kind != TaskKind::ImageExplain {
                    return Err(GlossError::UnsupportedModality);
                }
                let templates = locale.templates();
                let system = render_template(templates.image_explain, &[("target", &target)]);
                Ok(vec![
                    ChatMessage::system(system),
                    ChatMessage::user_parts(vec![ContentPart::ImagePng(Arc::clone(png))]),
                ])
            }
            // 语音是预留模态：模态矩阵已拦，这里显式收口。
            TaskInput::Audio { .. } => Err(GlossError::UnsupportedModality),
        }
    }

    /// 渲染**分类请求**的 messages：系统指令来自 classify 模板（任务说明、
    /// 允许清单、判别规则、输出契约），用户消息只有待分类原文。与任务渲染
    /// 的分工：分类不走 `render`（那是任务的路径），输出契约见
    /// [`CLASSIFY_SCHEMA`]。
    ///
    /// `allowed` 是允许模型选择的任务类型清单（编排传
    /// [`crate::classify::CLASSIFY_KINDS`]）。
    pub fn render_classify(
        &self,
        locale: Locale,
        allowed: &[TaskKind],
        text: &str,
    ) -> Vec<ChatMessage> {
        let templates = locale.templates();
        let allowed_block = classify_allowed_block(allowed, locale);
        let rules_block = classify_rules_block(allowed, locale);
        let system = render_template(
            templates.classify,
            &[
                ("allowed", &allowed_block),
                ("rules", &rules_block),
                ("schema", CLASSIFY_SCHEMA),
            ],
        );
        vec![ChatMessage::system(system), ChatMessage::user(text)]
    }
}

/// 允许清单的渲染块：每个 kind 一行「serde 标识 — 一句话判据」。标识是
/// 模型要原样输出的契约值，判据给它选择的依据；语言随 locale。
fn classify_allowed_block(allowed: &[TaskKind], locale: Locale) -> String {
    let mut lines = Vec::new();
    for kind in allowed {
        let description = match (kind, locale) {
            (TaskKind::TranslateWord, Locale::Zh) => {
                "单个词或短语，适合词典式查询（音标、释义、例句）"
            }
            (TaskKind::TranslateSentence, Locale::Zh) => {
                "自然语言句子或段落（可夹杂术语），需要翻译成目标语言"
            }
            (TaskKind::ExplainCode, Locale::Zh) => {
                "代码片段（含命令行、报错堆栈、配置与数据格式片段），需要解释其行为或原理"
            }
            (TaskKind::ImageOcr, Locale::Zh) => "图片，需要提取其中文字",
            (TaskKind::ImageExplain, Locale::Zh) => "图片，需要解释其内容",
            (TaskKind::TranslateWord, Locale::En) => {
                "a single word or phrase suited to a dictionary-style card"
            }
            (TaskKind::TranslateSentence, Locale::En) => {
                "a natural-language sentence or paragraph (terms mixed in are fine) to translate"
            }
            (TaskKind::ExplainCode, Locale::En) => {
                "a code snippet (including command lines, error stack traces, and config or \
                 data-format fragments) to explain"
            }
            (TaskKind::ImageOcr, Locale::En) => "an image to extract text from",
            (TaskKind::ImageExplain, Locale::En) => "an image to explain",
        };
        lines.push(format!("{kind:?} — {description}"));
    }
    lines.join("\n")
}

/// 易混淆簇的判别规则段（含段首标签，作为**一个可选单元**渲染）：只补
/// 「清单判据一句话说不清」的边界（命令、报错、配置片段没有代码围栏也属
/// 代码解释），不引入少样本示例——示例会把输出偏置到它自己的类别。规则
/// 只对**清单里存在**的 kind 出现：规则指向一个不在清单里的答案，等于
/// 推着模型选一个必然被拒绝的项；清单里一条规则都没有时整段为空，模板
/// 那一行按可选行语义整行剔除（标签随之消失）。
fn classify_rules_block(allowed: &[TaskKind], locale: Locale) -> String {
    let mut lines = Vec::new();
    for kind in allowed {
        let rule = match (kind, locale) {
            (TaskKind::ExplainCode, Locale::Zh) => Some(
                "命令行、报错堆栈、配置与数据格式片段，即使没有代码围栏或缩进，也按代码解释处理。",
            ),
            (TaskKind::TranslateSentence, Locale::Zh) => {
                Some("完整的自然语言句子或段落（即使夹杂术语），按句子翻译处理。")
            }
            (TaskKind::ExplainCode, Locale::En) => Some(
                "Command lines, error stack traces, and config or data-format fragments count \
                 as code to explain even without fences or indentation.",
            ),
            (TaskKind::TranslateSentence, Locale::En) => Some(
                "A full natural-language sentence or paragraph counts as a translation task \
                 even when terms are mixed in.",
            ),
            _ => None,
        };
        if let Some(rule) = rule {
            lines.push(format!("- {rule}"));
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    let label = match locale {
        Locale::Zh => "判别规则（逐条遵守）：",
        Locale::En => "Discrimination rules (follow each one):",
    };
    format!("{label}\n{}", lines.join("\n"))
}

/// 指令模板：按 kind 取本 locale 对应文件。
fn instruction_template(templates: Templates, kind: TaskKind) -> &'static str {
    match kind {
        TaskKind::TranslateWord => templates.word_card,
        TaskKind::TranslateSentence => templates.sentence,
        TaskKind::ExplainCode => templates.code,
        // 图像 kind 走不到这里：render 只对文本输入调本函数（图像分支
        // 直接取 image_explain），ImageOcr 无模板已被拒。
        TaskKind::ImageOcr | TaskKind::ImageExplain => "",
    }
}

/// 目标语言显示名：缺省中文，且**显示名不得为空**——指令模板里 `{{target}}`
/// 独占一行语义、周围全是静态文字，空值会让整条指令被当作可选行删掉，
/// 只剩契约。空名或纯空白名（`Lang::Other` 由配置文件手填得来，可能带空白）
/// 回落缺省。
///
/// 语言名随 prompt locale——英文模板里写 "Chinese" 而不是「中文」，否则
/// 模型拿到的是半汉半英的指令。
fn target_display(options: &TaskOptions, locale: Locale) -> String {
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
fn lang_display(lang: &Lang, locale: Locale) -> String {
    match locale {
        Locale::Zh => match lang {
            Lang::Zh => "中文".into(),
            Lang::En => "英语".into(),
            Lang::Ja => "日语".into(),
            Lang::Ko => "韩语".into(),
            Lang::Fr => "法语".into(),
            Lang::Other(name) => name.clone(),
        },
        Locale::En => match lang {
            Lang::Zh => "Chinese".into(),
            Lang::En => "English".into(),
            Lang::Ja => "Japanese".into(),
            Lang::Ko => "Korean".into(),
            Lang::Fr => "French".into(),
            Lang::Other(name) => name.clone(),
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
        Task {
            kind,
            input: TaskInput::Text { text: text.into() },
            options: TaskOptions::default(),
        }
    }

    fn localized_task(kind: TaskKind, text: &str, locale: Locale) -> Task {
        let mut task = text_task(kind, text);
        task.options.prompt_locale = Some(locale);
        task
    }

    fn image_task(kind: TaskKind) -> Task {
        Task {
            kind,
            input: TaskInput::Image {
                png: Arc::from(&b"png"[..]),
                region: None,
            },
            options: TaskOptions::default(),
        }
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
            (TaskKind::TranslateWord, "词典式词卡"),
            (TaskKind::TranslateSentence, "翻译"),
            (TaskKind::ExplainCode, "代码"),
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
        assert!(text_of(&word[0].content).contains("\"phonetic\""));
        assert!(text_of(&word[0].content).contains("\"examples\""));
        assert!(text_of(&word[0].content).contains("\"note\""));

        let plain = registry
            .render(&text_task(TaskKind::ExplainCode, "fn main() {}"))
            .expect("render");
        assert!(text_of(&plain[0].content).contains("\"examples\""));
        assert!(text_of(&plain[0].content).contains("\"code_language\""));
        assert!(text_of(&plain[0].content).contains("\"note\""));

        let sentence = registry
            .render(&text_task(TaskKind::TranslateSentence, "hello"))
            .expect("render");
        assert!(text_of(&sentence[0].content).contains("\"examples\""));
        assert!(text_of(&sentence[0].content).contains("\"note\""));
    }

    #[test]
    fn output_example_carries_the_note_field() {
        fn example_object(template: &str) -> &str {
            template
                .lines()
                .find(|line| line.trim_start().starts_with('{'))
                .expect("the output example must be a JSON object line")
        }

        for locale in [Locale::Zh, Locale::En] {
            let templates = locale.templates();
            for (name, text) in [
                ("word_card.md", templates.word_card),
                ("sentence.md", templates.sentence),
                ("code.md", templates.code),
                ("image_explain.md", templates.image_explain),
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
        task.options.target_lang = Some(Lang::Ja);
        let messages = registry.render(&task).expect("render");
        assert!(text_of(&messages[0].content).contains("日语"));
    }

    #[test]
    fn empty_target_language_name_falls_back_to_default() {
        let registry = PromptRegistry::new();
        for locale in [Locale::Zh, Locale::En] {
            for name in ["", " "] {
                let mut task = text_task(TaskKind::TranslateSentence, "hello");
                task.options.target_lang = Some(Lang::Other(name.into()));
                task.options.prompt_locale = Some(locale);
                let messages = registry.render(&task).expect("render");
                let expected = lang_display(&DEFAULT_TARGET, locale);
                let instruction = match locale {
                    Locale::Zh => "翻译助手",
                    Locale::En => "translation assistant",
                };
                assert!(
                    text_of(&messages[0].content).contains(instruction),
                    "{locale:?}/{name:?}: a blank target name must not swallow the instruction \
                     line: {}",
                    text_of(&messages[0].content)
                );
                assert!(text_of(&messages[0].content).contains(&expected));
                assert!(text_of(&messages[0].content).contains("\"note\""));
            }
        }
    }

    #[test]
    fn image_explain_renders_system_and_image_parts() {
        let registry = PromptRegistry::new();
        let png: Arc<[u8]> = Arc::from(&b"png"[..]);
        let task = Task {
            kind: TaskKind::ImageExplain,
            input: TaskInput::Image {
                png: Arc::clone(&png),
                region: None,
            },
            options: TaskOptions::default(),
        };
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
            system.contains("JSON 对象") || system.contains("JSON object"),
            "the image contract must demand a single JSON object: {system}"
        );
        assert!(system.contains("\"note\""), "{system}");
        assert!(system.contains("\"interpretation\""), "{system}");
        assert!(!system.contains("{{"), "{system}");
    }

    #[test]
    fn image_ocr_still_has_no_template() {
        let registry = PromptRegistry::new();
        assert_eq!(
            registry.render(&image_task(TaskKind::ImageOcr)),
            Err(GlossError::UnsupportedModality),
            "the OCR template lands with its own vision task"
        );
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
            assert!(
                object.find("interpretation").is_some_and(|note| note > 0),
                "{locale:?}: the interpretation field must follow note: {object}"
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
                keys.len(),
                2,
                "{locale:?}: the contract has exactly note and interpretation: {keys:?}"
            );
            assert!(
                keys.contains(&"note".to_owned()) && keys.contains(&"interpretation".to_owned()),
                "{locale:?}: the contract has exactly note and interpretation: {keys:?}"
            );
        }
    }

    #[test]
    fn modality_mismatch_is_rejected_before_rendering() {
        let registry = PromptRegistry::new();
        let task = text_task(TaskKind::ImageOcr, "not an image");
        assert_eq!(registry.render(&task), Err(GlossError::UnsupportedModality));
    }

    #[test]
    fn classify_prompt_carries_allowed_kinds_and_the_text() {
        let registry = PromptRegistry::new();
        let allowed = [TaskKind::TranslateWord, TaskKind::ExplainCode];
        for locale in [Locale::Zh, Locale::En] {
            let messages = registry.render_classify(locale, &allowed, "gloss 原文");
            assert_eq!(messages.len(), 2);
            assert_eq!(
                messages[1].content,
                MessageContent::Text("gloss 原文".into())
            );
            assert!(
                text_of(&messages[0].content).contains("TranslateWord"),
                "{locale:?}"
            );
            assert!(text_of(&messages[0].content).contains("ExplainCode"));
            assert!(text_of(&messages[0].content).contains("\"kind\""));
            assert!(!text_of(&messages[0].content).contains("{{"));
        }
    }

    #[test]
    fn classify_schema_is_a_neutral_placeholder() {
        for kind in [
            TaskKind::TranslateWord,
            TaskKind::TranslateSentence,
            TaskKind::ExplainCode,
        ] {
            let serde_name = format!("{kind:?}");
            assert!(
                !CLASSIFY_SCHEMA.contains(&serde_name),
                "the classify contract must not name a concrete kind ({serde_name}): \
                 the example is a few-shot bias"
            );
        }
    }

    #[test]
    fn rules_follow_the_allowed_list_and_leave_no_dangling_label() {
        let registry = PromptRegistry::new();
        let full = [TaskKind::TranslateSentence, TaskKind::ExplainCode];

        let zh_messages = registry.render_classify(Locale::Zh, &full, "kubectl get pods");
        let zh = text_of(&zh_messages[0].content);

        assert!(zh.contains("命令行"));
        assert!(zh.contains("判别规则"));

        let en_messages = registry.render_classify(Locale::En, &full, "kubectl get pods");
        let en = text_of(&en_messages[0].content);

        assert!(en.contains("Command lines"));
        assert!(en.contains("Discrimination rules"));

        let without_code = [TaskKind::TranslateSentence];
        let zh_messages = registry.render_classify(Locale::Zh, &without_code, "kubectl get pods");
        let zh = text_of(&zh_messages[0].content);

        assert!(!zh.contains("命令行"));
        assert!(zh.contains("判别规则"), "{zh}");

        let without_rules = [TaskKind::TranslateWord];
        let zh_messages = registry.render_classify(Locale::Zh, &without_rules, "光泽");
        let zh = text_of(&zh_messages[0].content);

        assert!(!zh.contains("命令行"));
        assert!(!zh.contains("判别规则"), "{zh}");
        assert!(!zh.contains("{{"), "{zh}");
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
    fn every_template_placeholder_is_declared() {
        let declared = ["target", "hint"];
        for locale in [Locale::Zh, Locale::En] {
            let templates = locale.templates();
            let files = [
                ("word_card.md", templates.word_card),
                ("sentence.md", templates.sentence),
                ("code.md", templates.code),
                ("image_explain.md", templates.image_explain),
            ];
            for (name, text) in files {
                assert_eq!(
                    text.matches("{{").count(),
                    text.matches("}}").count(),
                    "{locale:?}/{name}: unbalanced placeholder braces"
                );
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
        // 同分类契约的少样本偏置取舍：示例值必须是占位而不是真实词条，
        // 模型照抄示例类别的比例随示例显著性上升。
        for locale in [Locale::Zh, Locale::En] {
            let templates = locale.templates();
            for (name, text) in [
                ("word_card.md", templates.word_card),
                ("sentence.md", templates.sentence),
                ("code.md", templates.code),
                ("image_explain.md", templates.image_explain),
            ] {
                assert!(
                    text.contains("\"…\""),
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
        ];
        for locale in [Locale::Zh, Locale::En] {
            for kind in kinds {
                let mut task = match kind {
                    TaskKind::ImageExplain => image_task(kind),
                    _ => text_task(kind, "gloss"),
                };
                task.options.prompt_locale = Some(locale);
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
                    !text_of(&messages[0].content).contains(STRUCTURED_FENCE),
                    "{locale:?} x {kind:?}: the fence is legacy-fallback only and must not \
                     appear in the current prompt"
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
        assert!(text_of(&messages[0].content).contains("translation assistant"));
        assert!(text_of(&messages[0].content).contains("JSON object"));
        assert!(text_of(&messages[1].content).contains("bonjour"));
        assert!(!text_of(&messages[0].content).contains("翻译助手"));
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
        assert!(text_of(&english_prompt[0].content).contains("translation assistant"));
        assert!(
            text_of(&english_prompt[0].content).contains("Chinese"),
            "template language must not move the default target: {}",
            text_of(&english_prompt[0].content)
        );

        let mut chinese_prompt = localized_task(TaskKind::TranslateSentence, "hello", Locale::Zh);
        chinese_prompt.options.target_lang = Some(Lang::En);
        let messages = registry.render(&chinese_prompt).expect("render");
        assert!(text_of(&messages[0].content).contains("翻译助手"));
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
        assert!(text_of(&defaulted[0].content).contains("词典助手"));
        assert_eq!(Locale::default(), Locale::Zh);
    }
}
