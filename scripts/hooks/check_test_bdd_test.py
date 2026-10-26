"""测试清单门禁的单测：双向核对、重复登记与表格结构校验逐条覆盖。

被测入口是门禁的运行函数，返回通过与失败两个列表；夹具建在测试框架（pytest）
的临时目录（一次性版本库，扫描面走版本库清单）；用法见持续集成的钩子工作流。
清单文件缺失与测试源码扫描面为空是失败关闭的前置故障。另含反馈契约三件套：
报告格式断言、汇总计数与确定性，经子进程跑脚本端到端锁输出与退出码。
"""

from __future__ import annotations

import re
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).parent))

import check_test_bdd  # noqa: E402
import gate_report  # noqa: E402

CARGO_TOML = """\
[package]
name = "gloss"
version.workspace = true
edition = "2024"

[[test]]
name = "overlay_selftest"
harness = false
"""

SRC_LIB = """\
#[test]
fn registered_plain() {}

#[tokio::test]
async fn registered_tokio() {}

#[test]
#[ignore = "needs auth"]
fn registered_ignored() {}

#[test] fn same_line_registered() {}

#[test]
fn snapshots_match_baseline() {}
"""

BDD_MD = """\
# 测试行为清单（BDD）

## 总览
| 类别 | 数量 | 运行 |
| --- | --- | --- |
| 单元测试 | 5 | `just test` |

### crates/demo.rs
| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| registered_plain | 目标 | 给定…当…则… | 2026-09-19 |
| registered_tokio | 目标 | 给定…当…则… | 2026-09-19 |
| registered_ignored | 目标 | 给定…当…则… | 2026-09-19 |
| same_line_registered | 目标 | 给定…当…则… | 2026-09-19 |
| snapshots_match_baseline（lib） | 目标 | 给定…当…则… | 2026-09-19 |

## 性能测试
| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| overlay_selftest | 目标 | 给定…当…则… | 2026-09-19 |
"""


def build_fixture(tmp_path: Path) -> None:
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    (tmp_path / "src").mkdir()
    (tmp_path / "docs/tests").mkdir(parents=True)
    (tmp_path / "Cargo.toml").write_text(CARGO_TOML, encoding="utf-8")
    (tmp_path / "src/lib.rs").write_text(SRC_LIB, encoding="utf-8")
    (tmp_path / "docs/tests/bdd.md").write_text(BDD_MD, encoding="utf-8")
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)


def check(
    tmp_path: Path, mutate: Callable[[Path], None] | None = None
) -> tuple[list[str], list[str]]:
    build_fixture(tmp_path)
    if mutate is not None:
        mutate(tmp_path)
    return check_test_bdd.run(str(tmp_path))


def assert_pass(
    tmp_path: Path, mutate: Callable[[Path], None] | None = None, needle: str | None = None
) -> None:
    oks, fails = check(tmp_path, mutate)
    assert fails == []
    if needle is not None:
        assert any(needle in o for o in oks)


def assert_fail(
    tmp_path: Path,
    mutate: Callable[[Path], None] | None = None,
    needle: str | None = None,
    absent: str | None = None,
) -> None:
    oks, fails = check(tmp_path, mutate)
    out = "\n".join(fails)
    assert fails
    if needle is not None:
        assert needle in out
    if absent is not None:
        assert absent not in out


# ---- 一致（通过）----


def test_full_inventory_consistent(tmp_path: Path) -> None:
    assert_pass(tmp_path)


def test_section_file_heading_rename_irrelevant(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        p = root / "docs/tests/bdd.md"
        p.write_text(
            p.read_text(encoding="utf-8").replace("### crates/demo.rs", "### crates/renamed.rs"),
            encoding="utf-8",
        )

    assert_pass(tmp_path, mutate)


def test_table_header_and_separator_not_entries(tmp_path: Path) -> None:
    assert_pass(tmp_path)


def test_history_row_same_name_needs_history_marker(tmp_path: Path) -> None:
    """同名旧行带「最新沿革见」指针＝沿革行，豁免实例数对账。"""

    def mutate(root: Path) -> None:
        p = root / "docs/tests/bdd.md"
        with open(p, "a", encoding="utf-8") as fh:
            fh.write("| registered_plain | 旧目标 | 旧场景措辞（最新沿革见首行） | 2026-09-01 |\n")

    assert_pass(tmp_path, mutate)


def test_second_row_without_history_marker_fails_instance_count(tmp_path: Path) -> None:
    """同名两行都没有沿革指针：清单行数超过源码实例数，实例数对账拦下。"""

    def mutate(root: Path) -> None:
        p = root / "docs/tests/bdd.md"
        with open(p, "a", encoding="utf-8") as fh:
            fh.write("| registered_plain | 旧目标 | 旧场景措辞 | 2026-09-01 |\n")

    assert_fail(tmp_path, mutate, "registered_plain (2 inventory rows vs 1 source instances)")


def test_fn_name_colliding_with_target_deduped(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        with open(root / "Cargo.toml", "a", encoding="utf-8") as fh:
            fh.write('\n[[test]]\nname = "collide_target"\nharness = false\n')
        with open(root / "src/lib.rs", "a", encoding="utf-8") as fh:
            fh.write("\n#[test]\nfn collide_target() {}\n")
        with open(root / "docs/tests/bdd.md", "a", encoding="utf-8") as fh:
            fh.write("| collide_target | 目标 | 给定…当…则… | 2026-09-19 |\n")

    assert_pass(tmp_path, mutate)


def test_manual_entry_registered_once(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        with open(root / "src/lib.rs", "a", encoding="utf-8") as fh:
            fh.write('\n#[test]\n#[ignore = "needs device"]\nfn live_manual_thing() {}\n')
        with open(root / "docs/tests/bdd.md", "a", encoding="utf-8") as fh:
            fh.write(
                "\n### live_manual_thing\n- 测试目标：探针。\n- 测试步骤：\n"
                "  1. 跑一次\n- 更新时间：2026-09-19\n"
            )

    assert_pass(tmp_path, mutate)


# ---- 唯一性三道检查：限定名唯一、实例数对账、限定符文件绑定 ----


def add_second_instance(root: Path) -> None:
    """同名测试的第二个定义文件与两条文件绑定限定行（新文件须入库才进扫描面）。"""
    mod = root / "src/ui/popup"
    mod.mkdir(parents=True)
    (mod / "mod.rs").write_text("#[test]\nfn snapshots_match_baseline() {}\n", encoding="utf-8")
    with open(root / "docs/tests/bdd.md", "a", encoding="utf-8") as fh:
        fh.write("| snapshots_match_baseline（popup） | 目标 | 给定…当…则… | 2026-09-19 |\n")
    subprocess.run(["git", "-C", str(root), "add", "-A"], check=True, capture_output=True)


def test_cross_file_same_name_with_bound_qualifiers_passes(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        add_second_instance(root)

    assert_pass(tmp_path, mutate)


def test_cross_file_same_name_without_qualifier_fails_binding(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        add_second_instance(root)
        p = root / "docs/tests/bdd.md"
        text = p.read_text(encoding="utf-8").replace(
            "snapshots_match_baseline（popup）", "snapshots_match_baseline"
        )
        p.write_text(text, encoding="utf-8")

    assert_fail(tmp_path, mutate, "needs a file-bound qualifier")


def test_qualifier_with_free_text_fails_binding(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        p = root / "docs/tests/bdd.md"
        text = p.read_text(encoding="utf-8").replace(
            "snapshots_match_baseline（lib）", "snapshots_match_baseline（lib 旧措辞）"
        )
        p.write_text(text, encoding="utf-8")

    assert_fail(tmp_path, mutate, "is not a plain file-name token")


def test_qualifier_binds_mod_files_by_parent_dir() -> None:
    """mod 文件的限定符绑定是父目录名，直接锁定绑定函数的分支。"""
    assert check_test_bdd.qualifier_binds("popup", "src/ui/popup/mod.rs")
    assert not check_test_bdd.qualifier_binds("mod", "src/ui/popup/mod.rs")
    assert check_test_bdd.qualifier_binds("settings", "src/ui/settings.rs")
    assert not check_test_bdd.qualifier_binds("popup", "src/ui/settings.rs")


def test_qualifier_matching_no_file_fails_binding(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        p = root / "docs/tests/bdd.md"
        text = p.read_text(encoding="utf-8").replace(
            "snapshots_match_baseline（lib）", "snapshots_match_baseline（nowhere）"
        )
        p.write_text(text, encoding="utf-8")

    assert_fail(tmp_path, mutate, "matches none of")


def test_two_rows_binding_same_file_fail_dup_qualifier(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        add_second_instance(root)
        p = root / "docs/tests/bdd.md"
        text = p.read_text(encoding="utf-8").replace(
            "snapshots_match_baseline（lib）", "snapshots_match_baseline（popup）"
        )
        p.write_text(text, encoding="utf-8")

    assert_fail(tmp_path, mutate, "bound by two rows")


def test_uncovered_instance_fails_binding(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        add_second_instance(root)
        p = root / "docs/tests/bdd.md"
        text = p.read_text(encoding="utf-8").replace(
            "snapshots_match_baseline（lib）", "snapshots_match_baseline（popup）"
        )
        p.write_text(text, encoding="utf-8")

    assert_fail(tmp_path, mutate, "no qualifier bound to")


def test_history_row_exempt_from_qualifier_checks(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        add_second_instance(root)
        with open(root / "docs/tests/bdd.md", "a", encoding="utf-8") as fh:
            fh.write(
                "| snapshots_match_baseline | 旧目标 | 旧场景（最新沿革见限定行） | 2026-09-01 |\n"
            )

    assert_pass(tmp_path, mutate)


# ---- 不一致（拒绝）----


def test_unregistered_test(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        with open(root / "src/lib.rs", "a", encoding="utf-8") as fh:
            fh.write("\n#[test]\nfn unregistered_thing() {}\n")

    assert_fail(tmp_path, mutate, "unregistered_thing")


def test_stale_bdd_entry(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        with open(root / "docs/tests/bdd.md", "a", encoding="utf-8") as fh:
            fh.write("| ghost_entry | 目标 | 场景 | 2026-09-19 |\n")

    assert_fail(tmp_path, mutate, "ghost_entry")


def test_rename_in_code_only(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        p = root / "src/lib.rs"
        p.write_text(
            p.read_text(encoding="utf-8").replace(
                "fn registered_plain() {}", "fn registered_plain_renamed() {}"
            ),
            encoding="utf-8",
        )

    assert_fail(tmp_path, mutate, "registered_plain_renamed")


def test_bdd_file_missing(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        (root / "docs/tests/bdd.md").unlink()

    with pytest.raises(SystemExit) as exc:
        check(tmp_path, mutate)
    assert exc.value.code == gate_report.EXIT_INFRA


def test_empty_scan_surface_fails_closed(tmp_path: Path) -> None:
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    (tmp_path / "docs/tests").mkdir(parents=True)
    (tmp_path / "docs/tests/bdd.md").write_text(BDD_MD, encoding="utf-8")
    (tmp_path / "Cargo.toml").write_text('[package]\nname = "gloss"\n', encoding="utf-8")
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    with pytest.raises(SystemExit) as exc:
        check_test_bdd.run(str(tmp_path))
    assert exc.value.code == gate_report.EXIT_INFRA


def test_exact_duplicate_row(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        with open(root / "docs/tests/bdd.md", "a", encoding="utf-8") as fh:
            fh.write("| registered_plain | 目标 | 给定…当…则… | 2026-09-19 |\n")

    assert_fail(tmp_path, mutate, "registered multiple times")


def test_section_header_row_dropped(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        p = root / "docs/tests/bdd.md"
        lines = [
            line
            for line in p.read_text(encoding="utf-8").split("\n")
            if not (line.startswith("| 测试名称") or line.startswith("| ---"))
        ]
        p.write_text("\n".join(lines), encoding="utf-8")

    assert_fail(tmp_path, mutate, "missing a header row")


def test_duplicate_chapter(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        with open(root / "docs/tests/bdd.md", "a", encoding="utf-8") as fh:
            fh.write("\n## 性能测试\n")

    assert_fail(tmp_path, mutate, "appear more than once")


def test_blank_line_splits_table(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        p = root / "docs/tests/bdd.md"
        lines = p.read_text(encoding="utf-8").split("\n")
        out = []
        done = False
        for line in lines:
            out.append(line)
            if line.startswith("| ---") and not done:
                out.append("")
                done = True
        p.write_text("\n".join(out), encoding="utf-8")

    assert_fail(tmp_path, mutate, "missing a header row")


def test_paragraph_splits_table(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        p = root / "docs/tests/bdd.md"
        lines = p.read_text(encoding="utf-8").split("\n")
        out = []
        done = False
        for line in lines:
            out.append(line)
            if line.startswith("| ---") and not done:
                out.append("段落混进了表体。")
                done = True
        p.write_text("\n".join(out), encoding="utf-8")

    assert_fail(tmp_path, mutate, "missing a header row")


def test_table_header_dropped_body_only(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        p = root / "docs/tests/bdd.md"
        lines = p.read_text(encoding="utf-8").split("\n")
        out = []
        dropped = 0
        for line in lines:
            if line.startswith("| 测试名称") and dropped == 0:
                dropped = 1
                continue
            if dropped == 1 and line.startswith("| ---"):
                dropped = 2
                continue
            out.append(line)
        p.write_text("\n".join(out), encoding="utf-8")

    assert_fail(tmp_path, mutate, "missing a header row")


def test_duplicate_bullet_lines(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        with open(root / "docs/tests/bdd.md", "a", encoding="utf-8") as fh:
            fh.write("- 测试目标：重复块探针。\n- 测试目标：重复块探针。\n")

    assert_fail(tmp_path, mutate, "repeat within the same")


def test_duplicate_h3_in_same_chapter(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        with open(root / "docs/tests/bdd.md", "a", encoding="utf-8") as fh:
            fh.write("\n### crates/demo.rs\n")
            fh.write("\n### crates/demo.rs\n")

    assert_fail(tmp_path, mutate, "repeat within the same")


# ---- 反馈契约：报告格式、汇总计数、确定性与退出码（子进程端到端）----

GATE = str(Path(__file__).with_name("check_test_bdd.py"))


def run_gate(
    tmp_path: Path, mutate: Callable[[Path], None] | None = None, *extra: str
) -> subprocess.CompletedProcess[bytes]:
    build_fixture(tmp_path)
    if mutate is not None:
        mutate(tmp_path)
    return subprocess.run([sys.executable, GATE, str(tmp_path)] + list(extra), capture_output=True)


def violation_fixture(root: Path) -> None:
    def mutate_src(path: Path) -> None:
        with open(path, "a", encoding="utf-8") as fh:
            fh.write("\n#[test]\nfn unregistered_thing() {}\n")

    mutate_src(root / "src/lib.rs")
    # 同名测试的第二个定义文件 + 两行同限定行：触发文件绑定与限定名重复
    mod = root / "src/ui/popup"
    mod.mkdir(parents=True)
    (mod / "mod.rs").write_text("#[test]\nfn snapshots_match_baseline() {}\n", encoding="utf-8")
    subprocess.run(["git", "-C", str(root), "add", "-A"], check=True, capture_output=True)
    with open(root / "docs/tests/bdd.md", "a", encoding="utf-8") as fh:
        fh.write(
            "\n## 性能测试\n"
            "| 测试名称 | 测试目标 | 测试场景 | 更新时间 |\n"
            "| --- | --- | --- | --- |\n"
            "\n"
            "| ghost_entry | 目标 | 场景 | 2026-09-19 |\n"
            "| registered_plain | 目标 | 给定…当…则… | 2026-09-19 |\n"
            "| snapshots_match_baseline（popup） | 目标 | 给定…当…则… | 2026-09-19 |\n"
            "| snapshots_match_baseline（popup） | 目标 | 给定…当…则… | 2026-09-19 |\n"
            "| snapshots_match_baseline | 目标 | 给定…当…则… | 2026-09-19 |\n"
            "- 测试目标：重复块探针。\n"
            "- 测试目标：重复块探针。\n"
        )


def test_gate_reports_rule_ids_and_violation_exit_code(tmp_path: Path) -> None:
    proc = run_gate(tmp_path, violation_fixture)
    assert proc.returncode == 1
    text = proc.stderr.decode("utf-8")
    heads = [line for line in text.splitlines() if line.startswith("error[")]
    ids = {re.match(r"^error\[([A-Z0-9-]+)\]:", line).group(1) for line in heads}
    assert ids == {
        "BDD-STALE",
        "BDD-MISSING",
        "BDD-DUP-ROW",
        "BDD-DUP-CHAPTER",
        "BDD-DUP-BLOCK",
        "BDD-TABLE",
        "BDD-INSTANCE",
        "BDD-BINDING",
        "BDD-DUP-QUALIFIER",
    }
    for line in heads:
        assert re.match(r"^error\[[A-Z0-9-]+\]: .+ \(.+\)$", line)
    assert "    evidence: " in text
    assert "    help: " in text


def test_gate_summary_count_matches_printed_details(tmp_path: Path) -> None:
    proc = run_gate(tmp_path, violation_fixture)
    text = proc.stderr.decode("utf-8")
    details = sum(1 for line in text.splitlines() if line.startswith("error["))
    match = re.search(r"^error: (\d+) violations? found \(see above\)$", text, re.M)
    assert match
    assert int(match.group(1)) == details == 9


def test_gate_output_deterministic_across_runs(tmp_path: Path) -> None:
    first = run_gate(tmp_path, violation_fixture)
    second = subprocess.run([sys.executable, GATE, str(tmp_path)], capture_output=True)
    assert first.returncode == second.returncode == 1
    assert first.stderr == second.stderr
    assert first.stdout == second.stdout


def test_gate_clean_fixture_passes_with_exit_zero(tmp_path: Path) -> None:
    proc = run_gate(tmp_path)
    assert proc.returncode == 0
    assert "  ok: " in proc.stdout.decode("utf-8")


def test_gate_missing_inventory_fails_closed_with_exit_three(tmp_path: Path) -> None:
    run_gate(tmp_path)
    (tmp_path / "docs/tests/bdd.md").unlink()
    proc = subprocess.run([sys.executable, GATE, str(tmp_path)], capture_output=True)
    assert proc.returncode == 3
    assert "error[INFRA]:" in proc.stderr.decode("utf-8")


def test_gate_empty_scan_surface_fails_closed_with_exit_three(tmp_path: Path) -> None:
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    (tmp_path / "docs/tests").mkdir(parents=True)
    (tmp_path / "docs/tests/bdd.md").write_text(BDD_MD, encoding="utf-8")
    (tmp_path / "Cargo.toml").write_text('[package]\nname = "gloss"\n', encoding="utf-8")
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    proc = subprocess.run([sys.executable, GATE, str(tmp_path)], capture_output=True)
    assert proc.returncode == 3
    assert "error[EMPTY-SURFACE]:" in proc.stderr.decode("utf-8")


def test_gate_extra_argument_is_a_usage_error(tmp_path: Path) -> None:
    proc = run_gate(tmp_path, None, "extra-arg")
    assert proc.returncode == 2
    assert b"usage:" in proc.stderr


def test_gate_missing_root_is_a_usage_error(tmp_path: Path) -> None:
    proc = subprocess.run([sys.executable, GATE, str(tmp_path / "nowhere")], capture_output=True)
    assert proc.returncode == 2
    assert b"usage:" in proc.stderr
