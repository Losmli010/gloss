"""check_release_tag.py 的单测：版本一致、tag 格式、[workspace.package] 段定位。

被测函数是 check_release_tag.run(tag, root)（返回 (tag 版本, cargo 版本, 错误串)），
夹具在 pytest 的 tmp_path 里放定制的 Cargo.toml。用法：
    python3 -m pytest scripts/release/check_release_tag_test.py
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
    assert_reject("v0.2.0", MANIFEST_OK, tmp_path, "版本号不一致")


def test_missing_v_prefix(tmp_path):
    assert_reject("0.1.0", MANIFEST_OK, tmp_path, "缺 v 前缀")


def test_two_segment_version(tmp_path):
    assert_reject("v0.1", MANIFEST_OK, tmp_path, "不符合")


def test_four_segment_version(tmp_path):
    assert_reject("v0.1.0.1", MANIFEST_OK, tmp_path, "不符合")


def test_inherited_and_dep_versions_not_misread(tmp_path):
    assert_ok("v1.2.3", MANIFEST_TRAPS, tmp_path)


def test_workspace_package_missing_version(tmp_path):
    assert_reject("v0.1.0", '[workspace.package]\nedition = "2024"', tmp_path, "没能从 Cargo.toml")


def test_missing_argument_usage_exit_2():
    import subprocess

    proc = subprocess.run(
        [sys.executable, str(Path(__file__).parent / "check_release_tag.py")],
        capture_output=True,
        text=True,
    )
    assert proc.returncode == 2
    assert "用法" in proc.stderr
