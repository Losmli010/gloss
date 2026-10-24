"""中文区规则的单测：字符集判罚、专名注记与括号配对、白名单与指令段豁免。

被测入口是规则函数，直接传定位串与表述文本，不落进程；用法见注释门禁与
提交信息门禁的单测入口。
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parents[1]))

from language_spec import chinese  # noqa: E402


def test_gloss_allowed():
    assert chinese.body_problems("t:1", "中文头借用图形库（wgpu）渲染。", "header") == []


def test_bare_word_flagged():
    problems = chinese.body_problems("t:1", "中文头说明 wgpu 渲染。", "header")
    assert any("foreign letters" in p for p in problems)


def test_nested_brackets_flagged():
    problems = chinese.body_problems("t:1", "主线程（窗口库（winit）循环）。", "header")
    assert any("must not nest" in p for p in problems)


def test_unpaired_bracket_flagged():
    problems = chinese.body_problems("t:1", "中文头（缺右括号。", "header")
    assert any("unpaired" in p for p in problems)


def test_decimal_point_allowed():
    assert chinese.chinese_zone_problems("t:1", "版本 0.31 发布", "doc") == []


def test_halfwidth_punct_flagged():
    assert chinese.chinese_zone_problems("t:1", "hello, world", "doc")


def test_gloss_outside_allowlist_flagged():
    problems = chinese.body_problems("t:1", "登录页（login）校验。", "header")
    assert any("outside the allowlist" in p for p in problems)


def test_gloss_mixed_chinese_flagged():
    problems = chinese.body_problems("t:1", "中文头（用 wgpu 渲染）说明。", "header")
    assert any("mixes Chinese" in p for p in problems)


def test_pure_chinese_parenthetical_checked_as_body():
    assert chinese.body_problems("t:1", "吞吐（每秒笔数）达标。", "header") == []


def test_directive_segment_exempt():
    assert chinese.chinese_zone_problems("t:1", "noqa: E402", "doc") == []


def test_chinese_punct_allowed():
    assert chinese.chinese_zone_problems("t:1", "快、清、小、稳——“克制”。", "doc") == []
