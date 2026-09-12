//! 任务模型：任务类型、输入模态、热键绑定与管道消息的载荷类型。

use std::sync::Arc;

use crate::model::{Lang, ScreenRect};

/// 任务类型：新增场景 = 加变体 + Prompt 模板 + 结构化结果变体 + UI 模板，管道不动。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    /// 单词（词典式卡：音标/词性/释义/例句）。
    TranslateWord,
    /// 句子/段落翻译。
    TranslateSentence,
    /// 代码解释（输入可带语言提示）。
    ExplainCode,
    /// 框选图片 → 提取文本。
    ImageOcr,
    /// 框选图片 → 解释内容。
    ImageExplain,
}

/// 输入源规格：触发时确定「去哪取」，不携带数据。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputSource {
    /// 前台应用选区（文本）。
    Selection,
    /// 框选屏幕区域（图像）；rect 由手势产生。
    Region,
    // 语音输入预留 Microphone 变体：端口、管道、gen 协议均无需改动。
}

/// 模态提示：为 prompt 填充提供上下文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputHint {
    /// 代码语言，如 "rust"。
    CodeLanguage(String),
    SourceLang(Lang),
}

/// 热键绑定：一个热键 → 一个任务类型 + 一个输入源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyBinding {
    /// 如 "Cmd+Shift+1"。
    pub trigger: String,
    pub kind: TaskKind,
    pub source: InputSource,
}

/// 输入模态：取材产物的统一枚举形态，消息间 move 所有权。
#[derive(Debug, Clone, PartialEq)]
pub enum TaskInput {
    Text {
        text: String,
        /// 模态提示（如代码语言），用于 prompt 填充。
        hint: Option<InputHint>,
    },
    Image {
        /// PNG 字节。
        png: Arc<[u8]>,
        /// 截图区域屏幕坐标（供 UI 展示上下文）。
        region: ScreenRect,
    },
    /// 语音输入预留（MVP 不实现）：届时只新增取材端口与对应 TaskKind/模板。
    Audio {
        /// 音频字节（编码格式落地时定）。
        bytes: Arc<[u8]>,
        duration_hint: Option<f32>,
    },
}

/// 任务选项；留空的字段按配置默认值填充。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TaskOptions {
    /// 目标语言，None = 自动检测（默认中文）。
    pub target_lang: Option<Lang>,
    /// 回答深度档位。
    pub detail_level: Option<u8>,
    /// 临时覆盖该任务使用的模型。
    pub model_override: Option<String>,
}

/// 一条待执行任务 = 类型 + 输入 + 选项。
#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    pub kind: TaskKind,
    pub input: TaskInput,
    pub options: TaskOptions,
}

/// 任务产物：正文统一 markdown，另带 kind 专属结构化字段供 UI 精排。
#[derive(Debug, Clone, PartialEq)]
pub struct TaskOutcome {
    pub kind: TaskKind,
    /// markdown 正文（流式 chunk 拼接）。
    pub body: String,
    pub structured: OutcomeStructured,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OutcomeStructured {
    /// 单词卡（词典式）。
    WordCard {
        word: String,
        phonetic: Option<String>,
        senses: Vec<Sense>,
    },
    /// 句子翻译/代码解释/图片解释。
    Plain { title: Option<String> },
    /// OCR：另存纯文本便于一键复制。
    Extracted { text: String },
}

/// 词条释义（词典式卡）。
#[derive(Debug, Clone, PartialEq)]
pub struct Sense {
    /// 词性，如 "n."。
    pub pos: Option<String>,
    pub meaning: String,
    pub examples: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_input_carries_text_and_hint() {
        let input = TaskInput::Text {
            text: "hello".into(),
            hint: Some(InputHint::CodeLanguage("rust".into())),
        };
        match input {
            TaskInput::Text { text, hint } => {
                assert_eq!(text, "hello");
                assert_eq!(hint, Some(InputHint::CodeLanguage("rust".into())));
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn task_binds_kind_input_and_options() {
        let task = Task {
            kind: TaskKind::TranslateWord,
            input: TaskInput::Text {
                text: "gloss".into(),
                hint: None,
            },
            options: TaskOptions {
                target_lang: Some(Lang::Zh),
                ..Default::default()
            },
        };
        assert_eq!(task.kind, TaskKind::TranslateWord);
        assert_eq!(task.options.target_lang, Some(Lang::Zh));
        assert_eq!(task.options.detail_level, None);
    }

    #[test]
    fn image_input_shares_png_bytes_via_arc() {
        let png: Arc<[u8]> = vec![0x89, b'P', b'N', b'G'].into();
        let input = TaskInput::Image {
            png: Arc::clone(&png),
            region: ScreenRect {
                x: 10,
                y: 20,
                width: 300,
                height: 200,
            },
        };
        match input {
            TaskInput::Image { png: moved, region } => {
                assert!(Arc::ptr_eq(&png, &moved));
                assert_eq!(
                    region,
                    ScreenRect {
                        x: 10,
                        y: 20,
                        width: 300,
                        height: 200
                    }
                );
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn hotkey_binding_pairs_kind_with_source() {
        let binding = HotkeyBinding {
            trigger: "Cmd+Shift+1".into(),
            kind: TaskKind::TranslateSentence,
            source: InputSource::Selection,
        };
        assert_eq!(binding.source, InputSource::Selection);
    }

    #[test]
    fn outcome_carries_structured_variants() {
        let card = TaskOutcome {
            kind: TaskKind::TranslateWord,
            body: "# gloss".into(),
            structured: OutcomeStructured::WordCard {
                word: "gloss".into(),
                phonetic: Some("/ɡlɒs/".into()),
                senses: vec![Sense {
                    pos: Some("n.".into()),
                    meaning: "光泽；注释".into(),
                    examples: vec![],
                }],
            },
        };
        assert!(
            matches!(card.structured, OutcomeStructured::WordCard { ref word, .. } if word == "gloss")
        );
    }
}
