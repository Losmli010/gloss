//! 任务模型：任务类型、输入模态与管道消息的载荷类型。
//!
//! 文本分类的清单外落点是 [`TaskKind::General`]（分类器不提供它）；任务选项
//! 在触发时一次性冻结，执行途中不回读配置。

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::config::DEFAULT_MODEL;
use crate::model::{GlossError, Lang, Locale};

/// 任务类型。新增场景 = 加变体 + prompt 模板 + 结构化结果变体 + UI 模板；
/// 管道只搬运载荷，不按类型分支。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TaskKind {
    /// 单词或短语：落词卡产物（音标与例句在 `structured`，释义在 `note`）。
    TranslateWord,
    TranslateSentence,
    ExplainCode,
    /// 图像解读：唯一接受 [`TaskInput::Image`] 的 kind（见 [`validate_modality`]）。
    ImageExplain,
    /// 通用任务：不在 [`crate::classify::CLASSIFY_KINDS`] 里，是分类失效时的
    /// 执行落点。
    General,
}

/// 输入模态：取材产物的统一形态，随消息 move 所有权。
///
/// 不携带模态提示：源语言与代码语言都由模型从原文判断，代码语言经
/// [`OutcomeStructured::Code`] 回传 UI。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TaskInput {
    Text {
        text: String,
    },
    Image {
        /// PNG 字节；编码成 data URL 归传输层。
        png: Arc<[u8]>,
    },
}

/// 任务选项：全部字段都是**单次快照冻结**的——App 在触发时按配置快照解析
/// 填入，执行途中不再回读配置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskOptions {
    /// 目标语言；`None` 走缺省中文（[`crate::prompt`] 的 `DEFAULT_TARGET`）。
    pub target_lang: Option<Lang>,
    /// 本任务使用的模型 id；`Arc<str>` 让在途副本与下发请求共享同一份，
    /// 复制只是引用计数。缺省值只服务测试直构，App 路径恒按配置快照填入。
    pub model: Arc<str>,
    /// prompt 模板语言；缺省中文。它参与缓存 key——换模板语言后同输入
    /// 不得命中旧语言的产物。
    pub prompt_locale: Option<Locale>,
}

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
pub struct Task {
    pub kind: TaskKind,
    pub input: TaskInput,
    pub options: TaskOptions,
}

/// 模态约束表：任务类型与输入模态的合法组合。唯一被拒的错误是
/// [`GlossError::UnsupportedModality`]。
pub fn validate_modality(kind: TaskKind, input: &TaskInput) -> Result<(), GlossError> {
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

/// 任务产物：`note` 是流式拼接的 markdown 正文，`structured` 是 UI 精排字段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskOutcome {
    pub kind: TaskKind,
    /// markdown 正文；降级产物恒为空串（原文垃圾不展示不留存）。
    pub note: String,
    /// 按 kind 的附加字段，见 [`OutcomeStructured`]。
    pub structured: OutcomeStructured,
    /// 契约校验失败后的兜底产物标记（此时 `note` 为空串）：不进缓存、UI
    /// 出降级卡。`serde(default)` 是为兼容缺该字段的旧工件——没有标记即
    /// 视为正常产物。
    #[serde(default)]
    pub degraded: bool,
}

/// 按 `TaskKind` 分派的附加字段：释义在 [`TaskOutcome::note`]，这里只带音、
/// 例与语言判定。三个变体与 `crate::schema` 的 wire 契约同构。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OutcomeStructured {
    /// 词卡：音标（模型给出时才有）与例句。
    WordCard {
        phonetic: Option<String>,
        examples: Vec<String>,
    },
    /// 句译、讲解、图像解读与通用任务共用：展开条目列表。
    Plain { examples: Vec<String> },
    /// 代码讲解：展开条目 + 模型判定的语言（不确定为 `None`）。
    Code {
        examples: Vec<String>,
        code_language: Option<String>,
    },
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
        let task = Task {
            kind: TaskKind::TranslateWord,
            input: TaskInput::Text {
                text: "gloss".into(),
            },
            options: TaskOptions {
                target_lang: Some(Lang::Zh),
                ..Default::default()
            },
        };
        assert_eq!(task.kind, TaskKind::TranslateWord);
        assert_eq!(task.options.target_lang, Some(Lang::Zh));
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

    fn text_task(kind: TaskKind) -> Task {
        Task {
            kind,
            input: TaskInput::Text {
                text: "hello".into(),
            },
            options: TaskOptions::default(),
        }
    }

    fn image_task(kind: TaskKind) -> Task {
        Task {
            kind,
            input: TaskInput::Image {
                png: Arc::from(&b"png"[..]),
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
            TaskKind::ImageExplain,
            TaskKind::General,
        ];
        let text_legal = |kind| {
            matches!(
                kind,
                TaskKind::TranslateWord
                    | TaskKind::TranslateSentence
                    | TaskKind::ExplainCode
                    | TaskKind::General
            )
        };
        let image_legal = |kind| matches!(kind, TaskKind::ImageExplain);
        let verdict = |task: &Task| validate_modality(task.kind, &task.input);
        for kind in all_kinds {
            let (text_expect, image_expect) = (
                text_legal(kind)
                    .then_some(())
                    .ok_or(GlossError::UnsupportedModality),
                image_legal(kind)
                    .then_some(())
                    .ok_or(GlossError::UnsupportedModality),
            );
            assert_eq!(verdict(&text_task(kind)), text_expect, "text × {kind:?}");
            assert_eq!(verdict(&image_task(kind)), image_expect, "image × {kind:?}");
        }
    }

    #[test]
    fn task_round_trips_through_serde() {
        let task = Task {
            kind: TaskKind::ExplainCode,
            input: TaskInput::Text {
                text: "fn main() {}".into(),
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
        };
        let json = serde_json::to_string(&input).expect("image input should serialize");
        let back: TaskInput = serde_json::from_str(&json).expect("image input should deserialize");
        assert_eq!(back, input, "bytes must survive the roundtrip");
        assert!(!matches!(&back, TaskInput::Image { png: moved } if Arc::ptr_eq(&png, moved)));
    }
}
