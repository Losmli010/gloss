"""约束门禁·源码卫生域的单测：桩副本一致、文件行数上限与残留任务标记逐条
覆盖；任务标记用例在一次性版本库夹具上驱动，非版本库夹具用例覆盖整段跳过
路径。
"""

import shutil
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from constraints import fixtures, source_hygiene  # noqa: E402

CHECKER = Path(__file__).parent / "source_hygiene.py"


def test_engine_stub_drift(tmp_path):
    def mutate(root):
        with open(root / "crates/gloss-app/tests/stubs/engine.rs", "a") as fh:
            fh.write("\n// drift\n")

    fixtures.assert_fail(source_hygiene, tmp_path, mutate, "stub copy drift")


def test_ports_stub_section_drift(tmp_path):
    def mutate(root):
        fixtures.replace_line(
            root / "crates/gloss-app/tests/stubs/ports.rs",
            "pub struct MemoryConfigStore;",
            "pub struct MemoryConfigStore drift;",
        )

    fixtures.assert_fail(source_hygiene, tmp_path, mutate, "stub copy drift")


def test_overlong_rs_file_rejected(tmp_path):
    def mutate(root):
        (root / "crates/gloss-app/src").mkdir(exist_ok=True)
        (root / "crates/gloss-app/src/big.rs").write_text("let _x = 1;\n" * 520)

    fixtures.assert_fail(source_hygiene, tmp_path, mutate, "line cap")


def test_lines_allow_in_first_five_lines_passes(tmp_path):
    def mutate(root):
        (root / "crates/gloss-app/src").mkdir(exist_ok=True)
        (root / "crates/gloss-app/src/big.rs").write_text(
            "// lines:allow 自测阴性对照\n" + "let _x = 1;\n" * 520
        )

    fixtures.assert_pass(source_hygiene, tmp_path, mutate, "1 lines:allow exemptions")


def test_overlong_test_file_not_punished(tmp_path):
    def mutate(root):
        (root / "crates/gloss-app/tests").mkdir(exist_ok=True)
        (root / "crates/gloss-app/tests/big.rs").write_text("let _x = 1;\n" * 520)

    fixtures.assert_pass(source_hygiene, tmp_path, mutate, "0 lines:allow exemptions")


def test_inline_tests_excluded_from_production_lines(tmp_path):
    def mutate(root):
        (root / "crates/gloss-app/src").mkdir(exist_ok=True)
        (root / "crates/gloss-app/src/mixed.rs").write_text(
            "let _x = 1;\n" * 20 + "#[cfg(test)]\nmod tests {\n" + "let _y = 1;\n" * 520 + "}\n"
        )

    fixtures.assert_pass(source_hygiene, tmp_path, mutate, "0 lines:allow exemptions")


def test_lines_allow_past_header_rejected(tmp_path):
    def mutate(root):
        (root / "crates/gloss-app/src").mkdir(exist_ok=True)
        (root / "crates/gloss-app/src/late_allow.rs").write_text(
            "let _x = 1;\n" * 5
            + "// lines:allow 写在文件头 5 行之外不算豁免\n"
            + "let _x = 1;\n" * 520
        )

    fixtures.assert_fail(source_hygiene, tmp_path, mutate, "line cap")


def test_indented_cfg_test_not_truncated(tmp_path):
    def mutate(root):
        (root / "crates/gloss-app/src").mkdir(exist_ok=True)
        (root / "crates/gloss-app/src/indented_cfg.rs").write_text(
            "let _x = 1;\n" * 19 + "    #[cfg(test)]\n" + "let _y = 1;\n" * 520
        )

    fixtures.assert_fail(source_hygiene, tmp_path, mutate, "line cap")


# ---- 残留任务标记扫描（版本库夹具；上方非版本库夹具用例覆盖自动跳过路径）----
# 约束脚本自身豁免：模块副本（含各类任务标记字面量）被版本库跟踪，
# “干净版本库通过”用例正常退出即证明其未被自命中。


def test_clean_git_repo_passes(tmp_path):
    fixtures.new_fixture(tmp_path)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    (tmp_path / "scripts/hooks/constraints").mkdir(parents=True)
    shutil.copyfile(CHECKER, tmp_path / "scripts/hooks/constraints/source_hygiene.py")
    (tmp_path / "src").mkdir()
    (tmp_path / "src/clean.rs").write_text("let x = compute(input);\n")
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    oks, fails = source_hygiene.run(str(tmp_path))
    assert fails == []


def test_todo_marker_hit(tmp_path):
    fixtures.new_fixture(tmp_path)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    (tmp_path / "scripts/hooks/constraints").mkdir(parents=True)
    shutil.copyfile(CHECKER, tmp_path / "scripts/hooks/constraints/source_hygiene.py")
    (tmp_path / "src").mkdir()
    (tmp_path / "src/has_todo.rs").write_text("// TODO: 待办\n")
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    _, fails = source_hygiene.run(str(tmp_path))
    assert any("leftover task marker:" in f for f in fails)


def test_fixme_marker_hit(tmp_path):
    fixtures.new_fixture(tmp_path)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    (tmp_path / "scripts/hooks/constraints").mkdir(parents=True)
    shutil.copyfile(CHECKER, tmp_path / "scripts/hooks/constraints/source_hygiene.py")
    (tmp_path / "src").mkdir()
    (tmp_path / "src/has_fixme.rs").write_text("fixme_marker(); // FIXME\n")
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    _, fails = source_hygiene.run(str(tmp_path))
    assert any("leftover task marker:" in f for f in fails)


def test_todo_adjacent_to_cjk_hit(tmp_path):
    # 正则按英文词界（ASCII）语义对齐全词匹配：标记紧贴中文（汉字不算词字符）同样命中
    fixtures.new_fixture(tmp_path)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    (tmp_path / "scripts/hooks/constraints").mkdir(parents=True)
    shutil.copyfile(CHECKER, tmp_path / "scripts/hooks/constraints/source_hygiene.py")
    (tmp_path / "src").mkdir()
    (tmp_path / "src/cjk_todo.rs").write_text("// TODO记录：待办\n")
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    _, fails = source_hygiene.run(str(tmp_path))
    assert any("leftover task marker:" in f for f in fails)


def test_agents_md_uppercase_todo_exempt(tmp_path):
    fixtures.new_fixture(tmp_path)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    (tmp_path / "scripts/hooks/constraints").mkdir(parents=True)
    shutil.copyfile(CHECKER, tmp_path / "scripts/hooks/constraints/source_hygiene.py")
    (tmp_path / "AGENTS.md").write_text(
        "gloss、gloss-core、gloss-platform、gloss-app 的依赖方向见下。代码不留 TODO 残留标记。\n",
        encoding="utf-8",
    )
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    _, fails = source_hygiene.run(str(tmp_path))
    assert fails == []


def test_lowercase_and_word_boundary_no_false_positive(tmp_path):
    fixtures.new_fixture(tmp_path)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    (tmp_path / "scripts/hooks/constraints").mkdir(parents=True)
    shutil.copyfile(CHECKER, tmp_path / "scripts/hooks/constraints/source_hygiene.py")
    (tmp_path / "src").mkdir()
    (tmp_path / "src/boundary.rs").write_text('let todo = "TODOS"; let _x = MY_TODO;\n')
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    _, fails = source_hygiene.run(str(tmp_path))
    assert fails == []
