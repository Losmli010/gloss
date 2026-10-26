"""文档引用门禁的单测：逐条覆盖五类引用校验与跳过规则。

被测入口是门禁的运行函数，返回违规与计数元组；夹具建在测试框架（pytest）的
临时目录，互不串扰；用法见持续集成的钩子工作流（逐文件执行单测）。受检文档
文件缺失是失败关闭的前置故障。另含反馈契约三件套：报告格式断言、汇总计数与
确定性，经子进程跑脚本端到端锁输出与退出码。
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).parent))

import check_agents_doc  # noqa: E402
import gate_report  # noqa: E402

JUSTFILE = """\
coverage_min := "70"

run:
    cargo run

logs:
    @echo logs

logs-dir:
    @echo logs dir

test: run
    cargo test --workspace

bench-check name="last":
    @echo check {{name}}
"""

RULE_DOC = """## 质量条目与门禁对照

### 质量条目总表

| 维度 | 级别 | 条目 | 强制方式 |
| --- | --- | --- | --- |
| 可维护性 | CRITICAL | **日志统一出口**：只用 `just run` 一处出口 | `just run` |"""

RULE_DOC_RENAMED = """## 质量条目与门禁对照

### 质量条目总表

| 维度 | 级别 | 条目 | 强制方式 |
| --- | --- | --- | --- |
| 可维护性 | LOW | **日志出口**：只用 `just run`。 | `just run` |"""


def build_fixture(tmp_path: Path) -> None:
    for d in ("crates/demo/tests", "crates/demo/src", "tests", "docs"):
        (tmp_path / d).mkdir(parents=True, exist_ok=True)
    (tmp_path / "Cargo.toml").write_text("")
    (tmp_path / "kittest.toml").write_text("")
    (tmp_path / "tests/overlay_selftest.rs").write_text("")
    (tmp_path / "crates/demo/tests/pipeline.rs").write_text("")
    (tmp_path / "crates/demo/src/lib.rs").write_text("")
    (tmp_path / "docs/note.md").write_text("# 说明文档\n")
    (tmp_path / "justfile").write_text(JUSTFILE)


def check(
    tmp_path: Path, body: str, ref_body: str | None = None
) -> tuple[list[str], int, int, int]:
    """写入 AGENTS.md（可选 ref_body 进 crates/demo/src/lib.rs）并运行门禁。"""
    build_fixture(tmp_path)
    (tmp_path / "AGENTS.md").write_text(body + "\n", encoding="utf-8")
    if ref_body is not None:
        (tmp_path / "crates/demo/src/lib.rs").write_text(ref_body + "\n", encoding="utf-8")
    return check_agents_doc.run(str(tmp_path / "AGENTS.md"))


def assert_pass(tmp_path: Path, body: str, ref_body: str | None = None) -> None:
    violations, _, _, _ = check(tmp_path, body, ref_body)
    assert violations == []


def assert_fail(tmp_path: Path, body: str, needle: str, ref_body: str | None = None) -> None:
    violations, _, _, _ = check(tmp_path, body, ref_body)
    assert any(needle in v for v in violations)


# ---- 合法用例 ----


def test_all_references_exist(tmp_path: Path) -> None:
    body = "见 `just test`、`just logs`、`just logs-dir`、`docs/note.md`、`Cargo.toml`"
    assert_pass(tmp_path, body)


def test_recipe_with_default_arg(tmp_path: Path) -> None:
    assert_pass(tmp_path, "对照跑 `just bench-check`")


def test_repo_paths_file_and_dir(tmp_path: Path) -> None:
    assert_pass(tmp_path, "产物在 `crates/demo/tests/`，配置见 `kittest.toml`")


def test_cargo_test_targets_exist(tmp_path: Path) -> None:
    body = "跑 `cargo test --test overlay_selftest` 与 `cargo test --test pipeline`"
    assert_pass(tmp_path, body)


def test_placeholders_and_outside_paths_skipped(tmp_path: Path) -> None:
    body = "分支名 `feat/<主题>`，日志在 `~/.gloss/logs/`，模式 `*.workspace = true`"
    assert_pass(tmp_path, body)


def test_comment_syntax_and_urls_not_paths(tmp_path: Path) -> None:
    body = "每个 unsafe 块带 `// SAFETY:` 注释，文档见 `https://example.com/a/b`"
    assert_pass(tmp_path, body)


def test_justfile_and_just_list_not_recipes(tmp_path: Path) -> None:
    assert_pass(tmp_path, "全部 recipe 见 `justfile`，用 `just --list` 查看")


def test_recipe_in_code_block_checked(tmp_path: Path) -> None:
    body = "命令清单：\n\n    just run\n    just test"
    assert_pass(tmp_path, body)


# ---- 非法用例 ----


def test_missing_recipe(tmp_path: Path) -> None:
    assert_fail(tmp_path, "跑 `just deploy` 发布", 'recipe "just deploy" not found')


def test_plain_text_recipe_checked(tmp_path: Path) -> None:
    assert_fail(tmp_path, "先执行 just build-release 再打包", "just build-release")


def test_justfile_variable_not_a_recipe(tmp_path: Path) -> None:
    # 配方名提取不含下划线：带下划线的配方名被截断提取，按不存在的配方拒绝
    assert_fail(tmp_path, "阈值见 `just coverage_min`", 'recipe "just coverage" not found')


def test_missing_path(tmp_path: Path) -> None:
    assert_fail(tmp_path, "实现见 `crates/demo/src/store.rs`", "does not exist in the repo")


def test_missing_bare_filename(tmp_path: Path) -> None:
    assert_fail(tmp_path, "配置见 `nonexistent.toml`", "does not exist at the repo root")


def test_untracked_path_flagged_inside_repo(tmp_path: Path) -> None:
    build_fixture(tmp_path)
    (tmp_path / "AGENTS.md").write_text("见 `docs/note.md`\n", encoding="utf-8")
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True)
    subprocess.run(["git", "-C", str(tmp_path), "add", "AGENTS.md"], check=True)
    violations, _, _, _ = check_agents_doc.run(str(tmp_path / "AGENTS.md"))
    assert any("does not exist" in v for v in violations)


def test_tracked_path_accepted_inside_repo(tmp_path: Path) -> None:
    build_fixture(tmp_path)
    (tmp_path / "AGENTS.md").write_text("见 `docs/note.md`\n", encoding="utf-8")
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True)
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True)
    violations, _, _, _ = check_agents_doc.run(str(tmp_path / "AGENTS.md"))
    assert violations == []


def test_non_ascii_tracked_name_enumerated_raw(tmp_path: Path) -> None:
    build_fixture(tmp_path)
    (tmp_path / "AGENTS.md").write_text("见 `文档.md`\n", encoding="utf-8")
    (tmp_path / "文档.md").write_text("正文\n", encoding="utf-8")
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True)
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True)
    violations, _, _, _ = check_agents_doc.run(str(tmp_path / "AGENTS.md"))
    assert violations == []


def test_missing_test_target(tmp_path: Path) -> None:
    assert_fail(tmp_path, "跑 `cargo test --test smoke`", "no tests/smoke.rs found")


def test_wrong_directory_level(tmp_path: Path) -> None:
    body = "见 `crates/demo/tests/overlay_selftest.rs`"
    assert_fail(tmp_path, body, "does not exist in the repo")


def test_bad_recipe_in_code_block(tmp_path: Path) -> None:
    body = "命令清单：\n\n    just run\n    just deploy"
    assert_fail(tmp_path, body, 'recipe "just deploy" not found')


# ---- 质量条目名指名引用（仓库各处的 质量条目“名字”）----


def test_named_rule_reference_hits_real_entry(tmp_path: Path) -> None:
    ref = "//! 见 AGENTS.md 质量条目“日志统一出口”。"
    assert_pass(tmp_path, RULE_DOC, ref)


def test_named_rule_reference_unknown_entry(tmp_path: Path) -> None:
    ref = "//! 见 AGENTS.md 质量条目“日志英文”。"
    assert_fail(tmp_path, RULE_DOC, "日志英文", ref)


def test_named_rule_reference_stale_after_rename(tmp_path: Path) -> None:
    ref = "//! 见 AGENTS.md 质量条目“日志统一出口”。"
    assert_fail(tmp_path, RULE_DOC_RENAMED, "日志统一出口", ref)


def test_no_named_reference_no_false_positive(tmp_path: Path) -> None:
    assert_pass(tmp_path, RULE_DOC)


# ---- 受检文档缺失守卫（失败关闭）----


def test_missing_doc_fails_closed(tmp_path: Path) -> None:
    with pytest.raises(SystemExit) as exc:
        check_agents_doc.run(str(tmp_path / "AGENTS.md"))
    assert exc.value.code == gate_report.EXIT_INFRA


# ---- 反馈契约：报告格式、汇总计数、确定性与退出码（子进程端到端）----

GATE = str(Path(__file__).with_name("check_agents_doc.py"))

VIOLATION_BODY = (
    "跑 `just deploy` 发布，实现见 `crates/demo/src/store.rs`，"
    "配置见 `nonexistent.toml`，跑 `cargo test --test smoke`"
)


def run_gate(
    tmp_path: Path, body: str, ref_body: str | None = None, *extra: str
) -> subprocess.CompletedProcess[bytes]:
    build_fixture(tmp_path)
    (tmp_path / "AGENTS.md").write_text(body + "\n", encoding="utf-8")
    if ref_body is not None:
        (tmp_path / "crates/demo/src/lib.rs").write_text(ref_body + "\n", encoding="utf-8")
    return subprocess.run(
        [sys.executable, GATE, str(tmp_path / "AGENTS.md")] + list(extra), capture_output=True
    )


def test_gate_reports_rule_ids_and_violation_exit_code(tmp_path: Path) -> None:
    proc = run_gate(tmp_path, VIOLATION_BODY, "//! 见 AGENTS.md 质量条目“日志英文”。")
    assert proc.returncode == 1
    text = proc.stderr.decode("utf-8")
    heads = [line for line in text.splitlines() if line.startswith("error[")]
    ids = {re.match(r"^error\[([A-Z0-9-]+)\]:", line).group(1) for line in heads}
    assert ids == {"DOC-RECIPE", "DOC-PATH", "DOC-FILE", "DOC-TEST", "DOC-RULE-REF"}
    for line in heads:
        assert re.match(r"^error\[[A-Z0-9-]+\]: .+ \(.+:\d+\)$", line)
    assert "    evidence: " in text
    assert "    help: " in text


def test_gate_summary_count_matches_printed_details(tmp_path: Path) -> None:
    proc = run_gate(tmp_path, VIOLATION_BODY, "//! 见 AGENTS.md 质量条目“日志英文”。")
    text = proc.stderr.decode("utf-8")
    details = sum(1 for line in text.splitlines() if line.startswith("error["))
    match = re.search(r"^error: (\d+) violations? found \(see above\)$", text, re.M)
    assert match
    assert int(match.group(1)) == details == 5


def test_gate_output_deterministic_across_runs(tmp_path: Path) -> None:
    first = run_gate(tmp_path, VIOLATION_BODY, "//! 见 AGENTS.md 质量条目“日志英文”。")
    second = subprocess.run(
        [sys.executable, GATE, str(tmp_path / "AGENTS.md")], capture_output=True
    )
    assert first.returncode == second.returncode == 1
    assert first.stderr == second.stderr
    assert first.stdout == second.stdout


def test_gate_clean_fixture_passes_with_exit_zero(tmp_path: Path) -> None:
    proc = run_gate(tmp_path, "见 `just test`、`docs/note.md`")
    assert proc.returncode == 0
    assert proc.stdout.decode("utf-8").startswith("ok")


def test_gate_missing_doc_fails_closed_with_exit_three(tmp_path: Path) -> None:
    proc = subprocess.run([sys.executable, GATE, str(tmp_path / "AGENTS.md")], capture_output=True)
    assert proc.returncode == 3
    assert "error[DOC-INPUT]:" in proc.stderr.decode("utf-8")


def test_gate_extra_argument_is_a_usage_error(tmp_path: Path) -> None:
    proc = run_gate(tmp_path, "见 `just test`", None, "extra-arg")
    assert proc.returncode == 2
    assert b"usage:" in proc.stderr
