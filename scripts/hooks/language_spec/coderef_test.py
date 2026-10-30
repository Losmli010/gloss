"""代码引用规则的单测：结构性记号豁免、代码段残留判罚与标记语言遍历。

被测入口是行级判定与文档遍历函数，直接传路径、行号与文本，返回带规则标识
的违规条目，不落进程；断言取条目消息核对判罚片段；用法见注释门禁与提交信
息门禁的单测入口。
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parents[1]))

import gate_report  # noqa: E402

from language_spec.coderef import (  # noqa: E402
    MARKER,
    markdown_problems,
    prose_line_problems,
)


def messages(findings: list[gate_report.Finding]) -> list[str]:
    return [finding.message for finding in findings]


def test_backticked_identifier_exempt() -> None:
    assert prose_line_problems("t", 1, "改动见 `docs/tests/bdd.md`") == []


def test_bare_identifier_flagged() -> None:
    problems = prose_line_problems("t", 1, "改动见 docs/tests/bdd.md")
    assert any("outside the derived whitelist" in p for p in messages(problems))


def test_unpaired_backtick_flagged() -> None:
    problems = prose_line_problems("t", 1, "标题 ` 残缺")
    assert any("code segment" in p for p in messages(problems))


def test_pure_identifier_exempt() -> None:
    assert prose_line_problems("t", 1, "some_test_name") == []


def test_backtick_span_exempts_traditional_glyphs() -> None:
    # 简体判定的豁免仅代码引用：反引号内的繁体字形不判（提交信息等非标记面）
    assert prose_line_problems("t", 1, "对照 `軟體` 术语。") == []


def test_english_line_flagged() -> None:
    problems = prose_line_problems("t", 1, "Just run the tests.")
    assert any("must be written in Chinese" in p for p in messages(problems))


def test_list_and_task_markers_exempt() -> None:
    assert prose_line_problems("t", 1, "- [x] 已完成的条目") == []


def test_bold_and_date_exempt() -> None:
    assert prose_line_problems("t", 1, "**克制**——2026-09-19 起。") == []


def test_link_and_badge_exempt() -> None:
    assert prose_line_problems("t", 1, "[![CI](a.svg)](b)") == []


def test_allow_marker_regex() -> None:
    assert MARKER.search("let x = 1; # secrets:allow")


def test_markdown_fence_exempt() -> None:
    src = "# 标题\n\n```sh\njust test\n```\n\n中文正文。\n"
    assert markdown_problems("d.md", src) == []


def test_markdown_bare_word_flagged() -> None:
    problems = markdown_problems("d.md", "中文正文带 generic 词。\n")
    assert any("outside the derived whitelist" in p for p in messages(problems))


def test_markdown_table_first_cell_exempt() -> None:
    src = "| some_test_name | 中文场景 |\n| --- | --- |\n| other_name | generic 词 |\n"
    problems = markdown_problems("d.md", src)
    assert any("outside the derived whitelist" in p for p in messages(problems))


def rules(findings: list[gate_report.Finding]) -> list[str]:
    return [finding.rule for finding in findings]


def test_heading_annotation_tail_strips_to_test_name() -> None:
    src = "# some_test_name（chinese_test）\n\n中文正文。\n"
    assert markdown_problems("d.md", src) == []


def test_heading_unclosed_annotation_still_flagged() -> None:
    problems = markdown_problems("d.md", "# some_test_name（chinese_test\n")
    assert "PROSE-LANGUAGE" in rules(problems)


# ---- 围栏内整行注释（文档检查子项）----


def test_markdown_bash_fence_chinese_comment_allowed() -> None:
    src = "```bash\n# 按改动面挑选，完整清单看任务配方文件（justfile）。\njust fmt\n```\n"
    assert markdown_problems("d.md", src) == []


def test_markdown_bash_fence_english_comment_flagged() -> None:
    problems = markdown_problems("d.md", "```sh\n# pick by change surface\njust fmt\n```\n")
    assert rules(problems) == ["COMMENT-DOC-CODE-LANGUAGE"]
    assert problems[0].line == 2


def test_markdown_rust_fence_english_comment_flagged() -> None:
    problems = markdown_problems("d.md", "```rust\n// english tail comment\nlet x = 1;\n```\n")
    assert rules(problems) == ["COMMENT-DOC-CODE-LANGUAGE"]


def test_markdown_fence_foreign_word_flagged() -> None:
    src = "```bash\n# 按改动面挑选，完整配方见 justfile。\n```\n"
    problems = markdown_problems("d.md", src)
    assert any("outside the derived whitelist" in m for m in messages(problems))


def test_markdown_fence_backtick_comment_flagged() -> None:
    problems = markdown_problems("d.md", "```bash\n# 参考 `just fmt` 运行。\n```\n")
    assert rules(problems) == ["COMMENT-DOC-CODE-CODEREF"]


def test_markdown_unknown_fence_language_not_checked() -> None:
    src = "```\n# english line in plain fence\n```\n\n```text\n# another english line\n```\n"
    assert markdown_problems("d.md", src) == []


def test_markdown_fence_trailing_comment_not_checked() -> None:
    assert markdown_problems("d.md", "```sh\njust test -- core # 尾注不收\n```\n") == []


def test_markdown_fence_command_comment_allowed() -> None:
    assert (
        markdown_problems("d.md", "```sh\n# python3 scripts/hooks/check.py <repo-root>\n```\n")
        == []
    )


def test_markdown_fence_safety_comment_allowed() -> None:
    assert (
        markdown_problems("d.md", "```rust\n// SAFETY: the pointer outlives the call\n```\n") == []
    )


def test_markdown_fence_bare_marker_flagged() -> None:
    problems = markdown_problems("d.md", "```sh\n# secrets:allow\n```\n")
    assert rules(problems) == ["ALLOW-REASON"]


def test_markdown_fence_shebang_line_not_comment() -> None:
    assert markdown_problems("d.md", "```python\n#!/usr/bin/env python3\n```\n") == []


def test_markdown_rust_doc_comment_allowed() -> None:
    src = "```rust\n/// 中文文档示例说明。\n//! 中文内部文档示例。\nlet _ = 1;\n```\n"
    assert markdown_problems("d.md", src) == []


def test_markdown_html_fence_comment_checked() -> None:
    problems = markdown_problems("d.md", "```html\n<!-- english note -->\n<div></div>\n```\n")
    assert rules(problems) == ["COMMENT-DOC-CODE-LANGUAGE"]
    assert markdown_problems("d.md", "```html\n<!-- 中文注释 -->\n```\n") == []
    assert markdown_problems("d.md", "```html\n<!-- 跨行未闭合的注释\n```\n") == []


def test_markdown_fence_info_with_title_parses_language() -> None:
    src = '```bash title="demo"\n# english note\n```\n'
    assert any(p.rule == "COMMENT-DOC-CODE-LANGUAGE" for p in markdown_problems("d.md", src))


def test_markdown_braced_fence_info_is_unregistered_language() -> None:
    # 花括号信息串取不到语言名，落进未登记语言盲区，围栏内容不检
    assert markdown_problems("d.md", "```{rust}\n// 中文围栏注释说明。\n```\n") == []


# ---- 解析树遍历的块级豁免 ----


def test_markdown_indented_code_block_exempt() -> None:
    src = "中文正文。\n\n    generic word in code\n    another line\n"
    assert markdown_problems("d.md", src) == []


def test_markdown_tilde_fence_exempt() -> None:
    src = "中文正文。\n\n~~~\ngeneric word\n~~~\n"
    assert markdown_problems("d.md", src) == []


def test_markdown_tilde_fence_comment_checked() -> None:
    problems = markdown_problems("d.md", "~~~sh\n# pick by change surface\n~~~\n")
    assert rules(problems) == ["COMMENT-DOC-CODE-LANGUAGE"]
    assert problems[0].line == 2


def test_markdown_reference_definition_exempt() -> None:
    assert markdown_problems("d.md", "[引用定义]: https://example.com\n") == []


def test_markdown_html_block_exempt() -> None:
    src = "中文正文。\n\n<div>\nenglish note inside html\n</div>\n"
    assert markdown_problems("d.md", src) == []


def test_markdown_math_block_and_span_exempt() -> None:
    src = "中文正文。\n\n$$\nE = mc^2\n$$\n\n能耗按 $E = mc^2$ 折算。\n"
    assert markdown_problems("d.md", src) == []


def test_markdown_single_dollar_text_not_exempt() -> None:
    problems = markdown_problems("d.md", "价格 $5 与 $8 不等。\n")
    assert any("outside the derived whitelist" in m for m in messages(problems))


def test_markdown_setext_heading_judged() -> None:
    problems = markdown_problems("d.md", "中文标题\n===\n\n正文带 generic 词。\n")
    assert problems[0].line == 4


def test_markdown_softbreak_paragraph_judged_per_line() -> None:
    problems = markdown_problems("d.md", "第一行中文。\n第二行 generic 词。\n")
    assert problems[0].line == 2


def test_markdown_unclosed_registered_fence_still_checked() -> None:
    problems = markdown_problems("d.md", "```sh\n# pick by change surface\n")
    assert rules(problems) == ["COMMENT-DOC-CODE-LANGUAGE"]


def test_markdown_math_span_in_prose_exempt() -> None:
    assert prose_line_problems("t", 1, "满足 $a^2 + b^2 = c^2$ 即勾股定理。") == []


def test_markdown_unicode_line_separator_keeps_line_model() -> None:
    src = "中文。\u2028\n正文段有 generic 词。\n```\nencoded\n```\n"
    problems = markdown_problems("d.md", src)
    assert {p.line for p in problems} == {2}


def test_markdown_blockquote_fence_comment_checked() -> None:
    problems = markdown_problems("d.md", "> ```sh\n> # pick by change surface\n> ```\n")
    assert rules(problems) == ["COMMENT-DOC-CODE-LANGUAGE"]


def test_markdown_bash_fence_math_span_allowed() -> None:
    assert markdown_problems("d.md", "```bash\n# 满足 $r^2 >= 0$ 即通过。\n```\n") == []


def test_math_double_dollar_with_chinese_not_exempt() -> None:
    problems = prose_line_problems("t", 1, "记作 $$中文混排$$ 即散文。")
    assert any("outside the derived whitelist" in m for m in messages(problems))


# ---- 模板占位符段豁免 ----


def test_placeholder_span_exempt_in_prose() -> None:
    assert prose_line_problems("t", 1, "正文用{{target}}书写，逐条修正 {{violations}} 清单。") == []


def test_whole_line_placeholder_exempt() -> None:
    assert prose_line_problems("t", 1, "{{violations}}") == []


def test_placeholder_with_prose_still_checked() -> None:
    problems = prose_line_problems("t", 1, "正文用{{target}}书写带 generic 词。")
    assert any("outside the derived whitelist" in m for m in messages(problems))


def test_placeholder_with_spaces_not_exempt() -> None:
    problems = prose_line_problems("t", 1, "正文用 {{ some value }} 书写。")
    assert any("outside the derived whitelist" in m for m in messages(problems))


# ---- 表述区路由：英文区判据由使用方按路径分派 ----


def test_english_zone_prose_checked_as_english() -> None:
    assert prose_line_problems("t", 1, "Reply in English only.", zone="english") == []
    problems = prose_line_problems("t", 1, "Reply in 英文 only.", zone="english")
    assert any("pure English" in m for m in messages(problems))


def test_english_zone_keeps_structural_rules() -> None:
    problems = prose_line_problems("t", 1, "use ` note here", zone="english")
    assert any("code segment" in m for m in messages(problems))


def test_markdown_english_zone_routes_lines() -> None:
    src = "### Output Shape\n\nOutput a single JSON object.\n"
    assert markdown_problems("d.md", src, zone="english") == []
    problems = markdown_problems("d.md", "Output a 英文 object.\n", zone="english")
    assert any("pure English" in m for m in messages(problems))


# ---- 行内反引号内容只许英文 ----


def test_markdown_inline_code_chinese_flagged() -> None:
    problems = markdown_problems("d.md", "使用 `快速取词` 功能。\n")
    assert rules(problems) == ["COMMENT-DOC-CODE-ENGLISH"]
    assert problems[0].line == 1


def test_markdown_inline_code_fullwidth_flagged() -> None:
    problems = markdown_problems("d.md", "键 `（注）` 与 `allowed` 同行。\n")
    assert rules(problems) == ["COMMENT-DOC-CODE-ENGLISH"]


def test_markdown_inline_code_english_allowed() -> None:
    assert markdown_problems("d.md", "运行 `just fmt` 与 `cargo test --workspace`。\n") == []


def test_commit_message_backtick_chinese_still_exempt() -> None:
    assert prose_line_problems("t", 1, "见 ` 说明段 ` 一处。") == []


def test_markdown_table_first_column_chinese_code_flagged() -> None:
    problems = markdown_problems("d.md", "| `中文键` | 场景 |\n| --- | --- |\n")
    assert rules(problems) == ["COMMENT-DOC-CODE-ENGLISH"]


def test_markdown_table_later_column_chinese_code_flagged() -> None:
    problems = markdown_problems("d.md", "| 参数 | `中文值` |\n| --- | --- |\n")
    assert rules(problems) == ["COMMENT-DOC-CODE-ENGLISH"]


def test_markdown_reference_definition_backtick_still_checked() -> None:
    problems = markdown_problems("d.md", "[键]: `中文值` 备注\n")
    assert rules(problems) == ["COMMENT-DOC-CODE-ENGLISH"]
