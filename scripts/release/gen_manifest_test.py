# 发布清单生成脚本的单测：正常生成、缺产物拒绝、版本单点复用；体积与摘要
# 哈希（sha256）实测自产物文件。
#
# 被测入口吃标签、产物目录与仓库根三个参数，成功返回清单字典，失败以系统
# 退出异常终止；夹具在（pytest）临时目录里放已知内容的产物文件。运行方式
# 沉记录文件。

import hashlib
import json
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).parent))

import gen_manifest  # noqa: E402

SCRIPT_DIR = Path(__file__).parent

MANIFEST_OK = '[workspace.package]\nversion = "0.1.0"\nedition = "2024"'
ARTIFACT_PAYLOADS = {
    "gloss-0.1.0-aarch64-apple-darwin.zip": "arm64-zip-payload",
    "gloss-0.1.0-aarch64-apple-darwin.dmg": "arm64-dmg-payload",
    "gloss-0.1.0-x86_64-apple-darwin.zip": "x64-zip-payload",
    "gloss-0.1.0-x86_64-apple-darwin.dmg": "x64-dmg-payload",
}


def build_fixture(tmp_path, manifest=MANIFEST_OK, drop=None):
    (tmp_path / "Cargo.toml").write_text(manifest + "\n", encoding="utf-8")
    artifacts = tmp_path / "artifacts"
    artifacts.mkdir()
    for name, payload in ARTIFACT_PAYLOADS.items():
        if name != drop:
            (artifacts / name).write_text(payload, encoding="utf-8")
    return artifacts


def run_ok(tmp_path, tag="v0.1.0", drop=None):
    artifacts = build_fixture(tmp_path, drop=drop)
    return gen_manifest.run(tag, str(artifacts), str(tmp_path))


def test_manifest_generated(tmp_path):
    manifest = run_ok(tmp_path)
    assert manifest["schema"] == 1
    assert manifest["version"] == "0.1.0"
    assert set(manifest["channels"]["stable"]) == {"aarch64-apple-darwin", "x86_64-apple-darwin"}


def test_zip_url(tmp_path):
    manifest = run_ok(tmp_path)
    entry = manifest["channels"]["stable"]["aarch64-apple-darwin"]
    assert entry["url"] == (
        "https://losmli010.github.io/gloss/latest/gloss-0.1.0-aarch64-apple-darwin.zip"
    )


def test_dmg_url(tmp_path):
    manifest = run_ok(tmp_path)
    entry = manifest["channels"]["stable"]["x86_64-apple-darwin"]
    assert entry["dmg_url"] == (
        "https://losmli010.github.io/gloss/latest/gloss-0.1.0-x86_64-apple-darwin.dmg"
    )


def test_size_and_sha256_measured_from_artifacts(tmp_path):
    artifacts = build_fixture(tmp_path)
    zip_path = artifacts / "gloss-0.1.0-aarch64-apple-darwin.zip"
    expected_size = zip_path.stat().st_size
    expected_sha = hashlib.sha256(zip_path.read_bytes()).hexdigest()
    manifest = gen_manifest.run("v0.1.0", str(artifacts), str(tmp_path))
    entry = manifest["channels"]["stable"]["aarch64-apple-darwin"]
    assert entry["size"] == expected_size
    assert entry["sha256"] == expected_sha


def run_cli(tmp_path, artifacts, *args):
    """把被测脚本与版本单点校验脚本拷进夹具再跑 CLI（脚本按自身位置定位仓库根），
    返回 (returncode, stdout, stderr)。"""
    cli_dir = tmp_path / "scripts" / "release"
    cli_dir.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(SCRIPT_DIR / "gen_manifest.py", cli_dir / "gen_manifest.py")
    shutil.copyfile(SCRIPT_DIR / "check_release_tag.py", cli_dir / "check_release_tag.py")
    proc = subprocess.run(
        [sys.executable, str(cli_dir / "gen_manifest.py"), *args],
        cwd=str(tmp_path),
        capture_output=True,
        text=True,
    )
    return proc.returncode, proc.stdout, proc.stderr


def test_json_output_is_valid(tmp_path):
    build_fixture(tmp_path)
    rc, out, err = run_cli(tmp_path, tmp_path / "artifacts", "v0.1.0", "artifacts")
    assert rc == 0, err
    assert json.loads(out)["version"] == "0.1.0"


def test_missing_zip_rejected(tmp_path):
    with pytest.raises(SystemExit, match="缺产物文件"):
        run_ok(tmp_path, drop="gloss-0.1.0-aarch64-apple-darwin.zip")


def test_missing_dmg_rejected(tmp_path):
    with pytest.raises(SystemExit, match="缺产物文件"):
        run_ok(tmp_path, drop="gloss-0.1.0-x86_64-apple-darwin.dmg")


def test_version_mismatch_rejected(tmp_path):
    with pytest.raises(SystemExit, match="版本号不一致"):
        run_ok(tmp_path, tag="v0.2.0")


def test_missing_v_prefix_rejected(tmp_path):
    with pytest.raises(SystemExit, match="缺 v 前缀"):
        run_ok(tmp_path, tag="0.1.0")


def test_missing_arguments_usage_exit_2(tmp_path):
    build_fixture(tmp_path)
    rc, _, err = run_cli(tmp_path, tmp_path / "artifacts")
    assert rc == 2
    assert "用法" in err
