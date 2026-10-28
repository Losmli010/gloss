//! 任务模型：任务类型、输入模态与管道消息的载荷类型。
//!
//! 文本分类的清单外落点是通用任务（分类模板以兜底
//! 类别指认它，见分类校验器）；任务选项
//! 在触发时一次性冻结，执行途中不回读配置。

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::config::DEFAULT_MODEL;
use crate::model::{GlossError, Lang, Locale};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TaskKind {
    TranslateWord,
    TranslateSentence,
    ExplainCode,
    ImageExplain,
    General,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TaskInput {
    Text { text: String },
    Image { png: Arc<[u8]> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskOptions {
    pub target_lang: Option<Lang>,
    pub model: Arc<str>,
    pub prompt_locale: Option<Locale>,
}

#[doc(hidden)]
impl Default for TaskOptions {
    fn default() -> Self {
        Self {
            target_lang: None,
            model: Arc::from(DEFAULT_MODEL),
            prompt_locale: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "TaskParts")]
pub struct Task {
    kind: TaskKind,
    input: TaskInput,
    options: TaskOptions,
}

impl Task {
    pub fn new(kind: TaskKind, input: TaskInput, options: TaskOptions) -> Result<Self, GlossError> {
        validate_modality(kind, &input)?;
        Ok(Self {
            kind,
            input,
            options,
        })
    }

    pub fn kind(&self) -> TaskKind {
        self.kind
    }

    pub fn input(&self) -> &TaskInput {
        &self.input
    }

    pub fn options(&self) -> &TaskOptions {
        &self.options
    }

    pub fn options_mut(&mut self) -> &mut TaskOptions {
        &mut self.options
    }
}

#[derive(Deserialize)]
struct TaskParts {
    kind: TaskKind,
    input: TaskInput,
    options: TaskOptions,
}

impl TryFrom<TaskParts> for Task {
    type Error = GlossError;

    fn try_from(parts: TaskParts) -> Result<Self, Self::Error> {
        Self::new(parts.kind, parts.input, parts.options)
    }
}

fn validate_modality(kind: TaskKind, input: &TaskInput) -> Result<(), GlossError> {
    let legal = matches!(
        (kind, input),
        (
            TaskKind::TranslateWord
                | TaskKind::TranslateSentence
                | TaskKind::ExplainCode
                | TaskKind::General,
            TaskInput::Text { .. },
        ) | (TaskKind::ImageExplain, TaskInput::Image { .. })
    );
    if legal {
        Ok(())
    } else {
        Err(GlossError::UnsupportedModality)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskOutcome {
    pub kind: TaskKind,
    pub note: String,
    pub structured: OutcomeStructured,
    #[serde(default)]
    pub degraded: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OutcomeStructured {
    WordCard {
        phonetic: Option<String>,
        examples: Vec<String>,
    },
    Plain {
        examples: Vec<String>,
    },
    Code {
        examples: Vec<String>,
        code_language: Option<String>,
    },
}

impl OutcomeStructured {
    pub fn examples(&self) -> &[String] {
        match self {
            Self::WordCard { examples, .. }
            | Self::Plain { examples }
            | Self::Code { examples, .. } => examples,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_input_carries_text_only() {
        let input = TaskInput::Text {
            text: "hello".into(),
        };
        match input {
            TaskInput::Text { text } => assert_eq!(text, "hello"),
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn task_binds_kind_input_and_options() {
        let task = Task::new(
            TaskKind::TranslateWord,
            TaskInput::Text {
                text: "gloss".into(),
            },
            TaskOptions {
                target_lang: Some(Lang::Zh),
                ..Default::default()
            },
        )
        .expect("a text kind with text input is legal");
        assert_eq!(task.kind(), TaskKind::TranslateWord);
        assert_eq!(task.options().target_lang, Some(Lang::Zh));
    }

    #[test]
    fn task_options_default_carries_the_factory_model() {
        let options = TaskOptions::default();
        assert_eq!(options.model, DEFAULT_MODEL.into());
        assert_eq!(options.target_lang, None);
        assert_eq!(options.prompt_locale, None);
    }

    #[test]
    fn image_input_shares_png_bytes_via_arc() {
        let png: Arc<[u8]> = vec![0x89, b'P', b'N', b'G'].into();
        let input = TaskInput::Image {
            png: Arc::clone(&png),
        };
        match input {
            TaskInput::Image { png: moved } => {
                assert!(Arc::ptr_eq(&png, &moved));
            }
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn outcome_carries_word_card_subprov_and_the_code_verdict() {
        let card = TaskOutcome {
            kind: TaskKind::TranslateWord,
            note: "义释 markdown".into(),
            structured: OutcomeStructured::WordCard {
                phonetic: Some("/ɡlɒs/".into()),
                examples: vec!["a gloss of silk".into()],
            },
            degraded: false,
        };
        assert!(
            matches!(card.structured, OutcomeStructured::WordCard { phonetic, .. } if phonetic.as_deref() == Some("/ɡlɒs/"))
        );
        assert!(!card.degraded);

        let code = TaskOutcome {
            kind: TaskKind::ExplainCode,
            note: "讲解".into(),
            structured: OutcomeStructured::Code {
                examples: vec![],
                code_language: Some("rust".into()),
            },
            degraded: false,
        };
        assert!(
            matches!(&code.structured, OutcomeStructured::Code { code_language, .. } if code_language.as_deref() == Some("rust")),
            "the language verdict rides the Code variant"
        );
    }

    #[test]
    fn degraded_defaults_to_false_on_the_wire() {
        let outcome = TaskOutcome {
            kind: TaskKind::TranslateWord,
            note: "义".into(),
            structured: OutcomeStructured::WordCard {
                phonetic: None,
                examples: Vec::new(),
            },
            degraded: false,
        };
        let wire = serde_json::to_string(&outcome).expect("outcome serializes");
        let without_flag = wire.replace(r#","degraded":false"#, "");
        assert_ne!(
            wire, without_flag,
            "the flag must serialize explicitly when false: {wire}"
        );
        let back: TaskOutcome =
            serde_json::from_str(&without_flag).expect("an outcome without the flag parses");
        assert!(
            !back.degraded,
            "serde default keeps old entries on the normal path"
        );

        let degraded = TaskOutcome {
            degraded: true,
            ..outcome
        };
        let wire = serde_json::to_string(&degraded).expect("outcome serializes");
        assert!(
            wire.contains(r#""degraded":true"#),
            "the flag round-trips explicitly: {wire}"
        );
        let back: TaskOutcome = serde_json::from_str(&wire).expect("degraded round-trips");
        assert!(back.degraded);
    }

    fn text_input() -> TaskInput {
        TaskInput::Text {
            text: "hello".into(),
        }
    }

    fn image_input() -> TaskInput {
        TaskInput::Image {
            png: Arc::from(&b"png"[..]),
        }
    }

    #[test]
    fn modality_matrix_is_enforced_cell_by_cell() {
        let all_kinds = [
            TaskKind::TranslateWord,
            TaskKind::TranslateSentence,
            TaskKind::ExplainCode,
            TaskKind::ImageExplain,
            TaskKind::General,
        ];
        let legal = |kind: TaskKind, input: &TaskInput| {
            Task::new(kind, input.clone(), TaskOptions::default()).map(|_| ())
        };
        for kind in all_kinds {
            let text_expect = matches!(
                kind,
                TaskKind::TranslateWord
                    | TaskKind::TranslateSentence
                    | TaskKind::ExplainCode
                    | TaskKind::General
            );
            let image_expect = kind == TaskKind::ImageExplain;
            assert_eq!(
                legal(kind, &text_input()),
                text_expect
                    .then_some(())
                    .ok_or(GlossError::UnsupportedModality),
                "text × {kind:?}"
            );
            assert_eq!(
                legal(kind, &image_input()),
                image_expect
                    .then_some(())
                    .ok_or(GlossError::UnsupportedModality),
                "image × {kind:?}"
            );
        }
    }

    #[test]
    fn task_deserialization_goes_through_the_modality_gate() {
        let legal = serde_json::json!({
            "kind": "TranslateWord",
            "input": { "Text": { "text": "gloss" } },
            "options": { "target_lang": null, "model": DEFAULT_MODEL, "prompt_locale": null },
        });
        let task: Task = serde_json::from_value(legal).expect("a legal pair deserializes");
        assert_eq!(task.kind(), TaskKind::TranslateWord);

        let illegal = serde_json::json!({
            "kind": "ImageExplain",
            "input": { "Text": { "text": "gloss" } },
            "options": { "target_lang": null, "model": DEFAULT_MODEL, "prompt_locale": null },
        });
        assert!(
            serde_json::from_value::<Task>(illegal).is_err(),
            "a wire payload must not bypass the modality gate"
        );
    }

    #[test]
    fn task_round_trips_through_serde() {
        let task = Task::new(
            TaskKind::ExplainCode,
            TaskInput::Text {
                text: "fn main() {}".into(),
            },
            TaskOptions {
                target_lang: Some(Lang::Ja),
                model: "mock-model".into(),
                prompt_locale: None,
            },
        )
        .expect("a text kind with text input is legal");
        let json = serde_json::to_string(&task).expect("task should serialize");
        let back: Task = serde_json::from_str(&json).expect("task should deserialize");
        assert_eq!(back, task);
    }

    #[test]
    fn image_input_round_trips_through_serde() {
        let png: Arc<[u8]> = vec![0x89, b'P', b'N', b'G'].into();
        let input = TaskInput::Image {
            png: Arc::clone(&png),
        };
        let json = serde_json::to_string(&input).expect("image input should serialize");
        let back: TaskInput = serde_json::from_str(&json).expect("image input should deserialize");
        assert_eq!(back, input, "bytes must survive the roundtrip");
        assert!(!matches!(&back, TaskInput::Image { png: moved } if Arc::ptr_eq(&png, moved)));
    }
}
