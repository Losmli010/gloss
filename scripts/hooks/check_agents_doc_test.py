"""check_agents_doc.py 的单测：逐条覆盖五类引用校验与跳过规则。

被测函数是 check_agents_doc.run(doc)（返回 (violations, checked, ref_files,
ref_count)），夹具建在 pytest 的 tmp_path。用法：
    python3 -m pytest scripts/hooks/check_agents_doc_test.py
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import check_agents_doc  # noqa: E402

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


def build_fixture(tmp_path):
    for d in ("crates/demo/tests", "crates/demo/src", "tests", "docs"):
        (tmp_path / d).mkdir(parents=True, exist_ok=True)
    (tmp_path / "Cargo.toml").write_text("")
    (tmp_path / "kittest.toml").write_text("")
    (tmp_path / "tests/overlay_selftest.rs").write_text("")
    (tmp_path / "crates/demo/tests/pipeline.rs").write_text("")
    (tmp_path / "crates/demo/src/lib.rs").write_text("")
    (tmp_path / "docs/note.md").write_text("# 说明文档\n")
    (tmp_path / "justfile").write_text(JUSTFILE)


def check(tmp_path, body, ref_body=None):
    """写入 AGENTS.md（可选 ref_body 进 crates/demo/src/lib.rs）并运行门禁。"""
    build_fixture(tmp_path)
    (tmp_path / "AGENTS.md").write_text(body + "\n", encoding="utf-8")
    if ref_body is not None:
        (tmp_path / "crates/demo/src/lib.rs").write_text(ref_body + "\n", encoding="utf-8")
    return check_agents_doc.run(str(tmp_path / "AGENTS.md"))


def assert_pass(tmp_path, body, ref_body=None):
    violations, _, _, _ = check(tmp_path, body, ref_body)
    assert violations == []


def assert_fail(tmp_path, body, needle, ref_body=None):
    violations, _, _, _ = check(tmp_path, body, ref_body)
    assert any(needle in v for v in violations)


# ---- 合法用例 ----


def test_all_references_exist(tmp_path):
    body = "见 `just test`、`just logs`、`just logs-dir`、`docs/note.md`、`Cargo.toml`"
    assert_pass(tmp_path, body)


def test_recipe_with_default_arg(tmp_path):
    assert_pass(tmp_path, "对照跑 `just bench-check`")


def test_repo_paths_file_and_dir(tmp_path):
    assert_pass(tmp_path, "产物在 `crates/demo/tests/`，配置见 `kittest.toml`")


def test_cargo_test_targets_exist(tmp_path):
    body = "跑 `cargo test --test overlay_selftest` 与 `cargo test --test pipeline`"
    assert_pass(tmp_path, body)


def test_placeholders_and_outside_paths_skipped(tmp_path):
    body = "分支名 `feat/<主题>`，日志在 `~/.gloss/logs/`，模式 `*.workspace = true`"
    assert_pass(tmp_path, body)


def test_comment_syntax_and_urls_not_paths(tmp_path):
    body = "每个 unsafe 块带 `// SAFETY:` 注释，文档见 `https://example.com/a/b`"
    assert_pass(tmp_path, body)


def test_justfile_and_just_list_not_recipes(tmp_path):
    assert_pass(tmp_path, "全部 recipe 见 `justfile`，用 `just --list` 查看")


def test_recipe_in_code_block_checked(tmp_path):
    body = "命令清单：\n\n    just run\n    just test"
    assert_pass(tmp_path, body)


# ---- 非法用例 ----


def test_missing_recipe(tmp_path):
    assert_fail(tmp_path, "跑 `just deploy` 发布", "「just deploy」")


def test_plain_text_recipe_checked(tmp_path):
    assert_fail(tmp_path, "先执行 just build-release 再打包", "just build-release")


def test_justfile_variable_not_a_recipe(tmp_path):
    # 配方名不含下划线：`just coverage_min` 被提取为 coverage，按不存在的配方拒绝
    assert_fail(tmp_path, "阈值见 `just coverage_min`", "「just coverage」")


def test_missing_path(tmp_path):
    assert_fail(tmp_path, "实现见 `crates/demo/src/store.rs`", "在仓库里不存在")


def test_missing_bare_filename(tmp_path):
    assert_fail(tmp_path, "配置见 `nonexistent.toml`", "在仓库根不存在")


def test_missing_test_target(tmp_path):
    assert_fail(tmp_path, "跑 `cargo test --test smoke`", "找不到 tests/smoke.rs")


def test_wrong_directory_level(tmp_path):
    body = "见 `crates/demo/tests/overlay_selftest.rs`"
    assert_fail(tmp_path, body, "在仓库里不存在")


def test_bad_recipe_in_code_block(tmp_path):
    body = "命令清单：\n\n    just run\n    just deploy"
    assert_fail(tmp_path, body, "「just deploy」")


# ---- 质量条目名指名引用（仓库各处的 质量条目「名字」）----


def test_named_rule_reference_hits_real_entry(tmp_path):
    ref = "//! 见 AGENTS.md 质量条目「日志统一出口」。"
    assert_pass(tmp_path, RULE_DOC, ref)


def test_named_rule_reference_unknown_entry(tmp_path):
    ref = "//! 见 AGENTS.md 质量条目「日志英文」。"
    assert_fail(tmp_path, RULE_DOC, "日志英文", ref)


def test_named_rule_reference_stale_after_rename(tmp_path):
    ref = "//! 见 AGENTS.md 质量条目「日志统一出口」。"
    assert_fail(tmp_path, RULE_DOC_RENAMED, "日志统一出口", ref)


def test_no_named_reference_no_false_positive(tmp_path):
    assert_pass(tmp_path, RULE_DOC)
