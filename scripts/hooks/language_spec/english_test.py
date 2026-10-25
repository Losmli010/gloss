"""英文区规则的单测：纯英文判罚的放行与判罚两侧。

被测入口是英文区判定函数，直接传定位串与文本，不落进程；用法见注释门禁
的单测入口。
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parents[1]))

from language_spec import english  # noqa: E402


def test_pure_english_ok() -> None:
    assert english.english_zone_problems("t:1", "guard field access", "SAFETY comment") == []


def test_cjk_flagged() -> None:
    problems = english.english_zone_problems("t:1", "guard 字段访问", "SAFETY comment")
    assert any("pure English" in p for p in problems)


def test_fullwidth_and_curly_flagged() -> None:
    assert english.english_zone_problems("t:1", "return （x）", "SAFETY comment")
    assert english.english_zone_problems("t:1", "say “hi”", "SAFETY comment")
