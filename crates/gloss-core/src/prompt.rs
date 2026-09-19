//! Prompt 模板注册表：kind + input → OpenAI 兼容 messages。
//!
//! 文本任务 3 个模板（词卡/句译/代码解释）统一输出契约：markdown 正文 +
//! 末尾 ```gloss 围栏 JSON 块（按 kind 携带 [`crate::task::OutcomeStructured`]
//! 的结构化字段），供任务编排解析——正文给人读，JSON 给 UI 精排。
//! 图像/音频输入一律 [`GlossError::UnsupportedModality`]，
//! 不发出注定无效的请求。
//!
//! 参数缺省：`options.target_lang` 缺省按中文；`InputHint` 缺省不注入
//! 提示行。模板内容面向模型（用户可见产物），用中文书写不受日志英文
//! 约束（AGENTS.md 约束「日志一律英文」）。

use serde::Serialize;

use crate::model::{GlossError, Lang};
use crate::task::{InputHint, Task, TaskInput, TaskKind, TaskOptions, validate_modality};

/// 目标语言缺省值（`TaskOptions::target_lang` 文档：缺省中文）。
const DEFAULT_TARGET: &str = "中文";

/// 结构化 JSON 块的围栏标记：正文之后模型按此契约追加结构化字段；
/// 编排侧（engine）按同一标记解析，UI 侧流式渲染按它过滤未完成的
/// 结构化块——三处共用单一事实源。
pub const STRUCTURED_FENCE: &str = "```gloss";

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
/// 配置化（自定义模板/热键绑定）落地时在此扩展注册机制。
#[derive(Debug, Default, Clone, Copy)]
pub struct PromptRegistry;

impl PromptRegistry {
    /// 创建注册表。
    pub fn new() -> Self {
        Self
    }

    /// 渲染任务的完整 messages：先过模态约束表，非法组合
    /// 返回 [`GlossError::UnsupportedModality`]。
    pub fn render(&self, task: &Task) -> Result<Vec<ChatMessage>, GlossError> {
        validate_modality(task.kind, &task.input)?;
        let TaskInput::Text { text, hint } = &task.input else {
            // 图像模板随图像任务落地；音频是预留模态，模态校验已拦，
            // 这里对图像输入显式收口。
            return Err(GlossError::UnsupportedModality);
        };
        let target = target_language(&task.options);
        let system = system_prompt(task.kind, &target, hint.as_ref());
        Ok(vec![
            ChatMessage::system(system),
            ChatMessage::user(user_content(text, hint.as_ref())),
        ])
    }
}

/// 目标语言显示名：缺省中文。
fn target_language(options: &TaskOptions) -> String {
    options
        .target_lang
        .as_ref()
        .map_or_else(|| DEFAULT_TARGET.to_owned(), lang_display)
}

/// 语言显示名（模板面向模型，用中文名即可）。
fn lang_display(lang: &Lang) -> String {
    match lang {
        Lang::Zh => "中文".into(),
        Lang::En => "英语".into(),
        Lang::Ja => "日语".into(),
        Lang::Ko => "韩语".into(),
        Lang::Fr => "法语".into(),
        Lang::Other(name) => name.clone(),
    }
}

/// 模态提示行：缺省（无 hint）为空串，不注入任何行。
fn hint_lines(hint: Option<&InputHint>) -> String {
    match hint {
        Some(InputHint::CodeLanguage(lang)) => format!("代码语言：{lang}\n"),
        Some(InputHint::SourceLang(lang)) => {
            format!("源语言：{}\n", lang_display(lang))
        }
        None => String::new(),
    }
}

/// 系统指令：按 kind 选模板，注入目标语言与提示行。
fn system_prompt(kind: TaskKind, target: &str, hint: Option<&InputHint>) -> String {
    let hint_line = hint_lines(hint);
    let structured = structured_contract(kind);
    let instruction = match kind {
        TaskKind::TranslateWord => {
            format!(
                "你是词典助手。对用户给出的词条输出词典式词卡：\
                 正文用 markdown，依次给出音标、按词性分组的释义与例句。\
                 释义与例句使用{target}。"
            )
        }
        TaskKind::TranslateSentence => {
            format!(
                "你是翻译助手。把用户给出的句子或段落翻译成{target}：\
                 正文用 markdown，先给出译文，再视需要附简短译注（无则省略）。"
            )
        }
        TaskKind::ExplainCode => {
            format!(
                "你是代码讲解助手。解释用户给出的代码：\
                 正文用 markdown，先一句话概括作用，再分点说明关键逻辑。\
                 说明文字使用{target}。"
            )
        }
        // 图像 kind 走不到这里：render 已把非文本输入收口。
        TaskKind::ImageOcr | TaskKind::ImageExplain => String::new(),
    };
    format!("{instruction}\n{hint_line}{structured}")
}

/// 输出契约段：正文之后的结构化 JSON 块要求，按 kind 给出字段。
fn structured_contract(kind: TaskKind) -> String {
    let schema = match kind {
        TaskKind::TranslateWord => {
            r#"{"word":"词条原文","phonetic":"音标或 null","senses":[{"pos":"词性或 null","meaning":"释义","examples":["例句"]}]}"#
        }
        TaskKind::TranslateSentence | TaskKind::ExplainCode => r#"{"title":"一句话摘要或 null"}"#,
        TaskKind::ImageOcr => r#"{"text":"提取的纯文本"}"#,
        TaskKind::ImageExplain => r#"{"title":"一句话摘要或 null"}"#,
    };
    format!(
        "正文结束后，另起一行输出 {STRUCTURED_FENCE} 围栏的 JSON 块（\
         {STRUCTURED_FENCE} 单独一行开始，闭合 ``` 单独一行结束），\
         字段固定为：{schema}。除正文与该 JSON 块外不要输出任何内容，\
         也不要把整个回复包进代码块。"
    )
}

/// 用户消息：输入原文 + 提示行（无 hint 时只有原文）。
fn user_content(text: &str, hint: Option<&InputHint>) -> String {
    let hint_line = hint_lines(hint);
    if hint_line.is_empty() {
        text.to_owned()
    } else {
        format!("{hint_line}\n{text}")
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
}
