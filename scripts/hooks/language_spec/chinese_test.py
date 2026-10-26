"""中文区规则的单测：字符集判罚、专名注记与括号配对、白名单与指令段豁免。

被测入口是规则函数，直接传路径、行号与表述文本，返回带规则标识的违规条目，
不落进程；断言取条目消息核对判罚片段；用法见注释门禁与提交信息门禁的单测
入口。
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parents[1]))

from language_spec import chinese  # noqa: E402


def messages(findings: list[chinese.gate_report.Finding]) -> list[str]:
    return [finding.message for finding in findings]


def test_gloss_allowed() -> None:
    assert chinese.body_problems("t", 1, "中文头借用图形库（wgpu）渲染。", "header") == []


def test_bare_word_flagged() -> None:
    problems = chinese.body_problems("t", 1, "中文头说明 wgpu 渲染。", "header")
    assert any("foreign letters" in p for p in messages(problems))


def test_nested_brackets_flagged() -> None:
    problems = chinese.body_problems("t", 1, "主线程（窗口库（winit）循环）。", "header")
    assert any("must not nest" in p for p in messages(problems))


def test_unpaired_bracket_flagged() -> None:
    problems = chinese.body_problems("t", 1, "中文头（缺右括号。", "header")
    assert any("unpaired" in p for p in messages(problems))


def test_decimal_point_allowed() -> None:
    assert chinese.chinese_zone_problems("t", 1, "版本 0.31 发布", "doc") == []


def test_halfwidth_punct_flagged() -> None:
    assert chinese.chinese_zone_problems("t", 1, "hello, world", "doc")


def test_gloss_outside_allowlist_flagged() -> None:
    problems = chinese.body_problems("t", 1, "登录页（login）校验。", "header")
    assert any("outside the allowlist" in p for p in messages(problems))


def test_gloss_mixed_chinese_flagged() -> None:
    problems = chinese.body_problems("t", 1, "中文头（用 wgpu 渲染）说明。", "header")
    assert any("mixes Chinese" in p for p in messages(problems))


def test_pure_chinese_parenthetical_checked_as_body() -> None:
    assert chinese.body_problems("t", 1, "吞吐（每秒笔数）达标。", "header") == []


def test_directive_segment_exempt() -> None:
    assert chinese.chinese_zone_problems("t", 1, "noqa: E402", "doc") == []


def test_chinese_punct_allowed() -> None:
    assert chinese.chinese_zone_problems("t", 1, "快、清、小、稳——“克制”。", "doc") == []


# ---- 可运行命令段豁免 ----


def test_runnable_command_segment_exempt() -> None:
    line = "python3 scripts/hooks/check_demo.py <repo-root>，缺省为本脚本的上一级目录。"
    assert chinese.chinese_zone_problems("t", 1, line, "text") == []


def test_command_with_flags_and_placeholders_exempt() -> None:
    line = "cargo run -p gloss-eval --bin eval -- live [args ...]"
    assert chinese.chinese_zone_problems("t", 1, line, "text") == []


def test_command_mixed_with_chinese_still_flagged() -> None:
    problems = chinese.chinese_zone_problems(
        "t", 1, "python3 scripts/hooks/check_demo.py 仓库根", "text"
    )
    assert any("foreign letters" in m for m in messages(problems))


def test_english_prose_without_structural_char_flagged() -> None:
    # 以工具词开头、无命令形态结构性记号的英文散文句不豁免
    problems = chinese.chinese_zone_problems("t", 1, "just run the tests here", "text")
    assert problems


def test_english_sentence_with_period_flagged() -> None:
    problems = chinese.chinese_zone_problems("t", 1, "make sure it works.", "text")
    assert any("foreign letters" in m for m in messages(problems))


def test_math_symbols_allowed_in_chinese_body() -> None:
    assert (
        chinese.body_problems(
            "t", 1, "误差 ≤ 0.5，温度 25℃，夹角 45°，比例系数 α 与角频率 ω。", "text"
        )
        == []
    )


def test_math_operators_allowed_in_chinese_body() -> None:
    assert chinese.body_problems("t", 1, "满足 3 + 4 = 7 且 5 ≠ 6，收敛于 ∞。", "text") == []


def test_latin_variable_still_flagged_without_markup() -> None:
    problems = chinese.body_problems("t", 1, "当 x = 1 时成立。", "text")
    assert any("foreign letters" in p for p in messages(problems))


def test_markdown_collision_signs_still_flagged() -> None:
    problems = chinese.body_problems("t", 1, "斜杠 / 与竖线 | 不放行。", "text")
    assert any("foreign letters" in p for p in messages(problems))


def test_nbsp_and_ideographic_space_allowed() -> None:
    assert chinese.chinese_zone_problems("t", 1, "列\u00a0宽\u3000撑住", "doc") == []


def test_control_character_still_flagged() -> None:
    assert chinese.chinese_zone_problems("t", 1, "混入\u0007响铃控制符", "doc")
