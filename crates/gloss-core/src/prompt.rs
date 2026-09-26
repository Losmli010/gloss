//! Prompt 模板注册表：kind + input + locale → OpenAI 兼容 messages。
//!
//! 文本任务 3 个模板（词卡/句译/代码解释）统一输出契约：markdown 正文 +
//! 末尾 ```gloss 围栏 JSON 块（按 kind 携带 [`crate::task::OutcomeStructured`]
//! 的结构化字段），供任务编排解析——正文给人读，JSON 给 UI 精排。
//! 图像/音频输入一律 [`GlossError::UnsupportedModality`]，
//! 不发出注定无效的请求。
//!
//! 模板文本是编译期嵌入的文件资源（`crates/gloss-core/prompts/{locale}/*.md`，
//! `include_str!`），渲染是显式的 `{{占位符}}` 替换（可选行语义见
//! [`render_template`]）；输出契约的 schema JSON 留在代码里，与
//! [`crate::task::OutcomeStructured`] 的解析器同源。
//!
//! 模板内容面向模型，用各 locale 的语言书写，不受「日志一律英文」门禁约束
//! （`just constraints` 只查日志宏实参）。
//!
//! 参数缺省：`options.target_lang` 缺省中文；`options.prompt_locale`
//! 缺省中文模板；`InputHint` 缺省不注入提示行。

use serde::Serialize;

use crate::model::{GlossError, Lang, Locale};
use crate::task::{InputHint, Task, TaskInput, TaskKind, TaskOptions, validate_modality};

/// 目标语言缺省值（`TaskOptions::target_lang` 文档：缺省中文）。
const DEFAULT_TARGET: Lang = Lang::Zh;

/// 结构化 JSON 块的围栏标记：正文之后模型按此契约追加结构化字段；
/// 编排侧（engine）按同一标记解析，UI 侧流式渲染按它过滤未完成的
/// 结构化块——三处共用单一事实源。
pub const STRUCTURED_FENCE: &str = "```gloss";

/// 一个 locale 的模板文件集：三个文本 kind 的指令 + 输出契约散文 + 模态
/// 提示行片段 + 分类指令。契约与提示行抽成片段而不是抄进三个指令：抄写
/// 会在改契约时漏掉其中一处。
#[derive(Debug, Clone, Copy)]
struct Templates {
    word_card: &'static str,
    sentence: &'static str,
    code: &'static str,
    contract: &'static str,
    hints: &'static str,
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
                contract: include_str!("../prompts/zh/contract.md"),
                hints: include_str!("../prompts/zh/hints.md"),
                classify: include_str!("../prompts/zh/classify.md"),
            },
            Locale::En => Templates {
                word_card: include_str!("../prompts/en/word_card.md"),
                sentence: include_str!("../prompts/en/sentence.md"),
                code: include_str!("../prompts/en/code.md"),
                contract: include_str!("../prompts/en/contract.md"),
                hints: include_str!("../prompts/en/hints.md"),
                classify: include_str!("../prompts/en/classify.md"),
            },
        }
    }
}

/// OpenAI 兼容消息角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// 系统指令。
    System,
    /// 用户输入。
    User,
    /// 模型回复（当前模板不产生，预留给多轮对话）。
    Assistant,
}

/// OpenAI 兼容消息：`{"role": ..., "content": ...}`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatMessage {
    /// 消息角色。
    pub role: Role,
    /// 消息正文（多模态 content 数组随图像任务引入）。
    pub content: String,
}

impl ChatMessage {
    fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
        }
    }

    fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }
}

/// Prompt 模板注册表：内置文本任务模板，无状态可按需构造。
///
/// 模板来自编译期嵌入的文件资源（见模块文档）；配置化（自定义模板/热键
/// 绑定）落地时在此扩展注册机制。
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
    /// 任务只认触发时定下的那一份。
    ///
    /// [`TaskKind::Auto`] 在这里被显式拒绝（[`GlossError::ClassifyRequired`]）：
    /// 模态矩阵放行它的运输，但渲染不存在「待分类」的模板——走到这里
    /// 说明编排没把分类做在前半程。
    pub fn render(&self, task: &Task) -> Result<Vec<ChatMessage>, GlossError> {
        if task.kind == TaskKind::Auto {
            return Err(GlossError::ClassifyRequired);
        }
        validate_modality(task.kind, &task.input)?;
        let TaskInput::Text { text, hint } = &task.input else {
            // 图像模板随图像任务落地；音频是预留模态，模态校验已拦，
            // 这里对图像输入显式收口。
            return Err(GlossError::UnsupportedModality);
        };
        let locale = task.options.prompt_locale.unwrap_or_default();
        let templates = locale.templates();
        let hint_lines = hint_block(templates.hints, hint.as_ref(), locale);
        let contract = render_template(
            templates.contract,
            &[
                ("fence", STRUCTURED_FENCE),
                ("schema", schema_json(task.kind)),
            ],
        );
        let target = target_display(&task.options, locale);
        let system = render_template(
            instruction_template(templates, task.kind),
            &[
                ("target", &target),
                ("hint", &hint_lines),
                ("contract", &contract),
            ],
        );
        Ok(vec![
            ChatMessage::system(system),
            ChatMessage::user(user_content(text, &hint_lines)),
        ])
    }

    /// 渲染**分类请求**的 messages：系统指令来自 classify 模板（任务说明、
    /// 允许清单、输出契约），用户消息只有待分类原文。与任务渲染的分工：
    /// 分类不走 `render`（那是 Task 的路径，Auto 在那里被拒绝），输出契约
    /// 见 [`schema_json`] 的 Auto 臂。
    ///
    /// `allowed` 是允许模型选择的任务类型清单（调用方按
    /// 「text-capable ∩ enabled」算好传入）；清单里的 [`TaskKind::Auto`]
    /// 不渲染（哨兵不是可选答案）。
    pub fn render_classify(
        &self,
        locale: Locale,
        allowed: &[TaskKind],
        text: &str,
    ) -> Vec<ChatMessage> {
        let templates = locale.templates();
        let allowed_block = classify_allowed_block(allowed, locale);
        let system = render_template(
            templates.classify,
            &[
                ("allowed", &allowed_block),
                ("schema", schema_json(TaskKind::Auto)),
            ],
        );
        vec![ChatMessage::system(system), ChatMessage::user(text)]
    }
}

/// 允许清单的渲染块：每个 kind 一行「serde 标识 — 一句话判据」。标识是
/// 模型要原样输出的契约值，判据给它选择的依据；语言随 locale。
/// [`TaskKind::Auto`] 不是可选答案，跳过。
fn classify_allowed_block(allowed: &[TaskKind], locale: Locale) -> String {
    let mut lines = Vec::new();
    for kind in allowed {
        let description = match (kind, locale) {
            (TaskKind::TranslateWord, Locale::Zh) => {
                "单个词或短语，适合词典式查询（音标、释义、例句）"
            }
            (TaskKind::TranslateSentence, Locale::Zh) => "句子或段落，需要翻译成目标语言",
            (TaskKind::ExplainCode, Locale::Zh) => "代码片段，需要解释其行为或原理",
            (TaskKind::ImageOcr, Locale::Zh) => "图片，需要提取其中文字",
            (TaskKind::ImageExplain, Locale::Zh) => "图片，需要解释其内容",
            (TaskKind::TranslateWord, Locale::En) => {
                "a single word or phrase suited to a dictionary-style card"
            }
            (TaskKind::TranslateSentence, Locale::En) => "a sentence or paragraph to translate",
            (TaskKind::ExplainCode, Locale::En) => "a code snippet to explain",
            (TaskKind::ImageOcr, Locale::En) => "an image to extract text from",
            (TaskKind::ImageExplain, Locale::En) => "an image to explain",
            (TaskKind::Auto, _) => continue,
        };
        lines.push(format!("{kind:?} — {description}"));
    }
    lines.join("\n")
}

/// 指令模板：按 kind 取本 locale 对应文件。
fn instruction_template(templates: Templates, kind: TaskKind) -> &'static str {
    match kind {
        TaskKind::TranslateWord => templates.word_card,
        TaskKind::TranslateSentence => templates.sentence,
        TaskKind::ExplainCode => templates.code,
        // 图像 kind 走不到这里：render 已把非文本输入收口。
        // Auto 同样走不到：render 在模态表之前就拒绝了它。
        TaskKind::ImageOcr | TaskKind::ImageExplain | TaskKind::Auto => "",
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

/// 提示行片段：文本任务的输入只有一个 hint（[`InputHint`] 单值），两行
/// 占位符里最多一行有值。无 hint 时两行都渲染为空并整行剔除，得到空串。
///
/// 末尾换行在这里去掉：两处注入点各自决定换行（系统指令里占位符独占一行、
/// 用户消息里另起一段），留着会让消息多出空行。
fn hint_block(hints: &str, hint: Option<&InputHint>, locale: Locale) -> String {
    let mut code_lang = String::new();
    let mut source_lang = String::new();
    match hint {
        Some(InputHint::CodeLanguage(lang)) => code_lang.clone_from(lang),
        Some(InputHint::SourceLang(lang)) => source_lang = lang_display(lang, locale),
        None => {}
    }
    let rendered = render_template(
        hints,
        &[("code_lang", &code_lang), ("source_lang", &source_lang)],
    );
    rendered.trim_end_matches('\n').to_owned()
}

/// 用户消息：提示行片段 + 输入原文（无提示行时只有原文）。
fn user_content(text: &str, hint_lines: &str) -> String {
    if hint_lines.is_empty() {
        text.to_owned()
    } else {
        format!("{hint_lines}\n\n{text}")
    }
}

/// 输出契约的字段 schema：与 [`crate::task::OutcomeStructured`] 的解析器
/// 同源，故留在代码里（见模块文档）；字段名是给解析器的契约，示例值是给
/// 模型的提示，因此不随 prompt locale 变——契约只有一份，改 schema 的人
/// 面前不会出现两份措辞。
///
/// [`TaskKind::Auto`] 臂是**分类**的输出契约（`classify` 模块按它解析）：
/// 只回一个 kind 标识，不带任何理由字段——理由会把选区内容带进模型回复，
/// 而回复会进日志面。
pub(crate) fn schema_json(kind: TaskKind) -> &'static str {
    match kind {
        TaskKind::TranslateWord => {
            r#"{"word":"词条原文","phonetic":"音标或 null","senses":[{"pos":"词性或 null","meaning":"释义","examples":["例句"]}]}"#
        }
        TaskKind::TranslateSentence | TaskKind::ExplainCode => r#"{"title":"一句话摘要或 null"}"#,
        TaskKind::ImageOcr => r#"{"text":"提取的纯文本"}"#,
        TaskKind::ImageExplain => r#"{"title":"一句话摘要或 null"}"#,
        TaskKind::Auto => r#"{"kind":"TranslateSentence"}"#,
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

    fn text_task(kind: TaskKind, text: &str, hint: Option<InputHint>) -> Task {
        Task {
            kind,
            input: TaskInput::Text {
                text: text.into(),
                hint,
            },
            options: TaskOptions::default(),
        }
    }

    fn localized_task(kind: TaskKind, text: &str, locale: Locale) -> Task {
        let mut task = text_task(kind, text, None);
        task.options.prompt_locale = Some(locale);
        task
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
            let task = text_task(kind, "hello world", None);
            let messages = registry.render(&task).expect("text task should render");
            assert_eq!(messages.len(), 2, "{kind:?}");
            assert_eq!(messages[0].role, Role::System);
            assert_eq!(messages[1].role, Role::User);
            assert_eq!(messages[1].content, "hello world");
            assert!(
                messages[0].content.contains(keyword),
                "{kind:?} system prompt should mention {keyword}: {}",
                messages[0].content
            );
            assert!(
                messages[0].content.contains(STRUCTURED_FENCE),
                "{kind:?} must carry the structured output contract"
            );
        }
    }

    #[test]
    fn structured_contract_matches_outcome_schema() {
        let registry = PromptRegistry::new();

        let word = registry
            .render(&text_task(TaskKind::TranslateWord, "gloss", None))
            .expect("render");
        assert!(word[0].content.contains("\"senses\""));
        assert!(word[0].content.contains("\"phonetic\""));

        let plain = registry
            .render(&text_task(TaskKind::ExplainCode, "fn main() {}", None))
            .expect("render");
        assert!(plain[0].content.contains("\"title\""));
    }

    #[test]
    fn missing_target_lang_defaults_to_chinese() {
        let registry = PromptRegistry::new();
        let messages = registry
            .render(&text_task(TaskKind::TranslateSentence, "hello", None))
            .expect("render");
        assert!(
            messages[0].content.contains("中文"),
            "default target: {}",
            messages[0].content
        );
    }

    #[test]
    fn explicit_target_lang_is_rendered() {
        let registry = PromptRegistry::new();
        let mut task = text_task(TaskKind::TranslateSentence, "hello", None);
        task.options.target_lang = Some(Lang::Ja);
        let messages = registry.render(&task).expect("render");
        assert!(messages[0].content.contains("日语"));
    }

    #[test]
    fn empty_target_language_name_falls_back_to_default() {
        let registry = PromptRegistry::new();
        for locale in [Locale::Zh, Locale::En] {
            for name in ["", " "] {
                let mut task = text_task(TaskKind::TranslateSentence, "hello", None);
                task.options.target_lang = Some(Lang::Other(name.into()));
                task.options.prompt_locale = Some(locale);
                let messages = registry.render(&task).expect("render");
                let expected = lang_display(&DEFAULT_TARGET, locale);
                let instruction = match locale {
                    Locale::Zh => "翻译助手",
                    Locale::En => "translation assistant",
                };
                assert!(
                    messages[0].content.contains(instruction),
                    "{locale:?}/{name:?}: a blank target name must not swallow the instruction \
                     line: {}",
                    messages[0].content
                );
                assert!(messages[0].content.contains(&expected));
                assert!(messages[0].content.contains(STRUCTURED_FENCE));
            }
        }
    }

    #[test]
    fn hint_is_injected_and_defaults_to_nothing() {
        let registry = PromptRegistry::new();
        let hinted = registry
            .render(&text_task(
                TaskKind::ExplainCode,
                "fn main() {}",
                Some(InputHint::CodeLanguage("rust".into())),
            ))
            .expect("render");
        assert!(hinted[0].content.contains("代码语言：rust"));
        assert!(hinted[1].content.contains("代码语言：rust"));
        assert!(hinted[1].content.contains("fn main() {}"));

        let plain = registry
            .render(&text_task(TaskKind::ExplainCode, "fn main() {}", None))
            .expect("render");
        assert!(!plain[0].content.contains("代码语言："));
        assert_eq!(plain[1].content, "fn main() {}");
    }

    #[test]
    fn source_lang_hint_is_injected() {
        let registry = PromptRegistry::new();
        let messages = registry
            .render(&text_task(
                TaskKind::TranslateSentence,
                "bonjour",
                Some(InputHint::SourceLang(Lang::Fr)),
            ))
            .expect("render");
        assert!(messages[0].content.contains("源语言：法语"));
    }

    #[test]
    fn image_kinds_are_placeholders_until_m5() {
        let registry = PromptRegistry::new();
        let task = Task {
            kind: TaskKind::ImageOcr,
            input: TaskInput::Image {
                png: std::sync::Arc::from(&b"png"[..]),
                region: crate::model::ScreenRect {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
            },
            options: TaskOptions::default(),
        };
        assert_eq!(
            registry.render(&task),
            Err(GlossError::UnsupportedModality),
            "image template lands with vision task"
        );
    }

    #[test]
    fn modality_mismatch_is_rejected_before_rendering() {
        let registry = PromptRegistry::new();
        let task = text_task(TaskKind::ImageOcr, "not an image", None);
        assert_eq!(registry.render(&task), Err(GlossError::UnsupportedModality));
    }

    #[test]
    fn messages_serialize_to_openai_shape() {
        let message = ChatMessage::system("你是翻译助手");
        let json = serde_json::to_string(&message).expect("message should serialize");
        assert_eq!(json, r#"{"role":"system","content":"你是翻译助手"}"#);
        let user = ChatMessage::user("hi");
        let json = serde_json::to_string(&user).expect("message should serialize");
        assert!(json.contains(r#""role":"user""#));
    }

    #[test]
    fn classify_prompt_carries_allowed_kinds_and_the_text() {
        let registry = PromptRegistry::new();
        let allowed = [TaskKind::TranslateWord, TaskKind::ExplainCode];
        for locale in [Locale::Zh, Locale::En] {
            let messages = registry.render_classify(locale, &allowed, "gloss 原文");
            assert_eq!(messages.len(), 2);
            assert_eq!(messages[1].content, "gloss 原文");
            assert!(messages[0].content.contains("TranslateWord"), "{locale:?}");
            assert!(messages[0].content.contains("ExplainCode"));
            assert!(
                !messages[0].content.contains("Auto"),
                "the sentinel is never offered as an answer: {}",
                messages[0].content
            );
            assert!(messages[0].content.contains("\"kind\""));
            assert!(!messages[0].content.contains("{{"));
        }
    }

    #[test]
    fn render_rejects_the_auto_sentinel() {
        let registry = PromptRegistry::new();
        let task = text_task(TaskKind::Auto, "待分类", None);
        assert_eq!(
            registry.render(&task),
            Err(GlossError::ClassifyRequired),
            "Auto must be classified before it can render"
        );
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
        let declared = [
            "target",
            "hint",
            "contract",
            "fence",
            "schema",
            "code_lang",
            "source_lang",
        ];
        for locale in [Locale::Zh, Locale::En] {
            let templates = locale.templates();
            let files = [
                ("word_card.md", templates.word_card),
                ("sentence.md", templates.sentence),
                ("code.md", templates.code),
                ("contract.md", templates.contract),
                ("hints.md", templates.hints),
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
    fn every_locale_renders_without_leftover_placeholders() {
        let registry = PromptRegistry::new();
        let hints = [
            None,
            Some(InputHint::CodeLanguage("rust".into())),
            Some(InputHint::SourceLang(Lang::Ja)),
        ];
        for locale in [Locale::Zh, Locale::En] {
            for kind in [
                TaskKind::TranslateWord,
                TaskKind::TranslateSentence,
                TaskKind::ExplainCode,
            ] {
                for hint in hints.clone() {
                    let mut task = text_task(kind, "gloss", hint);
                    task.options.prompt_locale = Some(locale);
                    let messages = registry.render(&task).expect("render");
                    for message in &messages {
                        assert!(
                            !message.content.contains("{{"),
                            "{locale:?} x {kind:?} leaves a placeholder: {}",
                            message.content
                        );
                    }
                    assert!(
                        messages[0].content.contains(STRUCTURED_FENCE),
                        "{locale:?} x {kind:?} must carry the structured contract"
                    );
                }
            }
        }
    }

    #[test]
    fn english_locale_renders_english_prompts() {
        let registry = PromptRegistry::new();
        let mut task = localized_task(TaskKind::TranslateSentence, "bonjour", Locale::En);
        task.input = TaskInput::Text {
            text: "bonjour".into(),
            hint: Some(InputHint::SourceLang(Lang::Fr)),
        };
        let messages = registry.render(&task).expect("render");
        assert!(messages[0].content.contains("translation assistant"));
        assert!(messages[0].content.contains("Source language: French"));
        assert!(messages[0].content.contains("JSON block"));
        assert!(messages[1].content.contains("Source language: French"));
        assert!(!messages[0].content.contains("翻译助手"));
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
        assert!(english_prompt[0].content.contains("translation assistant"));
        assert!(
            english_prompt[0].content.contains("Chinese"),
            "template language must not move the default target: {}",
            english_prompt[0].content
        );

        let mut chinese_prompt = localized_task(TaskKind::TranslateSentence, "hello", Locale::Zh);
        chinese_prompt.options.target_lang = Some(Lang::En);
        let messages = registry.render(&chinese_prompt).expect("render");
        assert!(messages[0].content.contains("翻译助手"));
        assert!(messages[0].content.contains("英语"));
    }

    #[test]
    fn prompt_locale_defaults_to_chinese() {
        let registry = PromptRegistry::new();
        let defaulted = registry
            .render(&text_task(TaskKind::TranslateWord, "gloss", None))
            .expect("render");
        let explicit = registry
            .render(&localized_task(
                TaskKind::TranslateWord,
                "gloss",
                Locale::Zh,
            ))
            .expect("render");
        assert_eq!(defaulted, explicit, "no locale means the Chinese templates");
        assert!(defaulted[0].content.contains("词典助手"));
        assert_eq!(Locale::default(), Locale::Zh);
    }
}
