"""评审行内评论门禁的单测：评论正文的书写规则逐条覆盖与反馈契约。

被测入口是门禁的发现函数与运行函数，返回违规列表，不落进程；评论正文没有
模板骨架，夹具不需要模板文件；机器人与空评论的排除单点在钩子工作流的作业
条件，脚本侧以空正文放行锁定该边界——排除判定不在这里做第二份。另含反馈
契约三件套：报告格式断言、汇总计数与确定性，经子进程跑脚本端到端锁输出与
退出码；用法见持续集成的钩子工作流。
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import check_review_comment  # noqa: E402

GATE = str(Path(__file__).with_name("check_review_comment.py"))


def rules(body: str) -> list[str]:
    return [f.rule for f in check_review_comment.findings(body)]


# ---- 评论正文书写规则 ----


def test_corner_bracket_comment_flagged() -> None:
    # 直角引号不入中文标点白名单，行内评论同样拦下——收口计划的指定样例
    problems = check_review_comment.run("这个写法「不对」吧，换弯引号。\n")
    assert problems and "outside the derived whitelist" in problems[0]


def test_english_line_flagged() -> None:
    problems = check_review_comment.run("Just run the tests here.\n")
    assert any("must be written in Chinese" in p for p in problems)


def test_chinese_with_latin_joined_flagged() -> None:
    problems = check_review_comment.run("改动见 docs/tests/bdd.md 文件。\n")
    assert any(
        "joins Chinese with Latin" in p or "outside the derived whitelist" in p for p in problems
    )


def test_backtick_and_fence_ok() -> None:
    body = "改动见 `docs/tests/bdd.md`，跑法如下：\n\n```sh\njust test\n```\n"
    assert check_review_comment.run(body) == []


def test_links_and_checkboxes_ok() -> None:
    body = "- [x] 已跑 `just precommit`\n说明见 [指南](https://example.com/guide)\n"
    assert check_review_comment.run(body) == []


def test_multiline_comment_judged_per_line() -> None:
    problems = check_review_comment.run("第一段合规。\n\nBad English line.\n")
    assert len(problems) == 1


def test_empty_body_passes_and_exclusion_stays_in_workflow() -> None:
    # 空评论在作业条件排除，脚本不做第二份排除判定——空正文进判定面即放行
    assert check_review_comment.run("") == []
    assert check_review_comment.run(" \n\t\n　\n") == []


def test_advisory_candidate_keeps_rule_and_stays_out_of_violations() -> None:
    # 软约束候选保留原标识与警告属性，不计入违规清单
    body = "我们先（initialize the model and train it）再评估。\n"
    advisories = [f for f in check_review_comment.findings(body) if f.advisory]
    assert advisories and {f.rule for f in advisories} == {"PROSE-ANCHOR"}
    assert check_review_comment.run(body) == []


def test_findings_carry_registered_prose_rule_ids() -> None:
    # 标识沿用登记表内的书写规则标识，不另设
    found = check_review_comment.findings("这个写法「不对」吧。\n\nJust run the tests.\n")
    assert rules("这个写法「不对」吧。\n") == ["PROSE-FOREIGN"]
    assert {f.rule for f in found} == {"PROSE-FOREIGN", "PROSE-LANGUAGE"}


# ---- 反馈契约：报告格式、汇总计数、确定性与退出码（子进程端到端）----


def run_gate(body: str, tmp_path: Path) -> subprocess.CompletedProcess[bytes]:
    body_file = tmp_path / "comment.md"
    body_file.write_text(body, encoding="utf-8")
    return subprocess.run([sys.executable, GATE, str(body_file)], capture_output=True)


def test_gate_reports_rule_id_and_violation_exit_code(tmp_path: Path) -> None:
    proc = run_gate("这个写法「不对」吧。\n", tmp_path)
    assert proc.returncode == 1
    text = proc.stderr.decode("utf-8")
    heads = [line for line in text.splitlines() if line.startswith("error[")]
    assert heads and all(re.match(r"^error\[PROSE-[A-Z-]+\]: .+$", line) for line in heads)
    assert "    help: " in text


def test_gate_summary_count_matches_printed_details(tmp_path: Path) -> None:
    proc = run_gate("这个写法「不对」吧。\n", tmp_path)
    text = proc.stderr.decode("utf-8")
    details = sum(1 for line in text.splitlines() if line.startswith("error["))
    match = re.search(r"^error: (\d+) violations? found \(see above\)$", text, re.M)
    assert match
    assert int(match.group(1)) == details


def test_gate_output_deterministic_across_runs(tmp_path: Path) -> None:
    body = "这个写法「不对」吧。\n\nJust run the tests.\n"
    first = run_gate(body, tmp_path)
    second = run_gate(body, tmp_path)
    assert first.returncode == second.returncode == 1
    assert first.stderr == second.stderr
    assert first.stdout == second.stdout


def test_gate_clean_comment_passes_with_exit_zero(tmp_path: Path) -> None:
    proc = run_gate("这一段写得清楚，改成行内代码更稳。\n", tmp_path)
    assert proc.returncode == 0
    assert "ok: review comment" in proc.stdout.decode("utf-8")


def test_gate_stdin_input_supported(tmp_path: Path) -> None:
    proc = subprocess.run(
        [sys.executable, GATE, "-"],
        input="这个写法「不对」吧。\n".encode("utf-8"),
        capture_output=True,
    )
    assert proc.returncode == 1
    assert b"PROSE-" in proc.stderr


def test_gate_no_arguments_is_usage_error() -> None:
    proc = subprocess.run([sys.executable, GATE], capture_output=True)
    assert proc.returncode == 2
    assert b"usage:" in proc.stderr
