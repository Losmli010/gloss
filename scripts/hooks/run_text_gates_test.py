"""纯文本门禁并轨入口的单测：覆盖共享内容装入器的清单枚举、两层惰性缓存
与解码口径（容错模式分开缓存、通用换行翻译、缺文件报错），以及门禁清单的
注册顺序与前置失败不中断后续门禁的编排行为。在测试框架（pytest）的临时目
录里建一次性版本库作扫描面；并轨聚合输出与各门禁单跑输出的等价比对靠种子
违规样本端到端双跑，不入单测。
"""

import os
import subprocess
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).parent))

import run_text_gates  # noqa: E402


def make_repo(tmp_path, files):
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    for name, data in files.items():
        p = tmp_path / name
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(data)
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    return run_text_gates.ContentLoader(str(tmp_path))


def test_tracked_enumerated_once_and_shared(tmp_path):
    src = make_repo(tmp_path, {"b.txt": b"1\n", "a/c.rs": b"2\n"})
    assert src.tracked() == ["a/c.rs", "b.txt"]
    assert src.tracked() is src.tracked()


def test_read_bytes_cached_across_calls(tmp_path):
    src = make_repo(tmp_path, {"x.bin": b"ab\r\x00"})
    assert src.read_bytes("x.bin") == b"ab\r\x00"
    assert src.read_bytes("x.bin") is src.read_bytes("x.bin")


def test_read_text_matches_text_mode_semantics(tmp_path):
    src = make_repo(tmp_path, {"crlf.txt": b"a\r\nb\rc\n", "plain.txt": b"a\nb"})
    assert src.read_text("crlf.txt") == "a\nb\nc\n"
    assert src.read_text("plain.txt") == "a\nb"


def test_read_text_decodes_by_requested_errors_mode(tmp_path):
    src = make_repo(tmp_path, {"bad.txt": b"a\x80b\n"})
    with pytest.raises(UnicodeDecodeError):
        src.read_text("bad.txt")
    assert src.read_text("bad.txt", errors="replace") == "a\ufffdb\n"


def test_read_text_cached_per_errors_mode(tmp_path):
    src = make_repo(tmp_path, {"ok.txt": b"a\n"})
    assert src.read_text("ok.txt") is src.read_text("ok.txt")
    assert src.read_text("ok.txt") is not src.read_text("ok.txt", errors="replace")


def test_missing_file_raises_os_error(tmp_path):
    src = make_repo(tmp_path, {"here.txt": b"x"})
    with pytest.raises(OSError):
        src.read_bytes("gone.txt")
    with pytest.raises(OSError):
        src.read_text("gone.txt")


def test_relative_and_absolute_paths_share_byte_cache(tmp_path):
    src = make_repo(tmp_path, {"x.txt": b"data"})
    absolute = os.path.join(str(tmp_path), "x.txt")
    assert src.read_bytes(absolute) is src.read_bytes("x.txt")


def test_gate_registry_order():
    assert [name for name, _module in run_text_gates.GATES] == [
        "secrets",
        "agents-doc",
        "test-bdd",
        "constraints",
        "i18n",
        "comments",
    ]


class _Stub:
    def __init__(self, behavior):
        self.behavior = behavior

    def main(self, loader=None):
        return self.behavior()


def test_run_gate_returns_module_exit_code():
    assert run_text_gates.run_gate(_Stub(lambda: 0), None) == 0
    assert run_text_gates.run_gate(_Stub(lambda: 1), None) == 1


def test_run_gate_converts_preflight_system_exit(capsys):
    def boom():
        raise SystemExit("error: inventory missing")

    assert run_text_gates.run_gate(_Stub(boom), None) == 1
    assert "error: inventory missing" in capsys.readouterr().err
