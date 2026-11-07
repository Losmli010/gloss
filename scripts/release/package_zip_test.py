"""打包脚本的单测：载荷清单只收可执行文件与放行名单内的旁置动态库（dll）。

名单外与放行而缺位的动态库都按系统退出异常终止不打包；夹具在（pytest）
临时目录里摆可执行文件与动态库。运行方式沉记录文件。
"""

from __future__ import annotations

import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).parent))

import package_zip  # noqa: E402


def test_payload_is_the_executable_alone_without_dlls(tmp_path: Path) -> None:
    exe = tmp_path / "gloss.exe"
    exe.write_bytes(b"exe")
    assert package_zip.payload_files(str(exe), []) == [str(exe)]


def test_allowed_dlls_join_the_payload_and_stray_docs_stay_out(tmp_path: Path) -> None:
    (tmp_path / "gloss.exe").write_bytes(b"exe")
    (tmp_path / "helper.dll").write_bytes(b"dll")
    (tmp_path / "readme.txt").write_bytes(b"stray doc")
    files = package_zip.payload_files(str(tmp_path / "gloss.exe"), ["helper.dll"])
    assert [Path(path).name for path in files] == ["gloss.exe", "helper.dll"]


def test_dll_allowlist_matches_case_insensitively(tmp_path: Path) -> None:
    (tmp_path / "gloss.exe").write_bytes(b"exe")
    (tmp_path / "Helper.DLL").write_bytes(b"dll")
    files = package_zip.payload_files(str(tmp_path / "gloss.exe"), ["helper.dll"])
    assert [Path(path).name for path in files] == ["gloss.exe", "Helper.DLL"]


def test_stray_dll_is_rejected_without_packaging(tmp_path: Path) -> None:
    (tmp_path / "gloss.exe").write_bytes(b"exe")
    (tmp_path / "legacy_helper.dll").write_bytes(b"stray")
    with pytest.raises(SystemExit, match="legacy_helper.dll"):
        package_zip.payload_files(str(tmp_path / "gloss.exe"), [])


def test_stray_dll_is_rejected_even_when_others_are_allowed(tmp_path: Path) -> None:
    (tmp_path / "gloss.exe").write_bytes(b"exe")
    (tmp_path / "helper.dll").write_bytes(b"dll")
    (tmp_path / "legacy_helper.dll").write_bytes(b"stray")
    with pytest.raises(SystemExit, match="legacy_helper.dll"):
        package_zip.payload_files(str(tmp_path / "gloss.exe"), ["helper.dll"])


def test_allowed_but_missing_dll_is_rejected(tmp_path: Path) -> None:
    (tmp_path / "gloss.exe").write_bytes(b"exe")
    with pytest.raises(SystemExit, match="helper.dll"):
        package_zip.payload_files(str(tmp_path / "gloss.exe"), ["helper.dll"])
