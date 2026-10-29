"""中文区规则的单测：字符面派生判罚、数字附随邻接、注记括号单语与配对、
指令段豁免。

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


def test_nested_brackets_pass_and_stay_single_language() -> None:
    assert chinese.body_problems("t", 1, "主线程（窗口库（winit）循环）。", "header") == []


def test_nested_brackets_judged_per_level() -> None:
    problems = chinese.body_problems("t", 1, "主线程（窗口库（winit 循环）驱动）。", "header")
    assert any("mixes" in p for p in messages(problems))


def test_unpaired_bracket_flagged() -> None:
    problems = chinese.body_problems("t", 1, "中文头（缺右括号。", "header")
    assert any("unpaired" in p for p in messages(problems))


def test_stray_close_bracket_flagged() -> None:
    problems = chinese.body_problems("t", 1, "中文头）多出单边括号。", "header")
    assert any("unpaired" in p for p in messages(problems))


def test_unclosed_bracket_content_checked_as_body() -> None:
    problems = chinese.body_problems("t", 1, "版本（v1.2 缺右括号。", "header")
    assert any("unpaired" in p for p in messages(problems))
    assert any("foreign letters" in p for p in messages(problems))


def test_decimal_point_allowed() -> None:
    assert chinese.chinese_zone_problems("t", 1, "版本 0.31 发布", "doc") == []


def test_halfwidth_punct_flagged() -> None:
    assert chinese.chinese_zone_problems("t", 1, "hello, world", "doc")


def test_gloss_pure_foreign_term_allowed() -> None:
    assert chinese.body_problems("t", 1, "登录页（login）校验。", "header") == []


def test_gloss_mixed_runs_flagged() -> None:
    problems = chinese.body_problems("t", 1, "中文头（用 wgpu 渲染）说明。", "header")
    assert any("mixes" in p for p in messages(problems))


def test_gloss_single_letter_run_passes() -> None:
    assert chinese.body_problems("t", 1, "缓存（L 值）语义。", "header") == []


def test_gloss_kana_counts_as_foreign() -> None:
    problems = chinese.body_problems("t", 1, "渲染（テスト 混排）失败。", "header")
    assert any("mixes" in p for p in messages(problems))


def test_gloss_inner_content_skips_body_whitelist() -> None:
    assert chinese.body_problems("t", 1, "阈值（好@例子）达标。", "header") == []


def test_pure_chinese_parenthetical_allowed() -> None:
    assert chinese.body_problems("t", 1, "吞吐（每秒笔数）达标。", "header") == []


def test_directive_segment_exempt() -> None:
    assert chinese.chinese_zone_problems("t", 1, "noqa: E402", "doc") == []


def test_chinese_punct_allowed() -> None:
    assert chinese.chinese_zone_problems("t", 1, "快、清、小、稳——“克制”。", "doc") == []


# ---- 数字附随符号：三族邻接放行 ----


def test_attached_measures_and_currency_pass_by_digits() -> None:
    line = "温度 25℃，夹角 45°，比例 50‰，花费 €5 或 ￥100。"
    assert chinese.chinese_zone_problems("t", 1, line, "text") == []


def test_attached_between_digits_pass() -> None:
    assert chinese.chinese_zone_problems("t", 1, "区间 1,000 至 3-4 天、2.5 倍。", "text") == []


def test_attached_symbols_flag_without_digit_neighbor() -> None:
    line = "占比 50 ％偏高，花费 € 5，货币 ￥ 写法。"
    assert chinese.chinese_zone_problems("t", 1, line, "text")


def test_fullwidth_equals_percent_tilde_out_of_whitelist() -> None:
    problems = chinese.chinese_zone_problems(
        "t", 1, "降级＝有痕迹，范围 3～5 天，币种 ￥。", "text"
    )
    assert problems


def test_derived_whitelist_carries_all_three_faces() -> None:
    # 场景标点、破折号省略号家族与中性数学面各取其一，钉住并集派生；
    # 全角等号、百分号与浪纹线不在任何数据面
    for symbol in "、…≤":
        assert symbol in chinese.WHITELIST
    for symbol in "＝％～":
        assert symbol not in chinese.WHITELIST


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


def test_math_operators_allowed_in_chinese_body() -> None:
    assert chinese.body_problems("t", 1, "满足 3 + 4 = 7 且 5 ≠ 6。", "text") == []


def test_greek_and_extended_math_flagged() -> None:
    problems = chinese.chinese_zone_problems("t", 1, "收敛于 ∞，夹角 θ。", "text")
    assert problems


def test_extended_han_ranges_pass_in_body() -> None:
    line = "扩展甲区（㐀）与兼容区（豈）随三段口径放行。"
    assert chinese.body_problems("t", 1, line, "text") == []


def test_extended_han_counts_as_cjk_wise() -> None:
    # 区段口径统一后，扩展区与兼容区汉字进宽度启发面：英文区与行内代码不再漏判
    assert chinese.CJKWISE.search("㐀")
    assert chinese.CJKWISE.search("豈")


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
