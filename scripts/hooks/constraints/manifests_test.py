"""约束门禁·共享层内容缓存的单测：缓存语义（同一文件一次扫描只打开一次、
清单枚举与跟踪清单共享同一结果、字节与解码文本分层且容错口径分开）与读取
口径（文本模式读取语义、缺文件照常报错）逐条覆盖——读不到即报违规的路径
不因缓存变形。
"""

from __future__ import annotations

import builtins
import os
import subprocess
import sys
from pathlib import Path
from typing import Any

import pytest

sys.path.insert(0, str(Path(__file__).parent.parent))

from constraints import manifests  # noqa: E402


def make_cache_with_repo(tmp_path: Path, files: dict[str, bytes]) -> manifests.ContentCache:
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    for name, data in files.items():
        target = tmp_path / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    return manifests.ContentCache(str(tmp_path))


def test_read_text_matches_text_mode_semantics(tmp_path: Path) -> None:
    (tmp_path / "crlf.txt").write_bytes(b"a\r\nb\rc\n")
    cache = manifests.ContentCache(str(tmp_path))
    assert cache.read_text(str(tmp_path / "crlf.txt"), errors="replace") == "a\nb\nc\n"


def test_read_text_decodes_by_requested_errors_mode(tmp_path: Path) -> None:
    (tmp_path / "bad.txt").write_bytes(b"a\x80b\n")
    cache = manifests.ContentCache(str(tmp_path))
    with pytest.raises(UnicodeDecodeError):
        cache.read_text(str(tmp_path / "bad.txt"))
    assert cache.read_text(str(tmp_path / "bad.txt"), errors="replace") == "a\ufffdb\n"


def test_read_text_cached_by_path_and_errors_mode(tmp_path: Path) -> None:
    (tmp_path / "ok.txt").write_bytes(b"a\n")
    cache = manifests.ContentCache(str(tmp_path))
    path = str(tmp_path / "ok.txt")
    assert cache.read_text(path, errors="replace") is cache.read_text(path, errors="replace")
    assert cache.read_text(path) is cache.read_text(path)
    assert cache.read_text(path) is not cache.read_text(path, errors="replace")


def test_read_text_opens_target_file_once(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    target = tmp_path / "a.txt"
    target.write_bytes(b"line\n")
    cache = manifests.ContentCache(str(tmp_path))
    real_open = builtins.open
    opens = []

    def counting_open(file: Any, *args: Any, **kwargs: Any) -> Any:
        if os.fspath(file) == str(target):
            opens.append(file)
        return real_open(file, *args, **kwargs)

    monkeypatch.setattr(builtins, "open", counting_open)
    first = cache.read_text(str(target), errors="replace")
    second = cache.read_text(str(target), errors="replace")
    assert first == second == "line\n"
    assert opens == [str(target)]


def test_read_bytes_cached_and_matches_file(tmp_path: Path) -> None:
    (tmp_path / "x.bin").write_bytes(b"ab\r\x00")
    cache = manifests.ContentCache(str(tmp_path))
    path = str(tmp_path / "x.bin")
    assert cache.read_bytes(path) == b"ab\r\x00"
    assert cache.read_bytes(path) is cache.read_bytes(path)


def test_missing_file_raises_os_error(tmp_path: Path) -> None:
    (tmp_path / "here.txt").write_bytes(b"x")
    cache = manifests.ContentCache(str(tmp_path))
    with pytest.raises(OSError):
        cache.read_bytes(str(tmp_path / "gone.txt"))
    with pytest.raises(OSError):
        cache.read_text(str(tmp_path / "gone.txt"), errors="replace")


def test_manifests_enumeration_shared_within_scan(tmp_path: Path) -> None:
    (tmp_path / "crates/gloss-core").mkdir(parents=True)
    (tmp_path / "Cargo.toml").write_text("", encoding="utf-8")
    (tmp_path / "crates/gloss-core/Cargo.toml").write_text("", encoding="utf-8")
    cache = manifests.ContentCache(str(tmp_path))
    first = cache.manifests(str(tmp_path))
    assert len(first) == 2
    assert manifests.manifests(str(tmp_path), cache) is first
    assert cache.manifests(str(tmp_path)) is first


def test_tracked_enumerated_once_and_shared(tmp_path: Path) -> None:
    cache = make_cache_with_repo(tmp_path, {"b.txt": b"1\n", "a/c.rs": b"2\n"})
    assert cache.tracked() == ["a/c.rs", "b.txt"]
    assert cache.tracked() is cache.tracked()
