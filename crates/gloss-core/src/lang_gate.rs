//! 输出语言体例门禁：格式契约模块通过后的第二道内容契约。按文本段
//! 角色写一套规则（正文只认目标语言，原文进容器），任务差异收敛为
//! 策略表；违约进语言修复轮，预算耗尽落降级标识。判定器是纯函数：
//! 不读配置、不碰网络、不识别源语言，只消费任务输入做重合比对。
//!
//! 管线：（pulldown-cmark）解析 → 结构节点按任务（markdown）能力判定 →
//! 文本节点切数学定界区、括号组、引文组 → 容器外逐文字段与标点判定 →
//! 原文重合与字段级检查。豁免细则与违约类别到修复动作的映射见各子
//! 模块，设计文档与规则编号的对应关系见记录文件。

mod body;
mod char_map;
mod containers;
mod overlap;
mod simplified;
mod tables;
mod text;
mod violation;

pub use violation::{LangCategory, LangField, LangViolation};

use crate::model::Lang;
use crate::task::{OutcomeStructured, TaskInput, TaskKind, TaskOutcome};

use tables::{LangPolicy, TaskPolicy};

pub fn check(
    kind: TaskKind,
    target: &Lang,
    input: &TaskInput,
    outcome: &TaskOutcome,
) -> Vec<LangViolation> {
    let mut violations = Vec::new();
    let mut bodies: Vec<(LangField, String)> = Vec::new();
    let lang = tables::lang_policy(target);
    let task = tables::task_policy(kind);
    scan_field(
        kind,
        &lang,
        &task,
        LangField::Note,
        &outcome.note,
        &mut violations,
        &mut bodies,
    );
    for (index, example) in outcome.structured.examples().iter().enumerate() {
        scan_field(
            kind,
            &lang,
            &task,
            LangField::Example(index),
            example,
            &mut violations,
            &mut bodies,
        );
    }
    if let OutcomeStructured::WordCard {
        phonetic: Some(phonetic),
        ..
    } = &outcome.structured
        && !phonetic.chars().all(tables::is_phonetic_char)
    {
        violations.push(LangViolation::new(
            LangField::Phonetic,
            LangCategory::PhoneticCharset,
        ));
    }
    if let TaskInput::Text { text } = input {
        overlap::check_overlap(text, &bodies, &mut violations);
    }
    violations.sort_unstable();
    violations.dedup();
    violations
}

#[allow(clippy::too_many_arguments)]
fn scan_field(
    kind: TaskKind,
    lang: &LangPolicy,
    task: &TaskPolicy,
    field: LangField,
    raw: &str,
    violations: &mut Vec<LangViolation>,
    bodies: &mut Vec<(LangField, String)>,
) {
    let parsed = text::parse_field(task.markdown_allowed, raw);
    if parsed.markdown_forbidden {
        violations.push(LangViolation::new(field, LangCategory::MarkdownForbidden));
        return;
    }
    let joined = &parsed.text;
    let math = text::math_ranges(joined);
    let math_mask = span_mask(joined.len(), &math, &[]);
    let scan = containers::scan_containers(&joined.chars, &math_mask, lang, task.quote_container);
    if scan.unclosed || odd_markdown_pairs(task.markdown_allowed, raw) {
        violations.push(LangViolation::new(field, LangCategory::UnclosedContainer));
    }
    let contents: Vec<(usize, usize)> = scan
        .containers
        .iter()
        .map(|container| (container.open + 1, container.close - 1))
        .collect();
    let ranges = body::body_ranges(joined.len(), &math, &contents);
    let body = body::BodyCheck {
        lang,
        task,
        field,
        chars: &joined.chars,
        ranges: &ranges,
    };
    body.run(violations);
    if lang.simplified {
        let mut han_ranges = ranges.clone();
        han_ranges.extend(
            scan.containers
                .iter()
                .filter(|container| !container.is_quote)
                .map(|container| (container.open + 1, container.close - 1)),
        );
        let han_ranges = containers::subtract_math(&han_ranges, &math);
        simplified::check_simplified(field, &joined.chars, &han_ranges, violations);
    }
    let container = containers::ContainerCheck {
        kind,
        policy: lang,
        field,
        chars: &joined.chars,
        math: &math,
    };
    scan.check(&container, violations);
    let overlap = overlap_body(lang, task, &joined.chars, &ranges);
    bodies.push((field, overlap));
}

fn span_mask(len: usize, math: &[(usize, usize)], contents: &[(usize, usize)]) -> Vec<bool> {
    let mut mask = vec![false; len];
    for &(start, end) in math.iter().chain(contents.iter()) {
        for slot in mask.iter_mut().take(end.min(len)).skip(start) {
            *slot = true;
        }
    }
    mask
}

fn odd_markdown_pairs(markdown_allowed: bool, raw: &str) -> bool {
    if !markdown_allowed {
        return false;
    }
    let mut fences = 0usize;
    let mut backticks = 0usize;
    for line in raw.lines() {
        if line.trim_start().starts_with("```") {
            fences += 1;
            continue;
        }
        backticks += line.matches('`').count();
    }
    fences % 2 == 1 || backticks % 2 == 1
}

fn overlap_body(
    lang: &LangPolicy,
    task: &TaskPolicy,
    chars: &[char],
    ranges: &[(usize, usize)],
) -> String {
    let body = body::BodyCheck {
        lang,
        task,
        field: LangField::Note,
        chars,
        ranges,
    };
    let mut out = String::new();
    for &(start, end) in ranges {
        for &c in &chars[start..end] {
            if body.is_delimiter(c) {
                continue;
            }
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Lang;

    fn categories(
        kind: TaskKind,
        target: Lang,
        input: &str,
        note: &str,
        structured: OutcomeStructured,
    ) -> Vec<LangCategory> {
        let input = TaskInput::Text {
            text: input.to_owned(),
        };
        let outcome = TaskOutcome {
            kind,
            note: note.to_owned(),
            structured,
            degraded: false,
            lang_violations: Vec::new(),
        };
        check(kind, &target, &input, &outcome)
            .into_iter()
            .map(|violation| violation.category)
            .collect()
    }

    fn plain(examples: &[&str]) -> OutcomeStructured {
        OutcomeStructured::Plain {
            examples: examples.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    fn word_card(phonetic: Option<&str>, examples: &[&str]) -> OutcomeStructured {
        OutcomeStructured::WordCard {
            phonetic: phonetic.map(str::to_owned),
            examples: examples.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    fn code(examples: &[&str]) -> OutcomeStructured {
        OutcomeStructured::Code {
            examples: examples.iter().map(|s| (*s).to_owned()).collect(),
            code_language: None,
        }
    }

    fn has(violations: &[LangCategory], category: LangCategory) -> bool {
        violations.contains(&category)
    }

    #[test]
    fn foreign_runs_flag_bare_latin_and_kana_but_parenthesized_terms_pass() {
        let zh = Lang::Zh;
        let violations = categories(
            TaskKind::TranslateWord,
            zh.clone(),
            "serendipity",
            "小写外文词 english 混进来了。",
            word_card(None, &[]),
        );
        assert!(has(&violations, LangCategory::ForeignRun));

        for bare in ["要用 API 的地方", "一部 iPhone 而已", "两百 km 的距离"] {
            let violations = categories(
                TaskKind::TranslateWord,
                zh.clone(),
                "gloss",
                bare,
                word_card(None, &[]),
            );
            assert!(has(&violations, LangCategory::ForeignRun), "{bare}");
        }

        let violations = categories(
            TaskKind::TranslateWord,
            zh.clone(),
            "gloss",
            "日语残段テスト混进来了。",
            word_card(None, &[]),
        );
        assert!(has(&violations, LangCategory::ForeignRun));

        let violations = categories(
            TaskKind::TranslateWord,
            zh.clone(),
            "protected",
            "受保护的（protected）的释义。",
            word_card(None, &[]),
        );
        assert!(
            !has(&violations, LangCategory::ForeignRun),
            "a parenthesized term is container content: {violations:?}"
        );
    }

    #[test]
    fn single_latin_letter_passes_as_variable_and_n_dot_flags() {
        let violations = categories(
            TaskKind::TranslateSentence,
            Lang::Zh,
            "hello",
            "变量 x 与 y 的差。",
            plain(&[]),
        );
        assert!(
            !has(&violations, LangCategory::ForeignRun),
            "{violations:?}"
        );

        let violations = categories(
            TaskKind::TranslateWord,
            Lang::Zh,
            "gloss",
            "词性 n. 标注要译成名词。",
            word_card(None, &[]),
        );
        assert!(
            has(&violations, LangCategory::ForeignRun),
            "an n. abbreviation is a bare foreign token: {violations:?}"
        );
    }

    #[test]
    fn punctuation_set_catches_wrong_script_and_passes_cjk_brackets() {
        let violations = categories(
            TaskKind::TranslateWord,
            Lang::Zh,
            "gloss",
            "中文正文里的,英文逗号。",
            word_card(None, &[]),
        );
        assert!(has(&violations, LangCategory::PunctuationMismatch));

        let violations = categories(
            TaskKind::TranslateWord,
            Lang::Zh,
            "gloss",
            "「」在中文集内是合法标点。",
            word_card(None, &[]),
        );
        assert!(
            !has(&violations, LangCategory::PunctuationMismatch),
            "{violations:?}"
        );

        let violations = categories(
            TaskKind::TranslateSentence,
            Lang::En,
            "hello",
            "The English body keeps, its comma.",
            plain(&[]),
        );
        assert!(
            !has(&violations, LangCategory::PunctuationMismatch),
            "{violations:?}"
        );
    }

    #[test]
    fn dash_and_ellipsis_families_stay_style_items_not_violations() {
        for note in [
            "破折号——与——单破折号—都放行。",
            "省略号……与…与...都放行。",
            "半角双连字符 -- 也放行。",
            "小数 3.14 与区间 2020-2024 与比例 50% 都随数字放行。",
            "货币 $5 与 $100 都放行。",
        ] {
            let violations = categories(
                TaskKind::TranslateWord,
                Lang::Zh,
                "gloss",
                note,
                word_card(None, &[]),
            );
            assert!(
                !has(&violations, LangCategory::PunctuationMismatch),
                "{note}: {violations:?}"
            );
        }
    }

    #[test]
    fn traditional_chars_flag_outside_and_pass_inside_quotes() {
        let violations = categories(
            TaskKind::TranslateWord,
            Lang::Zh,
            "gloss",
            "這是繁體字。",
            word_card(None, &[]),
        );
        assert!(has(&violations, LangCategory::TraditionalChar));

        let violations = categories(
            TaskKind::TranslateWord,
            Lang::Zh,
            "gloss",
            "疏证引“這是古籍原文”为据。",
            word_card(None, &[]),
        );
        assert!(
            !has(&violations, LangCategory::TraditionalChar),
            "quote content keeps the traditional original: {violations:?}"
        );
    }

    #[test]
    fn pangu_space_flags_adjacency_only_for_chinese_target() {
        let violations = categories(
            TaskKind::TranslateWord,
            Lang::Zh,
            "gloss",
            "中文word相连要补空格。",
            word_card(None, &[]),
        );
        assert!(has(&violations, LangCategory::MissingSpace));

        let violations = categories(
            TaskKind::TranslateWord,
            Lang::Zh,
            "gloss",
            "中文，word 有标点隔开。",
            word_card(None, &[]),
        );
        assert!(!has(&violations, LangCategory::MissingSpace));

        let violations = categories(
            TaskKind::TranslateWord,
            Lang::Ja,
            "gloss",
            " Japanese 排版不用中欧空格，但拉丁成段仍判。",
            word_card(None, &[]),
        );
        assert!(
            !has(&violations, LangCategory::MissingSpace),
            "pangu only judges the Chinese target: {violations:?}"
        );
    }

    #[test]
    fn emoji_and_variation_selectors_flag_in_body_only() {
        let violations = categories(
            TaskKind::TranslateWord,
            Lang::Zh,
            "gloss",
            "好评👍再见了。",
            word_card(None, &[]),
        );
        assert!(has(&violations, LangCategory::EmojiForbidden));

        let violations = categories(
            TaskKind::TranslateWord,
            Lang::Zh,
            "gloss",
            "变体选择符❤️也在列。",
            word_card(None, &[]),
        );
        assert!(has(&violations, LangCategory::EmojiForbidden));

        let violations = categories(
            TaskKind::TranslateWord,
            Lang::Zh,
            "gloss",
            "裸红心❤不带变体符也判。",
            word_card(None, &[]),
        );
        assert!(has(&violations, LangCategory::EmojiForbidden));
    }

    #[test]
    fn markdown_structure_flags_on_forbidden_kinds_and_passes_on_full_kinds() {
        for kind in [
            TaskKind::TranslateWord,
            TaskKind::TranslateSentence,
            TaskKind::ImageExplain,
        ] {
            for note in [
                "**着重号**也判",
                "`着重号`也判",
                "见 [链接](https://a.b) 判",
            ] {
                let violations = categories(kind, Lang::Zh, "gloss", note, plain(&[]));
                assert!(
                    has(&violations, LangCategory::MarkdownForbidden),
                    "{kind:?}/{note}: {violations:?}"
                );
            }
        }
        let violations = categories(
            TaskKind::ExplainCode,
            Lang::Zh,
            "fn main() {}",
            "# 标题\n\n- 清单\n\n`行内码` 与 [链接](https://a.b) 全量放行。",
            code(&[]),
        );
        assert!(
            !has(&violations, LangCategory::MarkdownForbidden),
            "{violations:?}"
        );
    }

    #[test]
    fn unclosed_containers_flag_stray_parens_quotes_fences_and_backticks() {
        let violations = categories(
            TaskKind::TranslateWord,
            Lang::Zh,
            "gloss",
            "括号没有（闭合完。",
            word_card(None, &[]),
        );
        assert!(has(&violations, LangCategory::UnclosedContainer));

        let violations = categories(
            TaskKind::TranslateWord,
            Lang::Zh,
            "gloss",
            "引号没有“闭合完。",
            word_card(None, &[]),
        );
        assert!(has(&violations, LangCategory::UnclosedContainer));

        let violations = categories(
            TaskKind::ExplainCode,
            Lang::Zh,
            "fn main() {}",
            "```rust\nfn main() {}",
            code(&[]),
        );
        assert!(has(&violations, LangCategory::UnclosedContainer), "fence");

        let violations = categories(
            TaskKind::ExplainCode,
            Lang::Zh,
            "fn main() {}",
            "落单反引号 ` 在正文。",
            code(&[]),
        );
        assert!(
            has(&violations, LangCategory::UnclosedContainer),
            "backtick"
        );

        let violations = categories(
            TaskKind::ExplainCode,
            Lang::Zh,
            "fn main() {}",
            "```rust\nfn main() {}\n```\n成对围栏放行，行内 `code` 也放行。",
            code(&[]),
        );
        assert!(
            !has(&violations, LangCategory::UnclosedContainer),
            "{violations:?}"
        );
    }

    #[test]
    fn isolated_dollar_and_currency_amounts_stay_legal() {
        let violations = categories(
            TaskKind::TranslateWord,
            Lang::Zh,
            "gloss",
            "孤立 $ 与 $5 都放行。",
            word_card(None, &[]),
        );
        assert!(
            !has(&violations, LangCategory::PunctuationMismatch)
                && !has(&violations, LangCategory::UnclosedContainer),
            "{violations:?}"
        );
    }

    #[test]
    fn math_containers_exempt_their_content() {
        let violations = categories(
            TaskKind::TranslateSentence,
            Lang::Zh,
            "hello",
            "公式 $English words$ 与 $x$ 里的内容豁免。",
            plain(&[]),
        );
        assert!(
            !has(&violations, LangCategory::ForeignRun)
                && !has(&violations, LangCategory::PunctuationMismatch),
            "{violations:?}"
        );

        let violations = categories(
            TaskKind::TranslateWord,
            Lang::Zh,
            "gloss",
            "成对公式 $a+b$ 与落单的 $ 都不判。",
            word_card(None, &[]),
        );
        assert!(
            !has(&violations, LangCategory::UnclosedContainer),
            "{violations:?}"
        );
    }

    #[test]
    fn math_inside_containers_stays_exempt() {
        let violations = categories(
            TaskKind::TranslateWord,
            Lang::Zh,
            "gloss",
            "参见（详见 $O(n \\log n)$ 一节）。",
            word_card(None, &[]),
        );
        assert!(
            !has(&violations, LangCategory::ContainerMixing),
            "a formula inside a paren is container content: {violations:?}"
        );

        let violations = categories(
            TaskKind::TranslateWord,
            Lang::Zh,
            "gloss",
            "引文“复杂度为 $O(n)$ 的情形”成立。",
            word_card(None, &[]),
        );
        assert!(
            !has(&violations, LangCategory::ContainerMixing),
            "a formula inside a quote is container content: {violations:?}"
        );
    }

    #[test]
    fn line_wrapped_formulas_stay_math_containers() {
        let violations = categories(
            TaskKind::General,
            Lang::Zh,
            "gloss",
            "计算满足 $a +\nb > 0$ 的情形。",
            plain(&[]),
        );
        assert!(
            !has(&violations, LangCategory::PunctuationMismatch),
            "a formula wrapping a soft break still pairs: {violations:?}"
        );

        let violations = categories(
            TaskKind::General,
            Lang::Zh,
            "gloss",
            "单价为 $x\n\n第二段 テスト$y$ 混排。",
            plain(&[]),
        );
        assert!(
            has(&violations, LangCategory::ForeignRun),
            "a dollar in one paragraph never pairs across the blank line: {violations:?}"
        );
    }

    #[test]
    fn container_mixing_flags_mixed_groups_only() {
        let violations = categories(
            TaskKind::TranslateWord,
            Lang::Zh,
            "gloss",
            "组内混用（中文 english 不行）要判。",
            word_card(None, &[]),
        );
        assert!(has(&violations, LangCategory::ContainerMixing));
    }

    #[test]
    fn container_mixing_passes_single_language_and_excluded_tokens() {
        for (input, note, structured) in [
            (
                "serendipity",
                "“serendipity”（美妙事物）串行放行。",
                word_card(None, &[]) as OutcomeStructured,
            ),
            (
                "API",
                "嵌套（API（应用编程接口））放行。",
                word_card(None, &[]),
            ),
            (
                "gloss",
                "数字与单字母不计入（见 x 与 y 的定义）。",
                word_card(None, &[]),
            ),
            (
                "fn main() {}",
                "代码任务不开引文容器，正文里的 “quoted” 是普通标点。",
                code(&[]),
            ),
        ] {
            let kind = if matches!(structured, OutcomeStructured::Code { .. }) {
                TaskKind::ExplainCode
            } else {
                TaskKind::TranslateWord
            };
            let violations = categories(kind, Lang::Zh, input, note, structured);
            assert!(
                !has(&violations, LangCategory::ContainerMixing),
                "{note}: {violations:?}"
            );
        }
    }

    #[test]
    fn sentence_parens_flag_final_punctuation_only() {
        let violations = categories(
            TaskKind::TranslateSentence,
            Lang::Zh,
            "hello there",
            "短语级括注（hello there）放行。",
            plain(&[]),
        );
        assert!(
            !has(&violations, LangCategory::SentenceInParens),
            "{violations:?}"
        );

        let violations = categories(
            TaskKind::TranslateSentence,
            Lang::Zh,
            "hello there",
            "整句进括号（hello there。）要判。",
            plain(&[]),
        );
        assert!(has(&violations, LangCategory::SentenceInParens));

        let violations = categories(
            TaskKind::TranslateSentence,
            Lang::Zh,
            "hello there",
            "组中部句点（缩略语 Dr.、etc. 都不触发）。",
            plain(&[]),
        );
        assert!(
            !has(&violations, LangCategory::SentenceInParens),
            "{violations:?}"
        );
    }

    #[test]
    fn source_overlap_flags_copied_passages_and_folds_whitespace() {
        let input = "The quick brown fox jumps over the lazy dog near the riverbank";
        let violations = categories(
            TaskKind::TranslateSentence,
            Lang::Zh,
            input,
            "整段照抄要判：The quick brown fox jumps over the lazy dog near the riverbank",
            plain(&[]),
        );
        assert!(has(&violations, LangCategory::SourceOverlap));

        let violations = categories(
            TaskKind::TranslateSentence,
            Lang::Zh,
            "The   quick brown\nfox jumps over",
            "空白差异折叠后仍比中：The quick\nbrown   fox jumps over",
            plain(&[]),
        );
        assert!(has(&violations, LangCategory::SourceOverlap));

        let violations = categories(
            TaskKind::TranslateWord,
            Lang::Zh,
            "serendipity",
            "术语回标（serendipity）低于阈值放行。",
            word_card(None, &[]),
        );
        assert!(!has(&violations, LangCategory::SourceOverlap));
    }

    #[test]
    fn phonetic_charset_limits_word_card_phonetics() {
        let violations = categories(
            TaskKind::TranslateWord,
            Lang::Zh,
            "gloss",
            "义释。",
            word_card(Some("/ɡlɒs/"), &[]),
        );
        assert!(!has(&violations, LangCategory::PhoneticCharset));

        let violations = categories(
            TaskKind::TranslateWord,
            Lang::Zh,
            "光泽",
            "义释。",
            word_card(Some("pīnyīn shēngdiào"), &[]),
        );
        assert!(!has(&violations, LangCategory::PhoneticCharset));

        let violations = categories(
            TaskKind::TranslateWord,
            Lang::Zh,
            "gloss",
            "义释。",
            word_card(Some("汉字出界"), &[]),
        );
        assert!(has(&violations, LangCategory::PhoneticCharset));

        let violations = categories(
            TaskKind::TranslateSentence,
            Lang::Zh,
            "hello",
            "译文。",
            plain(&[]),
        );
        assert!(
            !has(&violations, LangCategory::PhoneticCharset),
            "non-word kinds have no phonetic field"
        );
    }

    #[test]
    fn other_target_skips_script_and_mixing_checks() {
        let violations = categories(
            TaskKind::TranslateWord,
            Lang::Other("西班牙语".into()),
            "hola",
            "Hello 世界 mixed run with hola.",
            word_card(None, &[]),
        );
        assert!(
            !has(&violations, LangCategory::ForeignRun)
                && !has(&violations, LangCategory::ContainerMixing),
            "an unknown target language skips script judgments: {violations:?}"
        );
    }

    #[test]
    fn image_input_skips_overlap_but_runs_the_rest() {
        let input = TaskInput::Image {
            png: std::sync::Arc::from(&b"png"[..]),
        };
        let outcome = TaskOutcome {
            kind: TaskKind::ImageExplain,
            note: "The quick brown fox jumps over the lazy dog near the riverbank".into(),
            structured: plain(&[]),
            degraded: false,
            lang_violations: Vec::new(),
        };
        let violations = check(TaskKind::ImageExplain, &Lang::En, &input, &outcome);
        assert!(
            !violations
                .iter()
                .any(|violation| violation.category == LangCategory::SourceOverlap),
            "image tasks have no text input to overlap: {violations:?}"
        );

        let outcome = TaskOutcome {
            note: "body with 👍 emoji".into(),
            ..outcome
        };
        let violations = check(TaskKind::ImageExplain, &Lang::En, &input, &outcome);
        assert!(
            violations
                .iter()
                .any(|violation| violation.category == LangCategory::EmojiForbidden),
            "{violations:?}"
        );
    }

    #[test]
    fn violation_lines_carry_position_and_rule_id_only() {
        let violation = LangViolation::new(LangField::Example(1), LangCategory::ForeignRun);
        assert_eq!(
            violation.line(),
            "examples[1]: ForeignRun",
            "the line is the field key plus the rule id, language-neutral"
        );
        assert_eq!(violation.to_string(), "ForeignRun at examples[1]");
        let summary = LangViolation::summary(&[
            violation,
            LangViolation::new(LangField::Note, LangCategory::ForeignRun),
            LangViolation::new(LangField::Note, LangCategory::MissingSpace),
        ]);
        assert_eq!(summary, "2 violation kind(s): ForeignRun, MissingSpace");
        for marker in ["选区", "回复原文", "MARKER", "中文word"] {
            assert!(
                !violation.line().contains(marker) && !violation.to_string().contains(marker),
                "violation renderings never quote reply content: {marker}"
            );
        }
    }
}
