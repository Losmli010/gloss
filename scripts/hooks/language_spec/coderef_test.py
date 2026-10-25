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
    markdown_structure_problems,
    prose_line_problems,
)


def messages(findings: list[gate_report.Finding]) -> list[str]:
    return [finding.message for finding in findings]


def test_backticked_identifier_exempt() -> None:
    assert prose_line_problems("t", 1, "改动见 `docs/tests/bdd.md`") == []


def test_bare_identifier_flagged() -> None:
    problems = prose_line_problems("t", 1, "改动见 docs/tests/bdd.md")
    assert any("foreign letters" in p for p in messages(problems))


def test_unpaired_backtick_flagged() -> None:
    problems = prose_line_problems("t", 1, "标题 ` 残缺")
    assert any("code segment" in p for p in messages(problems))


def test_pure_identifier_exempt() -> None:
    assert prose_line_problems("t", 1, "some_test_name") == []


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
    assert any("foreign letters" in p for p in messages(problems))


def test_markdown_table_first_cell_exempt() -> None:
    src = "| some_test_name | 中文场景 |\n| --- | --- |\n| other_name | generic 词 |\n"
    problems = markdown_problems("d.md", src)
    assert any("foreign letters" in p for p in messages(problems))


# ---- 文档结构遍历 ----


def rules(findings: list[gate_report.Finding]) -> list[str]:
    return [finding.rule for finding in findings]


def test_markdown_structure_clean_doc_passes() -> None:
    src = "# 标题\n\n## 小节\n\n### 次节\n\n```sh\njust test\n```\n\n## 回到上级\n"
    assert markdown_structure_problems("d.md", src) == []


def test_markdown_first_heading_any_level_allowed() -> None:
    assert markdown_structure_problems("d.md", "## 直接小节\n") == []


def test_markdown_level_decrease_then_increase_allowed() -> None:
    src = "# 标题\n\n## 小节\n\n### 次节\n\n## 回到上级\n\n### 再进一级\n"
    assert markdown_structure_problems("d.md", src) == []


def test_markdown_heading_jump_flagged_at_line() -> None:
    problems = markdown_structure_problems("d.md", "# 标题\n\n### 跳级小节\n")
    assert rules(problems) == ["COMMENT-DOC-HEADING-JUMP"]
    assert problems[0].line == 3


def test_markdown_duplicate_heading_flagged_at_later_line() -> None:
    problems = markdown_structure_problems("d.md", "## 小节\n\n正文一。\n\n## 小节\n")
    assert rules(problems) == ["COMMENT-DOC-HEADING-DUP"]
    assert problems[0].line == 5


def test_markdown_headings_inside_fence_not_counted() -> None:
    src = "## 小节\n\n```sh\n## 小节\n```\n\n## 另一节\n"
    assert markdown_structure_problems("d.md", src) == []


def test_markdown_unclosed_fence_flagged_at_opening_line() -> None:
    problems = markdown_structure_problems("d.md", "## 小节\n\n```sh\njust test\n")
    assert rules(problems) == ["COMMENT-DOC-FENCE-UNCLOSED"]
    assert problems[0].line == 3
