//! 语言违约的聚合与降级批注行：聚合算法与例句序号拼装是修复轮行清单与
//! 降级批注行的单一事实源（同字段同类并一行、按字段序排序、例句序号升序
//! 合并）。批注行清单在此收口：人读位置词与规则名由调用方注入——位置词
//! 是界面文案，核心只做填充；规则名取措辞注册表规则名区（与动作同册），
//! 动作措辞与截断注复用注册表原文；注册表缺席或查键未中得空清单，由界面
//! 回落泛化文案。批注行只描述字段位置与规则，不引回复原文。

use crate::lang_gate::{LangCategory, LangField, LangViolation};
use crate::model::Locale;
use crate::wordings;

pub(crate) struct LangGroup<'a> {
    pub(crate) field: LangField,
    pub(crate) category: LangCategory,
    pub(crate) example_indices: Vec<usize>,
    pub(crate) first: &'a LangViolation,
}

impl LangGroup<'_> {
    fn push(&mut self, violation: &LangViolation) {
        if let LangField::Example(index) = violation.field {
            self.example_indices.push(index);
        }
    }
}

pub(super) fn aggregate_groups(violations: &[LangViolation]) -> Vec<LangGroup<'_>> {
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

pub(super) fn joined_example_indices(locale: Locale, group: &LangGroup<'_>) -> String {
    let mut indices: Vec<usize> = group.example_indices.clone();
    indices.sort_unstable();
    let joiner = if locale == Locale::Zh { "、" } else { ", " }; // i18n:allow 序号连接符随聚合算法住核心库（与修复轮行清单共用一处），非界面词条

    indices
        .iter()
        .map(|index| (index + 1).to_string())
        .collect::<Vec<_>>()
        .join(joiner)
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
    Some(crate::prompt::render_template(
        &frame.row,
        &[
            ("position", position.as_str()),
            ("rule", name),
            ("action", action),
        ],
    ))
}
