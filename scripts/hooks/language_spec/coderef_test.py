"""代码引用规则的单测：结构性记号豁免、代码段残留判罚与标记语言遍历。

被测入口是行级判定与文档遍历函数，直接传定位串与文本，不落进程；用法见
注释门禁与提交信息门禁的单测入口。
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parents[1]))

from language_spec.coderef import MARKER, markdown_problems, prose_line_problems  # noqa: E402


def test_backticked_identifier_exempt() -> None:
    assert prose_line_problems("t:1", "改动见 `docs/tests/bdd.md`") == []


def test_bare_identifier_flagged() -> None:
    problems = prose_line_problems("t:1", "改动见 docs/tests/bdd.md")
    assert any("foreign letters" in p for p in problems)


def test_unpaired_backtick_flagged() -> None:
    problems = prose_line_problems("t:1", "标题 ` 残缺")
    assert any("code segment" in p for p in problems)


def test_pure_identifier_exempt() -> None:
    assert prose_line_problems("t:1", "some_test_name") == []


def test_english_line_flagged() -> None:
    problems = prose_line_problems("t:1", "Just run the tests.")
    assert any("must be written in Chinese" in p for p in problems)


def test_list_and_task_markers_exempt() -> None:
    assert prose_line_problems("t:1", "- [x] 已完成的条目") == []


def test_bold_and_date_exempt() -> None:
    assert prose_line_problems("t:1", "**克制**——2026-09-19 起。") == []


def test_link_and_badge_exempt() -> None:
    assert prose_line_problems("t:1", "[![CI](a.svg)](b)") == []


def test_allow_marker_regex() -> None:
    assert MARKER.search("let x = 1; # secrets:allow")


def test_markdown_fence_exempt() -> None:
    src = "# 标题\n\n```sh\njust test\n```\n\n中文正文。\n"
    assert markdown_problems("d.md", src) == []


def test_markdown_bare_word_flagged() -> None:
    problems = markdown_problems("d.md", "中文正文带 generic 词。\n")
    assert any("foreign letters" in p for p in problems)


def test_markdown_table_first_cell_exempt() -> None:
    src = "| some_test_name | 中文场景 |\n| --- | --- |\n| other_name | generic 词 |\n"
    problems = markdown_problems("d.md", src)
    assert any("foreign letters" in p for p in problems)
