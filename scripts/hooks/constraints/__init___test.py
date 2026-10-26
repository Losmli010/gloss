"""约束门禁·路由聚合的单测：基线夹具经聚合入口断言全部十二条通过行按模块
序齐全——少挂一个领域模块或漏一条约束即变红；违规夹具断言判罚穿透聚合、
对应的条件通过行随之缺席。另含反馈契约三件套与守卫的端到端用例：经子进程
跑命令入口，锁定报告格式、汇总计数、确定性与退出码语义，空扫描面失败关闭、
非版本库环境响亮跳过、单文件报告超限声明剩余数。
"""

from __future__ import annotations

import re
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

import constraints  # noqa: E402
from constraints import fixtures  # noqa: E402

OK_STEMS = (
    "ok dependency direction",
    "ok eval asset isolation",
    "ok unified log exit",
    "ok log messages English-only",
    "ok egui context unified load point",
    "ok FFI declarations centralized",
    "ok single version source",
    "ok default features off",
    "ok no dead dependencies",
    "ok stub copies identical",
    "ok line cap",
    "ok no leftover task markers",
)


def test_baseline_aggregate_reports_every_domain(tmp_path: Path) -> None:
    fixtures.new_fixture(tmp_path)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    oks, fails = constraints.run(str(tmp_path))
    assert fails == []
    assert [ok.split(" (", 1)[0] for ok in oks] == list(OK_STEMS)


def test_aggregate_surfaces_domain_violations(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        fixtures.add_dep(
            root / "crates/gloss-app/Cargo.toml", 'gloss-eval = { path = "../gloss-eval" }'
        )

    oks, fails = fixtures.run_case(constraints, tmp_path, mutate)
    assert any("must stay out of the production dependency graph" in f for f in fails)
    assert not any(ok.startswith("ok eval asset isolation") for ok in oks)


GATE = str(Path(__file__).parent / "__main__.py")


def run_gate(tmp_path: Path, *extra: str) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run([sys.executable, GATE, str(tmp_path)] + list(extra), capture_output=True)


def git_repo(tmp_path: Path, mutate: Callable[[Path], None] | None = None) -> None:
    fixtures.new_fixture(tmp_path)
    if mutate is not None:
        mutate(tmp_path)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)


def violation_mutate(root: Path) -> None:
    fixtures.add_dep(
        root / "crates/gloss-app/Cargo.toml",
        'gloss-eval = { path = "../gloss-eval" }\n'
        'tracing = { version = "0.1", default-features = false }',
    )
    (root / "crates/gloss-app/src").mkdir(exist_ok=True)
    (root / "crates/gloss-app/src/bare.rs").write_text(
        "let ctx = egui::Context::default(); // tail over forty characters kept as evidence\n"
    )
    (root / "crates/gloss-core/src/log.rs").write_text('info!("找不到配置文件");\n')
    (root / "crates/gloss-platform/src").mkdir(exist_ok=True)
    (root / "crates/gloss-platform/src/sneaky.rs").write_text(
        'unsafe extern "C" {\n    fn f();\n}\n'
    )


VIOLATION_RULES = {
    "DEP-DIRECTION",
    "EVAL-DEP",
    "LOG-EXIT",
    "LOG-ENGLISH",
    "EGUI-CTX",
    "FFI-CENTRAL",
    "DEAD-DEP",
}


def test_gate_reports_rule_ids_and_violation_exit_code(tmp_path: Path) -> None:
    git_repo(tmp_path, violation_mutate)
    proc = run_gate(tmp_path)
    assert proc.returncode == 1
    text = proc.stderr.decode("utf-8")
    heads = [line for line in text.splitlines() if line.startswith("error[")]
    ids = {re.match(r"^error\[([A-Z0-9-]+)\]:", line).group(1) for line in heads}
    assert ids == VIOLATION_RULES
    assert len(heads) == 8
    for line in heads:
        assert re.match(r"^error\[[A-Z0-9-]+\]: .+ \([^)]+\)$", line)
    assert "    evidence: " in text
    assert "    help: " in text
    assert 'evidence: unsafe extern "C" {' in text
    assert "tail over forty characters kept as evidence" in text


def test_gate_summary_count_matches_printed_details(tmp_path: Path) -> None:
    git_repo(tmp_path, violation_mutate)
    proc = run_gate(tmp_path)
    text = proc.stderr.decode("utf-8")
    details = sum(1 for line in text.splitlines() if line.startswith("error["))
    match = re.search(r"^error: (\d+) violations? found \(see above\)$", text, re.M)
    assert match
    assert int(match.group(1)) == details == 8


def test_gate_output_deterministic_across_runs(tmp_path: Path) -> None:
    git_repo(tmp_path, violation_mutate)
    first = run_gate(tmp_path)
    second = run_gate(tmp_path)
    assert first.returncode == second.returncode == 1
    assert first.stderr == second.stderr
    assert first.stdout == second.stdout


def test_gate_clean_repo_passes_with_exit_zero(tmp_path: Path) -> None:
    git_repo(tmp_path)
    proc = run_gate(tmp_path)
    assert proc.returncode == 0
    assert b"constraint checks passed (12 checks)" in proc.stdout


def test_gate_extra_argument_is_a_usage_error(tmp_path: Path) -> None:
    git_repo(tmp_path)
    proc = run_gate(tmp_path, "extra-arg")
    assert proc.returncode == 2
    assert b"usage:" in proc.stderr


def test_gate_missing_root_is_a_usage_error(tmp_path: Path) -> None:
    proc = subprocess.run([sys.executable, GATE, str(tmp_path / "nowhere")], capture_output=True)
    assert proc.returncode == 2
    assert b"usage:" in proc.stderr


def test_gate_empty_manifest_surface_exits_three(tmp_path: Path) -> None:
    proc = run_gate(tmp_path)
    assert proc.returncode == 3
    assert "error[EMPTY-SURFACE]:" in proc.stderr.decode("utf-8")


def manifest_only_fixture(tmp_path: Path) -> None:
    (tmp_path / "Cargo.toml").write_text(
        '[package]\nname = "gloss"\nversion.workspace = true\nedition.workspace = true\n'
        '\n[dependencies]\n\n[workspace.package]\nversion = "0.1.0"\nedition = "2024"',
        encoding="utf-8",
    )


def test_gate_empty_source_surface_exits_three(tmp_path: Path) -> None:
    manifest_only_fixture(tmp_path)
    proc = run_gate(tmp_path)
    assert proc.returncode == 3
    assert "error[EMPTY-SURFACE]:" in proc.stderr.decode("utf-8")


def test_gate_non_repo_environment_loud_skips(tmp_path: Path) -> None:
    fixtures.new_fixture(tmp_path)
    proc = run_gate(tmp_path)
    assert proc.returncode == 0
    assert b"constraint checks passed (11 checks)" in proc.stdout
    assert "task-marker scan skipped" in proc.stderr.decode("utf-8")


def cap_mutate(root: Path) -> None:
    for i in range(25):
        fixtures.add_dep(
            root / "crates/gloss-core/Cargo.toml",
            'extra%02d = { version = "1", default-features = false }' % i,
        )


def test_gate_caps_report_per_file_and_declares_rest(tmp_path: Path) -> None:
    fixtures.new_fixture(tmp_path)
    cap_mutate(tmp_path)
    _oks, fails = constraints.run(str(tmp_path))
    assert len(fails) == 25
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    proc = run_gate(tmp_path)
    assert proc.returncode == 1
    text = proc.stderr.decode("utf-8")
    heads = [line for line in text.splitlines() if line.startswith("error[")]
    assert len(heads) == 20
    assert all(re.match(r"^error\[DEAD-DEP\]:", line) for line in heads)
    assert "… 5 more violations in crates/gloss-core/Cargo.toml not shown" in text
    match = re.search(r"^error: (\d+) violations? found \(see above\)$", text, re.M)
    assert match
    assert int(match.group(1)) == 20
