"""记录标点门禁（check_record_punct）的测试：规则级直调判定入口，覆盖三条
规则的判罚形态与豁免面（解析树单元合并、行内代码与围栏豁免、文件名与章节
号放行、配对三形态）；门禁级用临时目录样本走显式清单与本门禁的收集入口，
另经注释纪律路由的收集入口确认分派生效。夹具为内存样本字符串与临时目录
样本文件，不落版本库；本文件自身注释同规。
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import check_comments  # noqa: E402
import check_docs  # noqa: E402
import check_record_punct  # noqa: E402


def problems(src: str, rel: str = "docs/comments/demo_comment.md") -> list:
    return check_record_punct.check(rel, src)


def rules(src: str) -> list[str]:
    return [finding.rule for finding in problems(src)]


# ---- 直角引号出现即罚 ----


def test_corner_brackets_flagged_once_per_unit() -> None:
    found = problems("甲「乙」与「丙」丁")
    assert len(found) == 1
    assert found[0].rule == "PUNCT-CORNER"
    assert "「 」" in found[0].message
    assert found[0].column == 2


def test_corner_inside_inline_code_exempt() -> None:
    assert problems("记号 `「」` 判定完毕") == []


def test_corner_inside_cross_line_code_span_exempt() -> None:
    assert problems("引用 `「记号」\n续行` 判定完毕") == []


def test_corner_inside_fence_exempt() -> None:
    src = "```bash\n# 注释「引号」,半角\n```\n"
    assert problems(src) == []


# ---- 半角标点邻接 ----


def test_halfwidth_comma_adjacent_flagged() -> None:
    found = problems("常驻纹理,边长上限")
    assert rules("常驻纹理,边长上限") == ["PUNCT-ADJACENT"]
    assert found[0].column == 5
    assert "," in found[0].message


def test_fullwidth_punct_passes() -> None:
    assert problems("常驻纹理，边长上限（八千）完毕。") == []


def test_digit_adjacent_punct_passes() -> None:
    assert problems("耗时 1,024 ms 与 3.5 秒（2026-10-16）完毕") == []


def test_filename_extension_dot_exempt() -> None:
    assert problems("见 文档.md 与脚本") == []


def test_section_number_dot_exempt() -> None:
    assert problems("取舍见设计文档 §八.8 与清单") == []


def test_straight_quote_adjacent_flagged() -> None:
    assert rules('所谓"正统"路径') == ["PUNCT-ADJACENT"]


def test_gloss_interior_adjacent_judged() -> None:
    assert rules("注记（keychain,词条）收口") == ["PUNCT-ADJACENT"]


# ---- 配对判定 ----


def test_softwrap_paren_pair_passes() -> None:
    assert problems("他说明（第一段\n续行）收尾。") == []


def test_orphan_closer_flagged() -> None:
    found = problems("已知（缺口）与）孤例")
    assert rules("已知（缺口）与）孤例") == ["PUNCT-UNPAIRED"]
    assert "orphan closer" in found[0].message
    assert found[0].column == 8


def test_unclosed_opener_flagged() -> None:
    found = problems("外层（内层“引语”未收")
    assert "unclosed opener" in found[0].message


def test_interleave_flagged() -> None:
    found = problems("外层（内层“引语）交叉”收尾")
    assert "interleaved" in found[0].message


def test_table_cell_judged() -> None:
    src = "| 名称 | 值,半角 |\n| --- | --- |\n"
    assert rules(src) == ["PUNCT-ADJACENT"]


def test_indented_code_block_exempt() -> None:
    src = "正文前导。\n\n    示例「引号」,半角\n"
    assert problems(src) == []


# ---- 受检面边界与门禁级行为 ----


def test_handles_scope_matches_record_directory_only() -> None:
    assert check_record_punct.handles(os.path.join("docs", "comments", "demo_comment.md"))
    assert not check_record_punct.handles("README.md")
    assert not check_record_punct.handles(os.path.join("docs", "comments", "demo_comment.py"))
    assert not check_docs.handles(os.path.join("docs", "comments", "demo_comment.md"))


def test_clean_sample_collects_nothing(tmp_path: Path) -> None:
    rel = os.path.join("docs", "comments", "demo_comment.md")
    (tmp_path / rel).parent.mkdir(parents=True)
    (tmp_path / rel).write_text("正常“弯引号”文本（配对）完毕。", encoding="utf-8")
    assert check_record_punct.collect(str(tmp_path), [rel]) == []


def test_violation_sample_reports_rule_and_remedy(tmp_path: Path) -> None:
    rel = os.path.join("docs", "comments", "demo_comment.md")
    (tmp_path / rel).parent.mkdir(parents=True)
    (tmp_path / rel).write_text("遗留「直角」样本（未配", encoding="utf-8")
    found = check_record_punct.collect(str(tmp_path), [rel])
    assert {finding.rule for finding in found} == {"PUNCT-CORNER", "PUNCT-UNPAIRED"}
    assert all(finding.remedy for finding in found)


def test_router_dispatches_record_directory_to_punct_gate(tmp_path: Path) -> None:
    rel = os.path.join("docs", "comments", "demo_comment.md")
    (tmp_path / rel).parent.mkdir(parents=True)
    (tmp_path / rel).write_text("路由「角引」样本", encoding="utf-8")
    _oks, found, _advisories = check_comments.collect(str(tmp_path), [rel])
    assert [finding.rule for finding in found] == ["PUNCT-CORNER"]


def test_main_exit_codes_follow_findings(monkeypatch, tmp_path: Path) -> None:
    rel = os.path.join("docs", "comments", "demo_comment.md")
    (tmp_path / rel).parent.mkdir(parents=True)
    (tmp_path / rel).write_text("干净样本（配对）完毕。", encoding="utf-8")
    monkeypatch.setattr(sys, "argv", ["check_record_punct.py", str(tmp_path), rel])
    assert check_record_punct.main() == 0
    (tmp_path / rel).write_text("违规「样本」", encoding="utf-8")
    assert check_record_punct.main() == 1


# ---- 单元形态、豁免边界与登记对账（评审补） ----


def test_heading_list_and_blockquote_units_judged() -> None:
    src = "## 标题「引号」,半角\n\n- 列表「引号」,半角\n\n> 引用「引号」,半角\n"
    found = problems(src)
    assert {finding.rule for finding in found} == {"PUNCT-CORNER", "PUNCT-ADJACENT"}
    assert len(found) == 6


def test_long_extension_word_not_exempt() -> None:
    assert rules("算子.compute均值") == ["PUNCT-ADJACENT"]


def test_inner_single_quotes_inside_double_passes() -> None:
    assert problems("外层“引语‘内层’收束”完毕") == []


def test_link_text_judged_and_image_skipped() -> None:
    src = "说明[甲,半角](https://example.com/x)与图![乙](https://x/y)收束"
    assert rules(src) == ["PUNCT-ADJACENT"]


def test_interleave_leaves_unclosed_reported() -> None:
    found = problems("残留（“）形态")
    assert "interleaved" in found[0].message
    assert "unclosed" in found[0].message


def test_word_apostrophe_passes() -> None:
    assert problems("模型照抄 it’s ok") == []


def test_quick_reference_categories_cover_router_gates() -> None:
    sys.path.insert(0, str(Path(__file__).parents[2] / "scripts" / "dev"))
    import comment_rules  # noqa: E402

    covered = {rel for category in comment_rules.CATEGORIES for rel in category.paths}
    gates = (
        "check_rust_comments",
        "check_script_comments",
        "check_docs",
        "check_record_punct",
    )
    for name in gates:
        assert "comments/%s.py" % name in covered
