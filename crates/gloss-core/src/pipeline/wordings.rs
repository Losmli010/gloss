//! 措辞注册表：修复轮三段、分类类别段、目标语言名与组装片段的措杂数
//! 据。注册表按提示词语言各一份、配置格式（TOML）、编译期嵌入。分区与
//! 键空间：修复骨架一区，收三段共用的头、行、尾、截断注与回指句；格式
//! 违约一区，按违约或细节标识收措辞行（位置加动作）；语言违约一区，按
//! 规则标识收动作；聚合一区，收例句序号连接符（修复轮行清单与降级批注
//! 行共用）；例句位置一区，收单复数两条模板（占位符（{{joined}}））；规则
//! 名一区，按规则标识收界面语短名（降级批注行与语言违约区同册——模型
//! 修什么、用户看什么，同键可对）；分类定义一区，按类别标识收定义，分
//! 类段头一区收类别节的标题词与分隔符；目标语言名一区收两变体的显示
//! 名。键与变体的双向集合相等由完整性测试锁定。
//!
//! 嵌入数据合法性由完整性测试前置锁定（解析成功、键与枚举变体双向
//! 集合相等）；运行时缺键是有痕降级——装载失败整表缺席、查键未中
//! 单键缺席，一律先警告再返回空，由组合层回落固定措辞，不中断运行。
//! 注册表只读不写，装载一次性进静态量。

use std::collections::BTreeMap;
use std::sync::OnceLock;

use crate::log::{thread, warn};
use crate::model::Locale;

const ZH_SOURCE: &str = include_str!("../../prompts/wordings/zh.toml");

const EN_SOURCE: &str = include_str!("../../prompts/wordings/en.toml");

#[derive(Debug, serde::Deserialize)]
pub(super) struct Registry {
    pub(super) repair: RepairSection,
    pub(super) classify: ClassifySection,
    pub(super) violation: ViolationSection,
    pub(super) target_lang: TargetLangSection,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct RepairSection {
    pub(super) frame: FrameWording,
    pub(super) format: BTreeMap<String, RowWording>,
    pub(super) lang: BTreeMap<String, ActionWording>,
    pub(super) aggregate: AggregateWording,
    pub(super) example_position: ExamplePositionWording,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct FrameWording {
    pub(super) header: String,
    pub(super) row: String,
    pub(super) closing: String,
    pub(super) truncated_note: String,
    pub(super) back_reference: String,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct RowWording {
    pub(super) position: String,
    pub(super) action: String,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct ActionWording {
    pub(super) action: String,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct AggregateWording {
    pub(super) joiner: String,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct ExamplePositionWording {
    pub(super) single: String,
    pub(super) plural: String,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct ClassifySection {
    pub(super) category: BTreeMap<String, DefinitionWording>,
    pub(super) section: SectionWording,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct SectionWording {
    pub(super) header: String,
    pub(super) separator: String,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct DefinitionWording {
    pub(super) definition: String,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct ViolationSection {
    pub(super) name: BTreeMap<String, NameWording>,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct NameWording {
    pub(super) name: String,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct TargetLangSection {
    pub(super) zh: String,
    pub(super) en: String,
}

fn source(locale: Locale) -> &'static str {
    match locale {
        Locale::Zh => ZH_SOURCE,
        Locale::En => EN_SOURCE,
    }
}

fn parse(source: &str) -> Option<Registry> {
    match toml::from_str(source) {
        Ok(registry) => Some(registry),
        Err(error) => {
            warn!(
                thread = thread::TOKIO,
                error = %error,
                "prompt wording registry failed to parse, wordings degrade to fixed \
                 fallbacks"
            );
            None
        }
    }
}

pub(super) fn registry(locale: Locale) -> Option<&'static Registry> {
    static ZH: OnceLock<Option<Registry>> = OnceLock::new();
    static EN: OnceLock<Option<Registry>> = OnceLock::new();
    let cell = match locale {
        Locale::Zh => &ZH,
        Locale::En => &EN,
    };
    cell.get_or_init(|| parse(source(locale))).as_ref()
}

pub(super) fn frame(locale: Locale) -> Option<&'static FrameWording> {
    registry(locale).map(|registry| &registry.repair.frame)
}

pub(super) fn format_row(locale: Locale, key: &str) -> Option<&'static RowWording> {
    let registry = registry(locale)?;
    match registry.repair.format.get(key) {
        Some(row) => Some(row),
        None => {
            warn!(
                thread = thread::TOKIO,
                key = key,
                "format wording key missing in the prompt registry, falling back to \
                 the fixed violation wording"
            );
            None
        }
    }
}

pub(super) fn lang_action(locale: Locale, rule_id: &str) -> Option<&'static str> {
    let registry = registry(locale)?;
    match registry.repair.lang.get(rule_id) {
        Some(wording) => Some(&wording.action),
        None => {
            warn!(
                thread = thread::TOKIO,
                key = rule_id,
                "language action key missing in the prompt registry, falling back to \
                 the position and rule id line"
            );
            None
        }
    }
}

pub(super) fn classify_definition(locale: Locale, name: &str) -> Option<&'static str> {
    let registry = registry(locale)?;
    match registry.classify.category.get(name) {
        Some(wording) => Some(&wording.definition),
        None => {
            warn!(
                thread = thread::TOKIO,
                key = name,
                "classify definition key missing in the prompt registry, falling back \
                 to the bare category entry"
            );
            None
        }
    }
}

pub(super) fn violation_name(locale: Locale, rule_id: &str) -> Option<&'static str> {
    let registry = registry(locale)?;
    match registry.violation.name.get(rule_id) {
        Some(wording) => Some(&wording.name),
        None => {
            warn!(
                thread = thread::TOKIO,
                key = rule_id,
                "violation name key missing in the prompt registry, the degraded hint \
                 falls back to the generic wording"
            );
            None
        }
    }
}

pub(super) fn aggregate_joiner(locale: Locale) -> Option<&'static str> {
    registry(locale).map(|registry| registry.repair.aggregate.joiner.as_str())
}

pub(super) fn example_position(locale: Locale) -> Option<&'static ExamplePositionWording> {
    registry(locale).map(|registry| &registry.repair.example_position)
}

pub(super) fn classify_section(locale: Locale) -> Option<&'static SectionWording> {
    registry(locale).map(|registry| &registry.classify.section)
}

pub(super) fn target_lang(locale: Locale) -> Option<&'static TargetLangSection> {
    registry(locale).map(|registry| &registry.target_lang)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang_gate::LangCategory;
    use crate::schema::{CLASSIFY_CATEGORY_NAMES, FormatDetail, Violation};

    const LANG_CATEGORIES: [LangCategory; 11] = [
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

    fn registry_of(locale: Locale) -> &'static Registry {
        registry(locale).expect("the embedded registry must parse")
    }

    fn key_set<'a>(keys: impl Iterator<Item = &'a String>) -> Vec<&'a str> {
        let mut keys: Vec<&str> = keys.map(String::as_str).collect();
        keys.sort_unstable();
        keys
    }

    #[test]
    fn registry_frame_wording_is_present_for_both_locales() {
        for locale in [Locale::Zh, Locale::En] {
            let registry = registry_of(locale);
            let frame = &registry.repair.frame;
            for field in [
                &frame.header,
                &frame.row,
                &frame.closing,
                &frame.truncated_note,
                &frame.back_reference,
            ] {
                assert!(!field.trim().is_empty(), "{locale:?}: empty frame wording");
            }
            assert!(
                !registry.repair.aggregate.joiner.trim().is_empty(),
                "{locale:?}: empty aggregate joiner"
            );
            for position in [
                &registry.repair.example_position.single,
                &registry.repair.example_position.plural,
            ] {
                assert!(
                    position.contains("{{joined}}"),
                    "{locale:?}: the example position must name the joined placeholder: \
                     {position}"
                );
                assert!(
                    render_check(position),
                    "{locale:?}: the example position renders with the joined placeholder"
                );
            }
            let section = &registry.classify.section;
            assert!(
                !section.header.trim().is_empty() && !section.separator.is_empty(),
                "{locale:?}: empty classify section wording"
            );
            assert!(
                !registry.target_lang.zh.trim().is_empty()
                    && !registry.target_lang.en.trim().is_empty(),
                "{locale:?}: empty target language name"
            );
        }
    }

    fn render_check(template: &str) -> bool {
        crate::prompt::render_template(template, &[("joined", "1")]).contains('1')
    }

    #[test]
    fn registry_covers_every_format_key_with_worded_rows() {
        for locale in [Locale::Zh, Locale::En] {
            let format = &registry_of(locale).repair.format;
            let violations = [
                Violation::EmptyReply,
                Violation::NotJson,
                Violation::MissingNote,
                Violation::KindNotAllowed,
            ];
            let details = [
                FormatDetail::Fenced,
                FormatDetail::NotAnObject,
                FormatDetail::Truncated,
                FormatDetail::NoteKeyMissing,
                FormatDetail::NoteNotString,
            ];
            for key in violations
                .into_iter()
                .map(Violation::key)
                .chain(details.into_iter().map(FormatDetail::key))
            {
                let row = format
                    .get(key)
                    .unwrap_or_else(|| panic!("{locale:?}: the wording key {key} is missing"));
                assert!(
                    !row.position.trim().is_empty() && !row.action.trim().is_empty(),
                    "{locale:?}: empty wording row for {key}"
                );
            }
        }
    }

    #[test]
    fn registry_covers_every_language_action_and_classify_definition() {
        for locale in [Locale::Zh, Locale::En] {
            let registry = registry_of(locale);
            for category in LANG_CATEGORIES {
                let rule_id = category.rule_id();
                let wording = registry.repair.lang.get(rule_id).unwrap_or_else(|| {
                    panic!("{locale:?}: the language action {rule_id} is missing")
                });
                assert!(
                    !wording.action.trim().is_empty(),
                    "{locale:?}: empty action for {rule_id}"
                );
                let name = registry.violation.name.get(rule_id).unwrap_or_else(|| {
                    panic!("{locale:?}: the violation name {rule_id} is missing")
                });
                assert!(
                    !name.name.trim().is_empty(),
                    "{locale:?}: empty violation name for {rule_id}"
                );
            }
            for name in CLASSIFY_CATEGORY_NAMES {
                let wording = registry.classify.category.get(name).unwrap_or_else(|| {
                    panic!("{locale:?}: the classify definition {name} is missing")
                });
                assert!(
                    !wording.definition.trim().is_empty(),
                    "{locale:?}: empty definition for {name}"
                );
            }
        }
    }

    fn assert_category_partition<V>(
        locale: Locale,
        label: &str,
        partition: &std::collections::BTreeMap<String, V>,
    ) {
        assert_eq!(
            partition.len(),
            LANG_CATEGORIES.len(),
            "{locale:?}: the {label} partition holds exactly the category rule ids"
        );
        for key in partition.keys() {
            let quoted = format!("\"{key}\"");
            assert!(
                serde_json::from_str::<LangCategory>(&quoted).is_ok(),
                "{locale:?}: {label} key {key:?} maps to no LangCategory variant"
            );
        }
    }

    #[test]
    fn registry_keys_match_the_enums_exactly_in_both_directions() {
        for locale in [Locale::Zh, Locale::En] {
            let registry = registry_of(locale);
            assert_eq!(
                registry.repair.format.len(),
                9,
                "{locale:?}: the format partition holds exactly the four violation-level \
                 and five detail keys"
            );
            for key in registry.repair.format.keys() {
                let quoted = format!("\"{key}\"");
                let known = serde_json::from_str::<Violation>(&quoted).is_ok()
                    || serde_json::from_str::<FormatDetail>(&quoted).is_ok();
                assert!(
                    known,
                    "{locale:?}: format key {key:?} maps to no Violation or FormatDetail \
                     variant"
                );
            }
            assert_category_partition(locale, "lang", &registry.repair.lang);
            assert_category_partition(locale, "violation-name", &registry.violation.name);
            assert_eq!(
                registry.classify.category.len(),
                CLASSIFY_CATEGORY_NAMES.len(),
                "{locale:?}: the classify partition holds exactly the template categories"
            );
            for key in registry.classify.category.keys() {
                assert!(
                    CLASSIFY_CATEGORY_NAMES.contains(&key.as_str()),
                    "{locale:?}: classify key {key:?} is not a template category"
                );
            }
        }
    }

    #[test]
    fn registry_carries_identical_key_sets_across_locales() {
        let zh = registry_of(Locale::Zh);
        let en = registry_of(Locale::En);
        assert_eq!(
            key_set(zh.repair.format.keys()),
            key_set(en.repair.format.keys()),
            "the format partition keys must line up across locales"
        );
        assert_eq!(
            key_set(zh.repair.lang.keys()),
            key_set(en.repair.lang.keys()),
            "the lang partition keys must line up across locales"
        );
        assert_eq!(
            key_set(zh.violation.name.keys()),
            key_set(en.violation.name.keys()),
            "the violation-name partition keys must line up across locales"
        );
        assert_eq!(
            key_set(zh.classify.category.keys()),
            key_set(en.classify.category.keys()),
            "the classify partition keys must line up across locales"
        );
    }

    #[test]
    fn missing_keys_and_broken_sources_degrade_to_none_with_a_trace() {
        let _serial = crate::log::test_support::lock_dispatchers();
        let logs = crate::log::capture(|| {
            assert!(format_row(Locale::Zh, "NoSuchKey").is_none());
            assert!(lang_action(Locale::Zh, "NoSuchRule").is_none());
            assert!(classify_definition(Locale::Zh, "NoSuchCategory").is_none());
            assert!(violation_name(Locale::Zh, "NoSuchRule").is_none());
        });
        assert!(
            logs.contains("falling back"),
            "a missing key must leave a warn trace: {logs}"
        );
        let logs = crate::log::capture(|| {
            assert!(parse("not [ valid toml").is_none());
        });
        assert!(
            logs.contains("failed to parse"),
            "a broken source must leave a warn trace: {logs}"
        );
    }
}
