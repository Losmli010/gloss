"""记录命名映射助手的单测：正反向映射、家族前缀表、体检判罚与建档。

被测入口是映射函数与命令行入口；夹具建在测试框架（pytest）的临时目录（一
次性版本库，跟踪清单走版本库清单）；用法见持续集成的钩子工作流。"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import comment_map  # noqa: E402

# ---- 纯函数 ----


def test_base_stem_strips_extension_and_prefixes_mod() -> None:
    assert comment_map.base_stem("src/main.rs") == "main"
    assert comment_map.base_stem("src/app/mod.rs") == "app_mod"
    assert comment_map.base_stem("tests/stubs/mod.rs") == "stubs_mod"


def test_record_names_is_a_decision_and_tests_pair() -> None:
    assert comment_map.record_names("src/cache.rs") == (
        "cache_comment.md",
        "cache_tests_comment.md",
    )


def test_family_stems_override_the_default_stem() -> None:
    assert comment_map.record_stem("web/assets/main.js") == "web_main"
    assert comment_map.record_stem("src/main.rs") == "main"
    assert comment_map.record_stem("crates/gloss-eval/src/main.rs") == "eval_main"


def test_bare_name_legality() -> None:
    assert comment_map.is_legal_bare_name("main")
    assert comment_map.is_legal_bare_name("clone_rs")
    assert not comment_map.is_legal_bare_name("main.rs")
    assert not comment_map.is_legal_bare_name("")


def test_residue_stem_needs_same_extension_source() -> None:
    index = comment_map.build_index(["benches/clone.rs", "scripts/perf/metrics.py"])
    assert comment_map.residue_stem("clone_rs", index) == "clone"
    assert comment_map.residue_stem("metrics_py", index) == "metrics"
    # 主干真以下划线扩展名形态结尾且无同名同扩展名源文件：合法名字，不是残留
    assert comment_map.residue_stem("foo_rs", comment_map.build_index(["src/foo_rs.py"])) is None
    # 剥掉后解析到不同扩展名：不算残留
    assert comment_map.residue_stem("clone_py", index) is None


def test_carries_records_excludes_prompts_and_records() -> None:
    assert comment_map.carries_records("src/lib.rs")
    assert comment_map.carries_records("justfile")
    assert comment_map.carries_records(".githooks/commit-msg")
    assert comment_map.carries_records(".zcode/hooks/check_pr_title.py")
    assert not comment_map.carries_records("crates/gloss-core/prompts/en/code.md")
    assert not comment_map.carries_records("docs/comments/main_comment.md")
    assert not comment_map.carries_records("assets/icons/logo.svg")


def test_build_index_shares_one_stem_family() -> None:
    tracked = [
        "src/main.rs",
        "crates/a/tests/stubs/mod.rs",
        "crates/b/tests/stubs/mod.rs",
    ]
    index = comment_map.build_index(tracked)
    assert index["main"] == ["src/main.rs"]
    assert index["stubs_mod_tests"] == [
        "crates/a/tests/stubs/mod.rs",
        "crates/b/tests/stubs/mod.rs",
    ]


def test_unregistered_collisions_flag_and_registration_silences() -> None:
    tracked = ["src/style.rs", "web/style.css"]
    assert comment_map.unregistered_collisions(tracked) == [
        ("style", ["src/style.rs", "web/style.css"])
    ]
    tracked = ["src/style.rs", "web/assets/style.css"]
    assert comment_map.unregistered_collisions(tracked) == []


def test_partially_registered_collision_flagged() -> None:
    """撞名组只登记部分成员：未登记成员的规范名与已登记成员相同，整组判罚。"""
    tracked = ["src/main.rs", "tools/main.py"]
    assert comment_map.unregistered_collisions(tracked) == [
        ("main", ["src/main.rs", "tools/main.py"])
    ]
    tracked = ["src/main.rs", "crates/gloss-eval/src/main.rs", "web/assets/main.js"]
    assert comment_map.unregistered_collisions(tracked) == []


# ---- 命令行与体检（临时版本库端到端）----

SCRIPT = str(Path(__file__).with_name("comment_map.py"))


def build_fixture(tmp_path: Path) -> None:
    (tmp_path / "src").mkdir()
    (tmp_path / "docs/comments").mkdir(parents=True)
    (tmp_path / "src/lib.rs").write_text("fn main() {}\n", encoding="utf-8")
    (tmp_path / "docs/comments/lib_comment.md").write_text("# lib.rs\n", encoding="utf-8")
    (tmp_path / "docs/comments/ghost_comment.md").write_text("# 阵亡\n", encoding="utf-8")
    (tmp_path / "docs/comments/old.rs_comment.md").write_text("# 残留\n", encoding="utf-8")
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)


def run_cli(*args: str, cwd: Path | None = None) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(
        [sys.executable, SCRIPT, *args], capture_output=True, cwd=str(cwd) if cwd else None
    )


def test_doctor_flags_illegal_and_stale(tmp_path: Path) -> None:
    build_fixture(tmp_path)
    proc = run_cli("--doctor", cwd=tmp_path)
    assert proc.returncode == comment_map.EXIT_FINDINGS
    out = proc.stdout.decode("utf-8")
    assert "illegal record name: docs/comments/old.rs_comment.md" in out
    assert "stale record (no source file): docs/comments/ghost_comment.md" in out
    assert "docs/comments/lib_comment.md" not in out.split("collision:")[0].split("illegal")[0]


def test_doctor_fix_deletes_stale_only(tmp_path: Path) -> None:
    build_fixture(tmp_path)
    proc = run_cli("--doctor", "--fix", cwd=tmp_path)
    assert proc.returncode == comment_map.EXIT_FINDINGS
    assert not (tmp_path / "docs/comments/ghost_comment.md").exists()
    assert (tmp_path / "docs/comments/old.rs_comment.md").exists()
    assert (tmp_path / "docs/comments/lib_comment.md").exists()


def test_doctor_clean_after_fix_passes(tmp_path: Path) -> None:
    build_fixture(tmp_path)
    run_cli("--doctor", "--fix", cwd=tmp_path)
    (tmp_path / "docs/comments/old.rs_comment.md").unlink()
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    proc = run_cli("--doctor", cwd=tmp_path)
    assert proc.returncode == comment_map.EXIT_PASS
    assert "record naming clean" in proc.stdout.decode("utf-8")


def test_name_and_reverse_round_trip(tmp_path: Path) -> None:
    build_fixture(tmp_path)
    proc = run_cli("src/lib.rs", cwd=tmp_path)
    assert proc.returncode == comment_map.EXIT_PASS
    assert "docs/comments/lib_comment.md" in proc.stdout.decode("utf-8")
    proc = run_cli("--reverse", "lib_comment.md", cwd=tmp_path)
    assert "src/lib.rs" in proc.stdout.decode("utf-8")


def test_ensure_creates_missing_record_with_preamble(tmp_path: Path) -> None:
    build_fixture(tmp_path)
    (tmp_path / "src/extra.rs").write_text("fn extra() {}\n", encoding="utf-8")
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    proc = run_cli("--ensure", "src/extra.rs", cwd=tmp_path)
    assert proc.returncode == comment_map.EXIT_PASS
    created = tmp_path / "docs/comments/extra_comment.md"
    assert created.exists()
    assert "随仓库跟踪" in created.read_text(encoding="utf-8")
    proc = run_cli("--ensure", "src/extra.rs", cwd=tmp_path)
    assert "created" not in proc.stdout.decode("utf-8")


def test_untracked_source_is_rejected(tmp_path: Path) -> None:
    build_fixture(tmp_path)
    proc = run_cli("src/absent.rs", cwd=tmp_path)
    assert proc.returncode == comment_map.EXIT_FINDINGS


def test_usage_errors(tmp_path: Path) -> None:
    build_fixture(tmp_path)
    assert run_cli(cwd=tmp_path).returncode == comment_map.EXIT_USAGE
    assert run_cli("--reverse", cwd=tmp_path).returncode == comment_map.EXIT_USAGE
    assert run_cli("--doctor", "extra", cwd=tmp_path).returncode == comment_map.EXIT_USAGE
