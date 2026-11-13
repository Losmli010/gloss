"""测试清单总览生成器的单测：计数口径、包前缀推导、列序与写回行为逐条覆盖。

生成与校验共用本模块，本单测锁三件事：计数口径（表格计登记行，表头与分隔
行除外，条目小节按标题计，路径形态的小节标题不是条目）、包前缀与列序的机
械推导、写回幂等（再生一次后重复再生不落盘）。夹具建在测试框架（pytest）
的临时目录，不走版本库清单。
"""

from __future__ import annotations

from pathlib import Path

import test_index

ROOT_MD = """\
# 测试行为清单（BDD）

## 总览

| 类别 | 数量 | 运行 |
| --- | --- | --- |
| 旧总览 | 1 | 占位 |

## 人工测试

### watch_and_reader_carry_a_real_image_copy
- 测试目标：探针。
- 更新时间：2026-09-19
"""

UNIT_MD = """\
# 测试行为清单（gloss-core）

## 单元测试

### `crates/gloss-core/src/log.rs`
| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| init_is_idempotent | 目标 | 给定…当…则… | 2026-09-19 |
| filter_falls_back | 目标 | 给定…当…则… | 2026-09-19 |

### `crates/gloss-core/tests/stubs_behavior.rs`
| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| config_store_mock_round_trips | 目标 | 给定…当…则… | 2026-09-19 |
"""


def write_repo(tmp_path: Path) -> None:
    docs = tmp_path / "docs" / "tests"
    docs.mkdir(parents=True)
    (docs / "bdd.md").write_text(ROOT_MD, encoding="utf-8")
    (docs / "gloss-core.md").write_text(UNIT_MD, encoding="utf-8")


# ---- 包前缀与列序的机械推导 ----


def test_package_prefix_derives_from_file_stem() -> None:
    assert test_index.package_prefix("docs/tests/gloss-core.md") == "crates/gloss-core/src/"
    assert test_index.package_prefix("docs/tests/gloss.md") == "src/"
    assert test_index.package_prefix("docs/tests/bdd.md") is None
    assert test_index.package_prefix("docs/tests/notes.md") is None


def test_column_order_puts_root_first_then_layers() -> None:
    rels = ["docs/tests/gloss.md", "docs/tests/bdd.md", "docs/tests/gloss-app.md"]
    assert test_index.column_order(rels) == [
        "docs/tests/bdd.md",
        "docs/tests/gloss-app.md",
        "docs/tests/gloss.md",
    ]


# ---- 计数口径 ----


def test_heading_path_strips_qualifier_and_backticks() -> None:
    assert (
        test_index.heading_path("### `crates/gloss-app/src/flow/task.rs`（预热发送端）")
        == "crates/gloss-app/src/flow/task.rs"
    )
    assert test_index.heading_path("### `src/main.rs`") == "src/main.rs"
    assert test_index.heading_path("### live_manual_thing") is None


def test_chapter_counts_rows_and_manual_headings() -> None:
    counts = test_index.chapter_counts(UNIT_MD)
    # 表格计登记行（表头与分隔行除外），路径形态的小节标题不是条目
    assert counts == {"单元测试": 3}


def test_chapter_counts_manual_headings_and_skips_overview() -> None:
    counts = test_index.chapter_counts(ROOT_MD)
    # 人工测试的条目是小节标题；总览章是生成产物不计
    assert counts == {"人工测试": 1}


def test_line_count_matches_physical_lines() -> None:
    assert test_index.line_count("a\nb\n") == 2
    assert test_index.line_count("a\nb") == 2


# ---- 渲染与写回 ----


def test_regenerate_replaces_overview_and_is_idempotent(tmp_path: Path) -> None:
    write_repo(tmp_path)
    root = str(tmp_path)
    assert test_index.regenerate(root) is True
    text = (tmp_path / "docs" / "tests" / "bdd.md").read_text(encoding="utf-8")
    assert "旧总览" not in text
    assert "| 人工测试 | 1 | 0 | 1 |" in text
    assert "| 单元测试 | 0 | 3 | 3 |" in text
    assert "登记文件行数：`bdd.md`（本文件）" in text
    assert test_index.regenerate(root) is False


def test_regenerated_overview_keeps_blank_line_before_next_chapter(tmp_path: Path) -> None:
    write_repo(tmp_path)
    test_index.regenerate(str(tmp_path))
    text = (tmp_path / "docs" / "tests" / "bdd.md").read_text(encoding="utf-8")
    assert "。\n\n## 人工测试" in text


def test_regenerate_inserts_overview_when_missing(tmp_path: Path) -> None:
    docs = tmp_path / "docs" / "tests"
    docs.mkdir(parents=True)
    (docs / "bdd.md").write_text("# 测试行为清单（BDD）\n\n## 人工测试\n", encoding="utf-8")
    (docs / "gloss.md").write_text(UNIT_MD, encoding="utf-8")
    assert test_index.regenerate(str(tmp_path)) is True
    text = (docs / "bdd.md").read_text(encoding="utf-8")
    assert text.index("## 总览") < text.index("## 人工测试")


def test_regenerate_requires_root_inventory(tmp_path: Path) -> None:
    docs = tmp_path / "docs" / "tests"
    docs.mkdir(parents=True)
    (docs / "gloss.md").write_text(UNIT_MD, encoding="utf-8")
    try:
        test_index.regenerate(str(tmp_path))
        raise AssertionError("expected SystemExit for missing root inventory")
    except SystemExit as exc:
        assert "bdd.md" in str(exc)
