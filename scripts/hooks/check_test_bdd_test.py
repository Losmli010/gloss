"""测试清单门禁的单测：双向核对、重复登记与表格结构校验逐条覆盖。

被测入口是门禁的运行函数，返回通过与失败两个列表；夹具建在测试框架（pytest）
的临时目录（一次性版本库，扫描面走版本库清单）；用法见持续集成的钩子工作流。
"""

import subprocess
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).parent))

import check_test_bdd  # noqa: E402

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
| snapshots_match_baseline（popup） | 目标 | 给定…当…则… | 2026-09-19 |

## 性能测试
| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| overlay_selftest | 目标 | 给定…当…则… | 2026-09-19 |
"""


def build_fixture(tmp_path):
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    (tmp_path / "src").mkdir()
    (tmp_path / "docs/tests").mkdir(parents=True)
    (tmp_path / "Cargo.toml").write_text(CARGO_TOML, encoding="utf-8")
    (tmp_path / "src/lib.rs").write_text(SRC_LIB, encoding="utf-8")
    (tmp_path / "docs/tests/bdd.md").write_text(BDD_MD, encoding="utf-8")
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)


def check(tmp_path, mutate=None):
    build_fixture(tmp_path)
    if mutate is not None:
        mutate(tmp_path)
    return check_test_bdd.run(str(tmp_path))


def assert_pass(tmp_path, mutate=None, needle=None):
    oks, fails = check(tmp_path, mutate)
    assert fails == []
    if needle is not None:
        assert any(needle in o for o in oks)


def assert_fail(tmp_path, mutate=None, needle=None, absent=None):
    oks, fails = check(tmp_path, mutate)
    out = "\n".join(fails)
    assert fails
    if needle is not None:
        assert needle in out
    if absent is not None:
        assert absent not in out


# ---- 一致（通过）----


def test_full_inventory_consistent(tmp_path):
    assert_pass(tmp_path)


def test_section_file_heading_rename_irrelevant(tmp_path):
    def mutate(root):
        p = root / "docs/tests/bdd.md"
        p.write_text(
            p.read_text(encoding="utf-8").replace("### crates/demo.rs", "### crates/renamed.rs"),
            encoding="utf-8",
        )

    assert_pass(tmp_path, mutate)


def test_table_header_and_separator_not_entries(tmp_path):
    assert_pass(tmp_path)


def test_history_row_same_name_not_duplicate(tmp_path):
    def mutate(root):
        p = root / "docs/tests/bdd.md"
        with open(p, "a", encoding="utf-8") as fh:
            fh.write("| registered_plain | 旧目标 | 旧场景措辞 | 2026-09-01 |\n")

    assert_pass(tmp_path, mutate)


def test_fn_name_colliding_with_target_deduped(tmp_path):
    def mutate(root):
        with open(root / "Cargo.toml", "a", encoding="utf-8") as fh:
            fh.write('\n[[test]]\nname = "collide_target"\nharness = false\n')
        with open(root / "src/lib.rs", "a", encoding="utf-8") as fh:
            fh.write("\n#[test]\nfn collide_target() {}\n")
        with open(root / "docs/tests/bdd.md", "a", encoding="utf-8") as fh:
            fh.write("| collide_target | 目标 | 给定…当…则… | 2026-09-19 |\n")

    assert_pass(tmp_path, mutate)


def test_manual_entry_registered_once(tmp_path):
    def mutate(root):
        with open(root / "src/lib.rs", "a", encoding="utf-8") as fh:
            fh.write('\n#[test]\n#[ignore = "needs device"]\nfn live_manual_thing() {}\n')
        with open(root / "docs/tests/bdd.md", "a", encoding="utf-8") as fh:
            fh.write(
                "\n### live_manual_thing\n- 测试目标：探针。\n- 测试步骤：\n"
                "  1. 跑一次\n- 更新时间：2026-09-19\n"
            )

    assert_pass(tmp_path, mutate)


# ---- 不一致（拒绝）----


def test_unregistered_test(tmp_path):
    def mutate(root):
        with open(root / "src/lib.rs", "a", encoding="utf-8") as fh:
            fh.write("\n#[test]\nfn unregistered_thing() {}\n")

    assert_fail(tmp_path, mutate, "unregistered_thing")


def test_stale_bdd_entry(tmp_path):
    def mutate(root):
        with open(root / "docs/tests/bdd.md", "a", encoding="utf-8") as fh:
            fh.write("| ghost_entry | 目标 | 场景 | 2026-09-19 |\n")

    assert_fail(tmp_path, mutate, "ghost_entry")


def test_rename_in_code_only(tmp_path):
    def mutate(root):
        p = root / "src/lib.rs"
        p.write_text(
            p.read_text(encoding="utf-8").replace(
                "fn registered_plain() {}", "fn registered_plain_renamed() {}"
            ),
            encoding="utf-8",
        )

    assert_fail(tmp_path, mutate, "registered_plain_renamed")


def test_bdd_file_missing(tmp_path):
    def mutate(root):
        (root / "docs/tests/bdd.md").unlink()

    with pytest.raises(SystemExit, match="not found"):
        check(tmp_path, mutate)


def test_exact_duplicate_row(tmp_path):
    def mutate(root):
        with open(root / "docs/tests/bdd.md", "a", encoding="utf-8") as fh:
            fh.write("| registered_plain | 目标 | 给定…当…则… | 2026-09-19 |\n")

    assert_fail(tmp_path, mutate, "registered multiple times")


def test_section_header_row_dropped(tmp_path):
    def mutate(root):
        p = root / "docs/tests/bdd.md"
        lines = [
            line
            for line in p.read_text(encoding="utf-8").split("\n")
            if not (line.startswith("| 测试名称") or line.startswith("| ---"))
        ]
        p.write_text("\n".join(lines), encoding="utf-8")

    assert_fail(tmp_path, mutate, "missing a header row")


def test_duplicate_chapter(tmp_path):
    def mutate(root):
        with open(root / "docs/tests/bdd.md", "a", encoding="utf-8") as fh:
            fh.write("\n## 性能测试\n")

    assert_fail(tmp_path, mutate, "appear more than once")


def test_blank_line_splits_table(tmp_path):
    def mutate(root):
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


def test_paragraph_splits_table(tmp_path):
    def mutate(root):
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


def test_table_header_dropped_body_only(tmp_path):
    def mutate(root):
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


def test_duplicate_bullet_lines(tmp_path):
    def mutate(root):
        with open(root / "docs/tests/bdd.md", "a", encoding="utf-8") as fh:
            fh.write("- 测试目标：重复块探针。\n- 测试目标：重复块探针。\n")

    assert_fail(tmp_path, mutate, "repeat within the same")


def test_duplicate_h3_in_same_chapter(tmp_path):
    def mutate(root):
        with open(root / "docs/tests/bdd.md", "a", encoding="utf-8") as fh:
            fh.write("\n### crates/demo.rs\n")
            fh.write("\n### crates/demo.rs\n")

    assert_fail(tmp_path, mutate, "repeat within the same")
