//! 模板资产嵌入：各界面语言一份模板文件集，编译期嵌入、运行时零输入输出
//! （IO）。任务侧一个任务一份自包含模板（词卡、句译、代码解释、图像解读、
//! 通用）：任务说明与纯格式（JSON）输出契约（含一份“中性占位”的输出示例）
//! 都在同一个文件里，改一个任务的任务书不会牵动其它任务；任务书按五段骨架
//! 组织（角色与任务、输出形状、字段规则、任务约定），契约按经注疏与说文解
//! 字的层次组织，与核心层契约模块的三行同构表同源：注（义）恒为首字段，按
//! 类别的疏证字段收敛在疏证对象里——词卡是音标与例句（音、例，字头即选区
//! 原文），句译、讲解、图像解读与通用任务是展开条目，代码另带由大语言模型
//! （LLM）判定的代码语言（界面角标与高亮使用）。
//!
//! 模板里的输出示例就是解析侧（任务校验器）所吃形状的唯一描述，两处改一须
//! 改二（完整性门禁测试锁定：每个模板的示例对象必须通过对应校验）。模板文
//! 本面向模型，用各语言环境的语言书写，不受“日志一律英文”门禁约束（约束
//! 门禁只查日志宏实参）。

use crate::model::Locale;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Templates {
    pub(crate) word_card: &'static str,
    pub(crate) sentence: &'static str,
    pub(crate) code: &'static str,
    pub(crate) image_explain: &'static str,
    pub(crate) general: &'static str,
    pub(crate) target_lang: &'static str,
    pub(crate) discipline: &'static str,
    pub(crate) classify: &'static str,
}

impl Locale {
    pub(crate) fn templates(self) -> Templates {
        match self {
            Locale::Zh => Templates {
                word_card: include_str!("../../../prompts/zh/word_card.md"),
                sentence: include_str!("../../../prompts/zh/sentence.md"),
                code: include_str!("../../../prompts/zh/code.md"),
                image_explain: include_str!("../../../prompts/zh/image_explain.md"),
                general: include_str!("../../../prompts/zh/general.md"),
                target_lang: include_str!("../../../prompts/zh/target_lang.md"),
                discipline: include_str!("../../../prompts/zh/discipline.md"),
                classify: include_str!("../../../prompts/zh/classify.md"),
            },
            Locale::En => Templates {
                word_card: include_str!("../../../prompts/en/word_card.md"),
                sentence: include_str!("../../../prompts/en/sentence.md"),
                code: include_str!("../../../prompts/en/code.md"),
                image_explain: include_str!("../../../prompts/en/image_explain.md"),
                general: include_str!("../../../prompts/en/general.md"),
                target_lang: include_str!("../../../prompts/en/target_lang.md"),
                discipline: include_str!("../../../prompts/en/discipline.md"),
                classify: include_str!("../../../prompts/en/classify.md"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::model::Locale;
    use crate::task::TaskKind;

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
}
