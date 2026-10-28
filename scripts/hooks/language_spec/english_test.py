"""英文区规则的单测：纯英文判罚的放行与判罚两侧。

被测入口是英文区判定函数，直接传路径、行号与文本，返回带规则标识的违规条
目，不落进程；断言取条目消息核对判罚片段；用法见注释门禁的单测入口。
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parents[1]))

from language_spec import english  # noqa: E402


def test_pure_english_ok() -> None:
    assert english.english_zone_problems("t", 1, "guard field access", "SAFETY comment") == []


def test_cjk_flagged() -> None:
    problems = english.english_zone_problems("t", 1, "guard 字段访问", "SAFETY comment")
    assert any("pure English" in finding.message for finding in problems)


def test_fullwidth_and_curly_flagged() -> None:
    assert english.english_zone_problems("t", 1, "return （x）", "SAFETY comment")
    assert english.english_zone_problems("t", 1, "say “hi”", "SAFETY comment")


def test_math_symbols_allowed() -> None:
    text = "the bound holds for all x ∈ S: n^2 ≤ n^3 when x ≠ 0 and ε > 0"
    assert english.english_zone_problems("t", 1, text, "SAFETY comment") == []


def test_default_remedy_keeps_invariant_wording() -> None:
    problems = english.english_zone_problems("t", 1, "guard 字段访问", "SAFETY comment")
    assert all(finding.remedy == english.ENGLISH_REMEDY for finding in problems)


def test_custom_remedy_rides_message() -> None:
    problems = english.english_zone_problems(
        "t", 1, "guard 字段访问", "prompt line", "rewrite the whole line in English"
    )
    assert all(
        finding.message == "prompt line must be pure English — rewrite the whole line in English"
        for finding in problems
    )
    assert all(finding.remedy == "rewrite the whole line in English" for finding in problems)
