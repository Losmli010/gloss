//! 任务模型：任务类型、输入模态与管道消息的载荷类型。

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::config::DEFAULT_TEXT_MODEL;
use crate::model::{GlossError, Lang, Locale, ScreenRect};

/// 任务类型：新增场景 = 加变体 + Prompt 模板 + 结构化结果变体 + UI 模板，管道不动。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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

/// 模态提示：为 prompt 填充提供上下文。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InputHint {
    /// 代码语言，如 "rust"。
    CodeLanguage(String),
    /// 已知源语言，避免模型误判（目标语言由 `TaskOptions` 决定）。
    SourceLang(Lang),
}

/// 输入模态：取材产物的统一枚举形态，消息间 move 所有权。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TaskInput {
    /// 文本输入。
    Text {
        /// 待处理的文本原文。
        text: String,
        /// 模态提示（如代码语言），用于 prompt 填充。
        hint: Option<InputHint>,
    },
    /// 图像输入。
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
        /// 时长提示（秒），供 prompt 与 UI 参考。
        duration_hint: Option<f32>,
    },
}

/// 任务选项；留空的字段按配置默认值填充。
///
/// 全部字段都是**单次快照冻结**的：App 在触发时按配置快照一次性解析填入，
/// 执行途中不再回读配置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskOptions {
    /// 目标语言，None = 自动检测（默认中文）。
    pub target_lang: Option<Lang>,
    /// 回答深度档位。
    pub detail_level: Option<u8>,
    /// 本任务使用的模型 id：App 在触发时按 `Config::model` 冻结进来。
    /// 缺省值只服务测试直构（[`DEFAULT_TEXT_MODEL`]）；App 路径恒由
    /// 状态机按配置快照填入。
    pub model: String,
    /// 本任务使用的 prompt 模板语言：App 在触发时按 `Config::language`
    /// 解析（`System` 按启动期读到的系统语言落定）填入，缺省中文模板。
    /// locale 参与缓存 key——换了模板语言后，同输入不得命中旧语言的产物。
    pub prompt_locale: Option<Locale>,
}

impl Default for TaskOptions {
    fn default() -> Self {
        Self {
            target_lang: None,
            detail_level: None,
            model: DEFAULT_TEXT_MODEL.to_owned(),
            prompt_locale: None,
        }
    }
}

/// 一条待执行任务 = 类型 + 输入 + 选项。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    /// 任务类型。
    pub kind: TaskKind,
    /// 输入数据。
    pub input: TaskInput,
    /// 任务选项（留空的字段按配置默认值填充）。
    pub options: TaskOptions,
}

impl Task {
    /// 模态约束表校验：编排层在渲染前调用，非法组合在进 prompt 与引擎前
    /// 拒绝。
    pub fn validate(&self) -> Result<(), GlossError> {
        validate_modality(self.kind, &self.input)
    }
}

/// 模态约束表：任务类型与输入模态的合法组合。唯一被拒的
/// 错误是 [`GlossError::UnsupportedModality`]。
pub fn validate_modality(kind: TaskKind, input: &TaskInput) -> Result<(), GlossError> {
    let legal = match (kind, input) {
        (
            TaskKind::TranslateWord | TaskKind::TranslateSentence | TaskKind::ExplainCode,
            TaskInput::Text { .. },
        )
        | (TaskKind::ImageOcr | TaskKind::ImageExplain, TaskInput::Image { .. }) => true,
        // 语音任务未落地：任何 kind + Audio 都是非法组合。
        _ => false,
    };
    if legal {
        Ok(())
    } else {
        Err(GlossError::UnsupportedModality)
    }
}

/// 任务产物：注文统一 markdown（经注疏的「注」），另带 kind 专属结构化
/// 字段供 UI 精排（词卡的字/音/义/例即说文解字式分层）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskOutcome {
    /// 产物对应的任务类型。
    pub kind: TaskKind,
    /// markdown 注文（流式 chunk 拼接；词卡为叙释 prose，句译/讲解为主体）。
    pub note: String,
    /// kind 专属结构化字段，见 [`OutcomeStructured`]。
    pub structured: OutcomeStructured,
}

/// 结构化结果：按 `TaskKind` 给出 UI 精排所需的字段，与 markdown 正文并行下发。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OutcomeStructured {
    /// 单词卡（词典式）。
    WordCard {
        /// 词条原文。
        word: String,
        /// 音标（模型给出时才有）。
        phonetic: Option<String>,
        /// 释义列表。
        senses: Vec<Sense>,
    },
    /// 句子翻译/代码解释/图片解释。
    Plain {
        /// 标题（如代码解释的一句话摘要），可缺省。
        title: Option<String>,
    },
    /// OCR：另存纯文本便于一键复制。
    Extracted {
        /// 提取出的纯文本。
        text: String,
    },
}

/// 词条释义（词典式卡）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sense {
    /// 词性，如 "n."。
    pub pos: Option<String>,
    /// 释义内容。
    pub meaning: String,
    /// 例句列表。
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
    fn task_options_default_carries_the_factory_model() {
        let options = TaskOptions::default();
        assert_eq!(options.model, DEFAULT_TEXT_MODEL);
        assert_eq!(options.target_lang, None);
        assert_eq!(options.prompt_locale, None);
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
    fn outcome_carries_structured_variants() {
        let card = TaskOutcome {
            kind: TaskKind::TranslateWord,
            note: "# gloss".into(),
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

    fn text_task(kind: TaskKind) -> Task {
        Task {
            kind,
            input: TaskInput::Text {
                text: "hello".into(),
                hint: None,
            },
            options: TaskOptions::default(),
        }
    }

    fn image_task(kind: TaskKind) -> Task {
        Task {
            kind,
            input: TaskInput::Image {
                png: Arc::from(&b"png"[..]),
                region: ScreenRect {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
            },
            options: TaskOptions::default(),
        }
    }

    #[test]
    fn modality_matrix_is_enforced_cell_by_cell() {
        let all_kinds = [
            TaskKind::TranslateWord,
            TaskKind::TranslateSentence,
            TaskKind::ExplainCode,
            TaskKind::ImageOcr,
            TaskKind::ImageExplain,
        ];
        let text_legal = |kind| {
            matches!(
                kind,
                TaskKind::TranslateWord | TaskKind::TranslateSentence | TaskKind::ExplainCode
            )
        };
        let image_legal = |kind| matches!(kind, TaskKind::ImageOcr | TaskKind::ImageExplain);
        for kind in all_kinds {
            let (text_expect, image_expect) = (
                text_legal(kind)
                    .then_some(())
                    .ok_or(GlossError::UnsupportedModality),
                image_legal(kind)
                    .then_some(())
                    .ok_or(GlossError::UnsupportedModality),
            );
            assert_eq!(text_task(kind).validate(), text_expect, "text × {kind:?}");
            assert_eq!(
                image_task(kind).validate(),
                image_expect,
                "image × {kind:?}"
            );
        }

        let audio_task = |kind| Task {
            kind,
            input: TaskInput::Audio {
                bytes: Arc::from(&b"au"[..]),
                duration_hint: None,
            },
            options: TaskOptions::default(),
        };
        for kind in all_kinds {
            assert_eq!(
                audio_task(kind).validate(),
                Err(GlossError::UnsupportedModality),
                "audio × {kind:?} must be reserved"
            );
        }
    }

    #[test]
    fn task_round_trips_through_serde() {
        let task = Task {
            kind: TaskKind::ExplainCode,
            input: TaskInput::Text {
                text: "fn main() {}".into(),
                hint: Some(InputHint::CodeLanguage("rust".into())),
            },
            options: TaskOptions {
                target_lang: Some(Lang::Ja),
                model: "mock-model".into(),
                ..Default::default()
            },
        };
        let json = serde_json::to_string(&task).expect("task should serialize");
        let back: Task = serde_json::from_str(&json).expect("task should deserialize");
        assert_eq!(back, task);
    }

    #[test]
    fn image_input_round_trips_through_serde() {
        let png: Arc<[u8]> = vec![0x89, b'P', b'N', b'G'].into();
        let input = TaskInput::Image {
            png: Arc::clone(&png),
            region: ScreenRect {
                x: -8,
                y: 4,
                width: 1920,
                height: 1080,
            },
        };
        let json = serde_json::to_string(&input).expect("image input should serialize");
        let back: TaskInput = serde_json::from_str(&json).expect("image input should deserialize");
        assert_eq!(back, input, "bytes and rect must survive the roundtrip");
        assert!(!matches!(&back, TaskInput::Image { png: moved, .. } if Arc::ptr_eq(&png, moved)));
    }
}
