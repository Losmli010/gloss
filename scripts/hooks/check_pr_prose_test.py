"""拉取请求内容门禁的单测：书写规则套用、模板小节核对与标题合并模拟。

被测入口是门禁的运行函数，返回违规列表，不落进程；模板写入测试框架
（pytest）的临时目录，互不串扰；用法见持续集成的钩子工作流。另含反馈契约
三件套：报告格式断言、汇总计数与确定性，经子进程跑脚本端到端锁输出与退
出码；模板必需小节从仓库真实模板动态提取，小节数随模板走不写死。
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import check_pr_prose  # noqa: E402

TEMPLATE = "## 测试（必填）\n\n## 安全与隐私（必填）\n"
REPO_TEMPLATE = Path(__file__).parents[2] / ".github" / "pull_request_template.md"


def make_root(tmp_path: Path, template: str = TEMPLATE) -> str:
    (tmp_path / ".github").mkdir(parents=True, exist_ok=True)
    (tmp_path / ".github" / "pull_request_template.md").write_text(template, encoding="utf-8")
    return str(tmp_path)


# ---- 模板小节 ----


def test_missing_section_flagged(tmp_path: Path) -> None:
    problems = check_pr_prose.run(make_root(tmp_path), "## 测试（必填）\n\n正文\n")
    assert any("misses required template section" in p for p in problems)


def test_all_sections_present_ok(tmp_path: Path) -> None:
    body = "## 测试（必填）\n\n跑了测试\n\n## 安全与隐私（必填）\n\n无\n"
    assert check_pr_prose.run(make_root(tmp_path), body) == []


def test_missing_template_skips_section_check(tmp_path: Path) -> None:
    body = "## 随意小节\n\n中文正文。\n"
    assert check_pr_prose.run(str(tmp_path), body) == []


# ---- 正文书写规则 ----


def test_body_bare_word_flagged(tmp_path: Path) -> None:
    problems = check_pr_prose.run(
        make_root(tmp_path), "## 测试（必填）\n\n改动见 docs/tests/bdd.md\n"
    )
    assert any("foreign letters" in p for p in problems)


def test_body_backtick_and_fence_ok(tmp_path: Path) -> None:
    body = (
        "## 测试（必填）\n\n改动见 `docs/tests/bdd.md`\n\n```sh\njust test\n```\n"
        "## 安全与隐私（必填）\n\n无\n"
    )
    assert check_pr_prose.run(make_root(tmp_path), body) == []


def test_body_links_and_checkboxes_ok(tmp_path: Path) -> None:
    body = (
        "## 测试（必填）\n\n- [x] 已跑 `just precommit`\n"
        "说明见 [指南](https://example.com/guide)\n"
        "## 安全与隐私（必填）\n\n无\n"
    )
    assert check_pr_prose.run(make_root(tmp_path), body) == []


def test_body_english_line_flagged(tmp_path: Path) -> None:
    problems = check_pr_prose.run(make_root(tmp_path), "## 测试（必填）\n\nJust run the tests.\n")
    assert any("must be written in Chinese" in p for p in problems)


def test_body_structure_penalties_not_applied(tmp_path: Path) -> None:
    body = (
        "## 测试（必填）\n\n#### 跳级小节\n\n## 测试（必填）\n\n```sh\njust test\n"
        "## 安全与隐私（必填）\n\n无\n"
    )
    assert check_pr_prose.run(make_root(tmp_path), body) == []


# ---- 标题合并模拟 ----


def test_title_chinese_ok(tmp_path: Path) -> None:
    body = "## 测试（必填）\n\n无\n\n## 安全与隐私（必填）\n\n无\n"
    assert check_pr_prose.run(make_root(tmp_path), body, title="feat: 新增翻译浮层") == []


def test_title_bare_word_flagged(tmp_path: Path) -> None:
    body = "## 测试（必填）\n\n无\n\n## 安全与隐私（必填）\n\n无\n"
    problems = check_pr_prose.run(make_root(tmp_path), body, title="feat: 添加 login 重试")
    assert any("squash-merge simulation" in p for p in problems)


def test_title_english_format_flagged(tmp_path: Path) -> None:
    body = "## 测试（必填）\n\n无\n"
    problems = check_pr_prose.run(make_root(tmp_path), body, title="Bump egui from 0.31 to 0.32")
    assert any("squash-merge simulation" in p for p in problems)


# ---- 规则标识：三判罚各归其位 ----


def test_findings_carry_rule_ids_per_category(tmp_path: Path) -> None:
    found = check_pr_prose.findings(
        make_root(tmp_path), "缺小节的正文\n\nJust run the tests.\n", title="Bump deps"
    )
    ids = {f.rule for f in found}
    assert ids == {"PR-SECTION", "PR-PROSE", "PR-TITLE-SIM"}


# ---- 反馈契约：报告格式、汇总计数、确定性与退出码（子进程端到端）----

GATE = str(Path(__file__).with_name("check_pr_prose.py"))


def repo_sections() -> list[str]:
    out = []
    for line in REPO_TEMPLATE.read_text(encoding="utf-8").splitlines():
        m = re.match(r"^## (.+)$", line)
        if m:
            out.append(m.group(1).strip())
    return out


def full_section_body(tail: str) -> str:
    parts = ["## %s\n\n%s\n" % (s, tail) for s in repo_sections()]
    return "".join(parts)


def run_gate(tmp_path: Path, body: str, title: str) -> subprocess.CompletedProcess[bytes]:
    body_file = tmp_path / "body.md"
    title_file = tmp_path / "title.txt"
    body_file.write_text(body, encoding="utf-8")
    title_file.write_text(title, encoding="utf-8")
    return subprocess.run(
        [sys.executable, GATE, str(body_file), str(title_file)], capture_output=True
    )


def test_gate_reports_rule_ids_and_violation_exit_code(tmp_path: Path) -> None:
    proc = run_gate(tmp_path, "缺少全部小节的正文\n\nJust run the tests.\n", "Bump deps")
    assert proc.returncode == 1
    text = proc.stderr.decode("utf-8")
    heads = [line for line in text.splitlines() if line.startswith("error[")]
    ids = {re.match(r"^error\[([A-Z0-9-]+)\]:", line).group(1) for line in heads}
    assert ids == {"PR-SECTION", "PR-PROSE", "PR-TITLE-SIM"}
    for line in heads:
        assert re.match(r"^error\[[A-Z0-9-]+\]: .+$", line)
    assert "    help: " in text


def test_gate_summary_count_matches_printed_details(tmp_path: Path) -> None:
    proc = run_gate(tmp_path, "缺少全部小节的正文\n\nJust run the tests.\n", "Bump deps")
    text = proc.stderr.decode("utf-8")
    details = sum(1 for line in text.splitlines() if line.startswith("error["))
    match = re.search(r"^error: (\d+) violations? found \(see above\)$", text, re.M)
    assert match
    assert int(match.group(1)) == details
    assert details >= 3


def test_gate_output_deterministic_across_runs(tmp_path: Path) -> None:
    body = "缺少全部小节的正文\n\nJust run the tests.\n"
    first = run_gate(tmp_path, body, "Bump deps")
    second = run_gate(tmp_path, body, "Bump deps")
    assert first.returncode == second.returncode == 1
    assert first.stderr == second.stderr
    assert first.stdout == second.stdout


def test_gate_clean_content_passes_with_exit_zero(tmp_path: Path) -> None:
    proc = run_gate(tmp_path, full_section_body("无\n"), "feat: 新增翻译浮层")
    assert proc.returncode == 0
    assert "ok pull request content" in proc.stdout.decode("utf-8")


def test_gate_no_arguments_is_usage_error() -> None:
    proc = subprocess.run([sys.executable, GATE], capture_output=True)
    assert proc.returncode == 2
    assert b"usage:" in proc.stderr
