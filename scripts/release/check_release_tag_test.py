"""发行标签校验脚本的单测：版本一致、标签格式与工作区版本段的定位。

被测入口吃标签与仓库根两个参数，返回标签版本、清单版本与错误串三元组；
夹具在（pytest）临时目录里放定制的工作区清单文件。运行方式沉记录文件。
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import check_release_tag  # noqa: E402

MANIFEST_OK = (
    '[package]\nname = "gloss"\nversion.workspace = true\n'
    '\n[workspace.package]\nversion = "0.1.0"\nedition = "2024"'
)

# 根包继承行与依赖段的版本行都不得被当成单点版本：
# 单点行故意放在工作区段，值与继承行、依赖行都不同。
MANIFEST_TRAPS = (
    '[package]\nname = "gloss"\nversion.workspace = true\n'
    "\n[dependencies]\n"
    'serde = { version = "9.9.9", default-features = false }\n'
    "\n[workspace.package]\n" + 'version = "1.2.3"'
)


def run(tag, manifest, tmp_path):
    (tmp_path / "Cargo.toml").write_text(manifest + "\n", encoding="utf-8")
    return check_release_tag.run(tag, str(tmp_path))


def assert_ok(tag, manifest, tmp_path):
    version, cargo_version, error = run(tag, manifest, tmp_path)
    assert error is None
    assert version == tag[1:]


def assert_reject(tag, manifest, tmp_path, needle):
    _, _, error = run(tag, manifest, tmp_path)
    assert error is not None and needle in error


def test_tag_matches_version(tmp_path):
    assert_ok("v0.1.0", MANIFEST_OK, tmp_path)


def test_multi_digit_version(tmp_path):
    manifest = MANIFEST_OK.replace("0.1.0", "10.20.30")
    assert_ok("v10.20.30", manifest, tmp_path)


def test_tag_mismatch_rejected(tmp_path):
    assert_reject("v0.2.0", MANIFEST_OK, tmp_path, "version mismatch")


def test_missing_v_prefix(tmp_path):
    assert_reject("0.1.0", MANIFEST_OK, tmp_path, "lacks the v prefix")


def test_two_segment_version(tmp_path):
    assert_reject("v0.1", MANIFEST_OK, tmp_path, "does not match")


def test_four_segment_version(tmp_path):
    assert_reject("v0.1.0.1", MANIFEST_OK, tmp_path, "does not match")


def test_inherited_and_dep_versions_not_misread(tmp_path):
    assert_ok("v1.2.3", MANIFEST_TRAPS, tmp_path)


def test_workspace_package_missing_version(tmp_path):
    assert_reject(
        "v0.1.0", '[workspace.package]\nedition = "2024"', tmp_path, "failed to read the version"
    )


def test_missing_argument_usage_exit_2():
    import subprocess

    proc = subprocess.run(
        [sys.executable, str(Path(__file__).parent / "check_release_tag.py")],
        capture_output=True,
        text=True,
    )
    assert proc.returncode == 2
    assert "usage" in proc.stderr
