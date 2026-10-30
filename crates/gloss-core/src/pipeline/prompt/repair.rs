//! 修复轮会话组装与降级批注行：三段修复轮（任务、分类、语言）共用一份
//! 骨架（头、行、尾），任务与分类段的行是违约级或细节措辞行，语言段的行
//! 由聚合、排序与封顶在组合层落成（同字段同类违约合并为一行、按字段序
//! 排序、封顶附截断注），回指句只指称上一条回复的身份。
//!
//! 语言降级的界面批注行与本区同源：公开的批注行清单函数用同一行模板与
//! 同一聚合算法渲染批注行，只换注入的词——位置词与规则名由调用方注入
//! （位置词是界面文案，规则名取注册表规则名区，与语言违约区同册），动作
//! 措辞复用语言违约区原文，超限附注册表截断注；注册表缺席或查键未中得空
//! 清单，由界面回落泛化文案。批注行只描述字段位置与规则，不引回复原文。
//! 人机两册在此分道：修复轮的行给模型，批注行给人，行式与聚合保持单一
//! 事实源。

use super::{ChatMessage, PromptRegistry, render_template};
use crate::lang_gate::{LangCategory, LangField, LangViolation};
use crate::model::Locale;
use crate::schema::{FormatDetail, Violation};
use crate::wordings;

const LANG_REPAIR_ROW_CAP: usize = 12;

impl PromptRegistry {
    pub fn render_repair(
        &self,
        locale: Locale,
        violation: Violation,
        detail: Option<FormatDetail>,
    ) -> String {
        let key = detail
            .map(FormatDetail::key)
            .unwrap_or_else(|| violation.key());
        let frame = wordings::frame(locale);
        let row = repair_row(frame, wordings::format_row(locale, key), violation);
        match frame {
            Some(frame) => repair_instruction(frame, &[row], false),
            None => row,
        }
    }

    pub fn render_language_repair(&self, locale: Locale, violations: &[LangViolation]) -> String {
        let frame = wordings::frame(locale);
        let (rows, truncated) = language_repair_rows(locale, frame, violations);
        let Some(frame) = frame else {
            return rows.join("\n");
        };
        let mut instruction = String::new();
        instruction.push_str(frame.back_reference.trim_end());
        instruction.push('\n');
        instruction.push_str(&repair_instruction(frame, &rows, truncated));
        instruction
    }
}

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

fn repair_instruction(frame: &wordings::FrameWording, rows: &[String], truncated: bool) -> String {
    let mut out = String::new();
    out.push_str(frame.header.trim_end());
    for row in rows {
        out.push('\n');
        out.push_str(row);
    }
    if truncated {
        out.push('\n');
        out.push_str(frame.truncated_note.trim_end());
    }
    out.push('\n');
    out.push_str(frame.closing.trim_end());
    out
}

fn repair_row(
    frame: Option<&wordings::FrameWording>,
    wording: Option<&wordings::RowWording>,
    violation: Violation,
) -> String {
    let Some((frame, wording)) = frame.zip(wording) else {
        return violation.to_string();
    };
    render_template(
        &frame.row,
        &[
            ("position", wording.position.as_str()),
            ("rule", violation.key()),
            ("action", wording.action.as_str()),
        ],
    )
}

struct LangGroup<'a> {
    field: LangField,
    category: LangCategory,
    example_indices: Vec<usize>,
    first: &'a LangViolation,
}

impl LangGroup<'_> {
    fn push(&mut self, violation: &LangViolation) {
        if let LangField::Example(index) = violation.field {
            self.example_indices.push(index);
        }
    }
}

fn aggregate_groups(violations: &[LangViolation]) -> Vec<LangGroup<'_>> {
    let mut sorted: Vec<&LangViolation> = violations.iter().collect();
    sorted.sort_unstable();
    let mut groups: Vec<LangGroup<'_>> = Vec::new();
    for violation in sorted {
        match groups.last_mut() {
            Some(group)
                if group.category == violation.category
                    && same_field_family(group.field, violation.field) =>
            {
                group.push(violation);
            }
            _ => groups.push(LangGroup {
                field: violation.field,
                category: violation.category,
                example_indices: match violation.field {
                    LangField::Example(index) => vec![index],
                    _ => Vec::new(),
                },
                first: violation,
            }),
        }
    }
    groups
}

fn same_field_family(left: LangField, right: LangField) -> bool {
    matches!(
        (left, right),
        (LangField::Note, LangField::Note)
            | (LangField::Phonetic, LangField::Phonetic)
            | (LangField::Example(_), LangField::Example(_))
    )
}

fn joined_example_indices(locale: Locale, group: &LangGroup<'_>) -> String {
    let mut indices: Vec<usize> = group.example_indices.clone();
    indices.sort_unstable();
    let joiner = wordings::aggregate_joiner(locale).unwrap_or(", ");

    indices
        .iter()
        .map(|index| (index + 1).to_string())
        .collect::<Vec<_>>()
        .join(joiner)
}

fn lang_position(locale: Locale, group: &LangGroup<'_>) -> String {
    match group.field {
        LangField::Note => "note".to_owned(),
        LangField::Phonetic => "phonetic".to_owned(),
        LangField::Example(_) => {
            let joined = joined_example_indices(locale, group);
            let position = match group.example_indices.len() {
                1 => wordings::example_position(locale).map(|wording| wording.single.as_str()),
                _ => wordings::example_position(locale).map(|wording| wording.plural.as_str()),
            };
            match position {
                Some(template) => {
                    format!(
                        "examples {}",
                        render_template(template, &[("joined", &joined)])
                    )
                }
                None => format!("examples[{joined}]"),
            }
        }
    }
}

fn group_row(
    locale: Locale,
    frame: Option<&wordings::FrameWording>,
    group: &LangGroup<'_>,
) -> String {
    let rule = group.category.rule_id();
    let Some((frame, action)) = frame.zip(wordings::lang_action(locale, rule)) else {
        return group.first.line();
    };
    render_template(
        &frame.row,
        &[
            ("position", lang_position(locale, group).as_str()),
            ("rule", rule),
            ("action", action),
        ],
    )
}

fn language_repair_rows(
    locale: Locale,
    frame: Option<&wordings::FrameWording>,
    violations: &[LangViolation],
) -> (Vec<String>, bool) {
    let mut rows = Vec::new();
    let mut truncated = false;
    for group in aggregate_groups(violations) {
        if rows.len() == LANG_REPAIR_ROW_CAP {
            truncated = true;
            break;
        }
        rows.push(group_row(locale, frame, &group));
    }
    (rows, truncated)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViolationPositions {
    pub note: String,
    pub phonetic: String,
    pub example: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViolationBrief {
    pub rows: Vec<String>,
    pub truncated: bool,
}

pub fn violation_briefs(
    locale: Locale,
    positions: &ViolationPositions,
    violations: &[LangViolation],
    cap: usize,
) -> ViolationBrief {
    let empty = ViolationBrief {
        rows: Vec::new(),
        truncated: false,
    };
    let Some(frame) = wordings::frame(locale) else {
        return empty;
    };
    let mut rows = Vec::new();
    let mut truncated = false;
    for group in aggregate_groups(violations) {
        if rows.len() == cap {
            truncated = true;
            break;
        }
        let Some(row) = violation_row(locale, frame, positions, &group) else {
            return empty;
        };
        rows.push(row);
    }
    if truncated {
        rows.push(frame.truncated_note.trim_end().to_owned());
    }
    ViolationBrief { rows, truncated }
}

fn violation_row(
    locale: Locale,
    frame: &wordings::FrameWording,
    positions: &ViolationPositions,
    group: &LangGroup<'_>,
) -> Option<String> {
    let rule = group.category.rule_id();
    let name = wordings::violation_name(locale, rule)?;
    let action = wordings::lang_action(locale, rule)?;
    let position = match group.field {
        LangField::Note => positions.note.clone(),
        LangField::Phonetic => positions.phonetic.clone(),
        LangField::Example(_) => positions
            .example
            .replace("{{n}}", &joined_example_indices(locale, group)),
    };
    Some(render_template(
        &frame.row,
        &[
            ("position", position.as_str()),
            ("rule", name),
            ("action", action),
        ],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang_gate::LangCategory;
    use crate::prompt::Role;

    const ALL_LANG_CATEGORIES: [LangCategory; 11] = [
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

    fn brief_positions(locale: Locale) -> super::ViolationPositions {
        match locale {
            Locale::Zh => super::ViolationPositions {
                note: "注".into(),
                phonetic: "音标".into(),
                example: "疏·例句 {{n}}".into(),
            },
            Locale::En => super::ViolationPositions {
                note: "NOTE".into(),
                phonetic: "Phonetics".into(),
                example: "EXP item {{n}}".into(),
            },
        }
    }

    #[test]
    fn repair_instruction_renders_without_leftover_placeholders() {
        let registry = PromptRegistry::new();
        let details = [
            None,
            Some(FormatDetail::Fenced),
            Some(FormatDetail::NotAnObject),
            Some(FormatDetail::Truncated),
            Some(FormatDetail::NoteKeyMissing),
            Some(FormatDetail::NoteNotString),
        ];
        for locale in [Locale::Zh, Locale::En] {
            for violation in [
                Violation::EmptyReply,
                Violation::NotJson,
                Violation::MissingNote,
                Violation::KindNotAllowed,
            ] {
                for detail in details {
                    let instruction = registry.render_repair(locale, violation, detail);
                    assert!(
                        !instruction.contains("{{"),
                        "{locale:?} x {violation:?} x {detail:?} leaves a placeholder: \
                         {instruction}"
                    );
                    assert!(
                        !instruction.is_empty(),
                        "{locale:?} x {violation:?} x {detail:?}: the wording keeps the \
                         instruction non-empty"
                    );
                    assert!(
                        instruction.to_lowercase().contains("json"),
                        "{locale:?} x {violation:?} x {detail:?}: the repair prompt must \
                         mention json (the endpoint's JSON-mode gate reads it): {instruction}"
                    );
                }
            }
        }
    }

    const DETAIL_WORDING_CASES: [(FormatDetail, &str, &str, &str, &str); 5] = [
        (
            FormatDetail::Fenced,
            "回复被代码围栏包裹",
            "去掉围栏",
            "wrapped in a code fence",
            "Remove the fence",
        ),
        (
            FormatDetail::NotAnObject,
            "不是对象",
            "改为以花括号开头结尾的单个对象",
            "not an object",
            "single object",
        ),
        (
            FormatDetail::Truncated,
            "对象未闭合",
            "精简内容后完整输出",
            "not closed",
            "Trim the content",
        ),
        (
            FormatDetail::NoteKeyMissing,
            "缺少 `note` 键",
            "补上 `note` 键",
            "missing the `note` key",
            "Add the `note` key",
        ),
        (
            FormatDetail::NoteNotString,
            "不是非空字符串",
            "改为非空字符串",
            "non-empty string",
            "non-empty string",
        ),
    ];

    #[test]
    fn repair_instruction_composes_the_detail_wording_onto_the_skeleton() {
        let registry = PromptRegistry::new();
        for locale in [Locale::Zh, Locale::En] {
            for (detail, zh_position, zh_action, en_position, en_action) in DETAIL_WORDING_CASES {
                let instruction = registry.render_repair(locale, Violation::NotJson, Some(detail));
                let (needle, action) = match locale {
                    Locale::Zh => (zh_position, zh_action),
                    Locale::En => (en_position, en_action),
                };
                assert!(
                    instruction.contains(needle) && instruction.contains(action),
                    "{locale:?} x {detail:?}: the detail position and action ride the \
                     instruction: {instruction}"
                );
                assert!(
                    instruction.contains("NotJson"),
                    "{locale:?} x {detail:?}: the row names the violated rule id: \
                     {instruction}"
                );
                for marker in ["MARKER", "{\"note\":\"", "选区原文"] {
                    assert!(
                        !instruction.contains(marker),
                        "{locale:?} x {detail:?}: the detail wording carries structural \
                         facts only, never reply content ({marker}): {instruction}"
                    );
                }
            }
        }
    }

    #[test]
    fn repair_instruction_without_detail_rides_the_violation_level_row() {
        let registry = PromptRegistry::new();
        for locale in [Locale::Zh, Locale::En] {
            let prose = registry.render_repair(Locale::Zh, Violation::NotJson, None);
            assert!(
                prose.contains("回复不是裸（JSON）对象：NotJson——"),
                "the violation-level row carries position, rule id and action: {prose}"
            );
            let en = registry.render_repair(Locale::En, Violation::NotJson, None);
            assert!(
                en.contains("not a bare JSON object: NotJson — "),
                "the violation-level row carries position, rule id and action: {en}"
            );
            for violation in [Violation::EmptyReply, Violation::KindNotAllowed] {
                let instruction = registry.render_repair(locale, violation, None);
                assert!(
                    instruction.contains(violation.key()),
                    "{locale:?} x {violation:?}: every violation maps into the instruction: \
                     {instruction}"
                );
                assert!(
                    !instruction.contains("{{"),
                    "{locale:?} x {violation:?}: no leftover placeholders: {instruction}"
                );
            }
        }
        let zh = registry.render_repair(Locale::Zh, Violation::KindNotAllowed, None);
        assert!(
            !zh.contains("{\"kind\"") && !zh.contains("{\"note\""),
            "the repair instruction no longer quotes a contract schema line: {zh}"
        );
    }

    #[test]
    fn repair_row_falls_back_to_the_display_wording_when_the_registry_is_missing() {
        let violation = Violation::NotJson;
        assert_eq!(
            repair_row(None, None, violation),
            violation.to_string(),
            "without the frame the row degrades to the fixed violation wording"
        );
        let frame = crate::wordings::frame(Locale::Zh).expect("the embedded registry parses");
        assert_eq!(
            repair_row(Some(frame), None, violation),
            violation.to_string(),
            "a missing wording key degrades to the fixed violation wording"
        );
    }

    #[test]
    fn language_repair_instruction_carries_the_row_list_only() {
        use crate::lang_gate::{LangField, LangViolation};

        let registry = PromptRegistry::new();
        let violations = [
            LangViolation::new(LangField::Note, LangCategory::MissingSpace),
            LangViolation::new(LangField::Example(1), LangCategory::ForeignRun),
        ];
        for locale in [Locale::Zh, Locale::En] {
            let instruction = registry.render_language_repair(locale, &violations);
            assert!(
                !instruction.contains("{{"),
                "{locale:?} leaves a placeholder: {instruction}"
            );
            assert!(
                instruction.contains("ForeignRun") && instruction.contains("MissingSpace"),
                "every rule id rides the list: {instruction}"
            );
            assert!(
                !instruction.contains("中文word"),
                "the instruction carries positions and rule ids, never reply text: {instruction}"
            );
            assert!(
                instruction.to_lowercase().contains("json"),
                "the language repair prompt must mention json (the endpoint's JSON-mode gate \
                 reads it): {instruction}"
            );
        }
        let zh = registry.render_language_repair(Locale::Zh, &violations);
        assert!(
            zh.starts_with("你在上一条回复中的输出即反例。"),
            "the back reference names the previous reply's identity only: {zh}"
        );
        assert!(
            zh.contains("note：MissingSpace——"),
            "the row format is field position, rule id, action: {zh}"
        );
        assert!(
            zh.contains("examples 第 2 条：ForeignRun——"),
            "the row format is field position, rule id, action: {zh}"
        );
        let en = registry.render_language_repair(Locale::En, &violations);
        assert!(
            en.starts_with("The output in your previous reply is the counterexample."),
            "the back reference names the previous reply's identity only: {en}"
        );
        assert!(en.contains("examples item 2: ForeignRun — "), "{en}");
    }

    #[test]
    fn language_repair_aggregates_same_field_and_category_and_sorts_by_field_order() {
        use crate::lang_gate::{LangField, LangViolation};

        let registry = PromptRegistry::new();
        let violations = [
            LangViolation::new(LangField::Example(4), LangCategory::ForeignRun),
            LangViolation::new(LangField::Note, LangCategory::MissingSpace),
            LangViolation::new(LangField::Example(1), LangCategory::ForeignRun),
            LangViolation::new(LangField::Phonetic, LangCategory::PhoneticCharset),
            LangViolation::new(LangField::Example(6), LangCategory::ForeignRun),
            LangViolation::new(LangField::Note, LangCategory::ForeignRun),
        ];
        let zh = registry.render_language_repair(Locale::Zh, &violations);
        let examples_row = zh
            .find("examples 第 2、5、7 条：ForeignRun")
            .unwrap_or_else(|| {
                panic!(
                    "same-field same-category rows merge with sorted \
                                       indices: {zh}"
                )
            });
        let phonetic_row = zh
            .find("phonetic：PhoneticCharset")
            .unwrap_or_else(|| panic!("every rule id survives the aggregation: {zh}"));
        let note_foreign_row = zh
            .find("note：ForeignRun——")
            .unwrap_or_else(|| panic!("the note field keeps its own row: {zh}"));
        let note_missing_row = zh
            .find("note：MissingSpace——")
            .unwrap_or_else(|| panic!("the note field keeps its own row: {zh}"));
        assert!(
            note_foreign_row < phonetic_row
                && phonetic_row < examples_row
                && note_missing_row < phonetic_row,
            "rows follow the schema field order (note, phonetic, examples): {zh}"
        );
        let en = registry.render_language_repair(Locale::En, &violations);
        assert!(
            en.contains("examples items 2, 5, 7: ForeignRun — "),
            "merged indices ride the localized position: {en}"
        );
    }

    #[test]
    fn language_repair_caps_rows_and_appends_the_truncated_note() {
        use crate::lang_gate::{LangField, LangViolation};

        let registry = PromptRegistry::new();
        let mut over: Vec<LangViolation> = ALL_LANG_CATEGORIES
            .map(|category| LangViolation::new(LangField::Note, category))
            .into_iter()
            .collect();
        over.push(LangViolation::new(
            LangField::Phonetic,
            LangCategory::PhoneticCharset,
        ));
        over.push(LangViolation::new(
            LangField::Example(0),
            LangCategory::ForeignRun,
        ));
        let zh = registry.render_language_repair(Locale::Zh, &over);
        let rows = zh
            .lines()
            .skip_while(|line| !line.contains("违约清单如下"))
            .skip(1)
            .take_while(|line| !line.contains("请逐条修正后") && !line.contains("其余为同类违约"))
            .count();
        assert_eq!(
            rows,
            super::LANG_REPAIR_ROW_CAP,
            "the list caps at the budget after aggregation: {zh}"
        );
        assert!(
            zh.contains("其余为同类违约，一并修正。"),
            "the truncated note rides the capped list: {zh}"
        );
        assert!(
            !zh.contains("examples 第 1 条：ForeignRun"),
            "the capped-off group is dropped, not silently overflowed: {zh}"
        );
    }

    #[test]
    fn language_repair_within_the_cap_carries_no_truncated_note() {
        use crate::lang_gate::{LangField, LangViolation};

        let registry = PromptRegistry::new();
        let mut within: Vec<LangViolation> = ALL_LANG_CATEGORIES
            .map(|category| LangViolation::new(LangField::Note, category))
            .into_iter()
            .collect();
        within.push(LangViolation::new(
            LangField::Phonetic,
            LangCategory::PhoneticCharset,
        ));
        let zh = registry.render_language_repair(Locale::Zh, &within);
        assert!(
            !zh.contains("其余为同类违约"),
            "a list within the cap carries no truncated note: {zh}"
        );
    }

    #[test]
    fn language_repair_degrades_to_position_and_rule_id_lines_without_the_registry() {
        use crate::lang_gate::{LangField, LangViolation};

        let violations = [
            LangViolation::new(LangField::Example(0), LangCategory::ForeignRun),
            LangViolation::new(LangField::Note, LangCategory::MissingSpace),
        ];
        let (rows, truncated) = super::language_repair_rows(Locale::Zh, None, &violations);
        assert!(!truncated);
        assert_eq!(
            rows,
            vec![
                "note: MissingSpace".to_owned(),
                "examples[0]: ForeignRun".to_owned(),
            ],
            "without the frame each group degrades to the position and rule id line"
        );
    }

    #[test]
    fn violation_briefs_render_human_position_and_rule_names_on_the_shared_row() {
        use crate::lang_gate::{LangField, LangViolation};

        let violations = [
            LangViolation::new(LangField::Note, LangCategory::MissingSpace),
            LangViolation::new(LangField::Example(1), LangCategory::ForeignRun),
            LangViolation::new(LangField::Example(4), LangCategory::ForeignRun),
        ];
        let zh = super::violation_briefs(Locale::Zh, &brief_positions(Locale::Zh), &violations, 12);
        assert_eq!(
            zh.rows.len(),
            2,
            "same-field same-category violations merge into one row: {:?}",
            zh.rows
        );
        assert!(
            zh.rows[0].starts_with("注：缺空格——"),
            "the row keeps the three-part shape with human words: {:?}",
            zh.rows
        );
        assert!(
            zh.rows[0].contains("在中文与西文、数字之间补一个半角空格"),
            "the action rides the registry wording verbatim: {:?}",
            zh.rows
        );
        assert!(
            zh.rows[1].starts_with("疏·例句 2、5：夹外文——"),
            "the example position fills the placeholder with the merged indices: {:?}",
            zh.rows
        );
        assert!(
            zh.rows[1].contains("将外文词移入括号并补目标语译名"),
            "the action rides the registry wording verbatim: {:?}",
            zh.rows
        );
        assert!(!zh.truncated);

        let en = super::violation_briefs(Locale::En, &brief_positions(Locale::En), &violations, 12);
        assert!(
            en.rows[0].starts_with("NOTE: spacing — "),
            "the human rule name rides the en registry: {:?}",
            en.rows
        );
        assert!(
            en.rows[1].starts_with("EXP item 2, 5: foreign text — "),
            "merged indices ride the localized position: {:?}",
            en.rows
        );
    }

    #[test]
    fn violation_briefs_carry_structural_facts_only_without_leftover_placeholders() {
        use crate::lang_gate::{LangField, LangViolation};

        let violations = [
            LangViolation::new(LangField::Note, LangCategory::ForeignRun),
            LangViolation::new(LangField::Phonetic, LangCategory::PhoneticCharset),
            LangViolation::new(LangField::Example(0), LangCategory::TraditionalChar),
        ];
        for locale in [Locale::Zh, Locale::En] {
            let briefs = super::violation_briefs(locale, &brief_positions(locale), &violations, 12);
            assert_eq!(briefs.rows.len(), 3, "{locale:?}");
            for row in &briefs.rows {
                assert!(
                    !row.contains("{{"),
                    "{locale:?}: no placeholder survives the render: {row}"
                );
                for marker in ["MARKER", "中文word", "{\"note\""] {
                    assert!(
                        !row.contains(marker),
                        "{locale:?}: the brief carries positions, rule names and actions \
                         only, never reply content ({marker}): {row}"
                    );
                }
            }
        }
    }

    #[test]
    fn violation_briefs_cap_rows_and_append_the_truncated_note() {
        use crate::lang_gate::{LangField, LangViolation};

        let mut violations: Vec<LangViolation> = ALL_LANG_CATEGORIES
            .map(|category| LangViolation::new(LangField::Note, category))
            .into_iter()
            .collect();
        violations.push(LangViolation::new(
            LangField::Phonetic,
            LangCategory::PhoneticCharset,
        ));
        let zh = super::violation_briefs(Locale::Zh, &brief_positions(Locale::Zh), &violations, 5);
        assert_eq!(
            zh.rows.len(),
            6,
            "the cap holds five rows and the truncated note rides as the last row: {:?}",
            zh.rows
        );
        assert!(zh.truncated);
        assert_eq!(
            zh.rows[5], "其余为同类违约，一并修正。",
            "the truncated note is the registry wording, not a ui copy: {:?}",
            zh.rows
        );
        assert!(
            !zh.rows.join("\n").contains("记号未闭合"),
            "the capped-off group is dropped, not silently overflowed: {:?}",
            zh.rows
        );

        let within =
            super::violation_briefs(Locale::Zh, &brief_positions(Locale::Zh), &violations, 12);
        assert_eq!(
            within.rows.len(),
            12,
            "a list within the cap carries every row without the note: {:?}",
            within.rows
        );
        assert!(!within.truncated);
    }

    #[test]
    fn violation_briefs_of_an_empty_list_stay_empty_without_a_truncated_note() {
        let briefs = super::violation_briefs(Locale::Zh, &brief_positions(Locale::Zh), &[], 5);
        assert!(briefs.rows.is_empty());
        assert!(!briefs.truncated);
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
}
