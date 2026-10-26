"""共享检索引擎的单测：空清单短路、引擎缺失与异常流的失败关闭、事件流解
析与字节字段解码、多行模式的起始行号。

被测入口是引擎的运行函数；子进程交互用假完成态替换，不起真实引擎即可锁
失败关闭路径；多行模式另起真实引擎验起始行号与跨行行文；用法见约束门禁
各域与密钥门禁的单测入口。
"""

from __future__ import annotations

import base64
import json
import subprocess
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).parent))

import rg_engine  # noqa: E402


def fake_completed(returncode: int, stdout: bytes = b"", stderr: bytes = b""):
    return subprocess.CompletedProcess([], returncode, stdout, stderr)


MARKER = "TO" + "DO"  # 夹具字面量片段拼接，任务标记门禁不自命中


def match_event(rel: str, line_no: int, text: str) -> str:
    event = {
        "type": "match",
        "data": {
            "path": {"text": rel},
            "lines": {"text": text},
            "line_number": line_no,
        },
    }
    return json.dumps(event) + "\n"


def test_empty_inputs_short_circuit_without_subprocess(monkeypatch: pytest.MonkeyPatch) -> None:
    calls: list[int] = []

    def fake_run(*args: object, **kwargs: object) -> None:
        calls.append(1)

    monkeypatch.setattr(rg_engine.subprocess, "run", fake_run)
    assert rg_engine.run("root", [], ["a.txt"]) == {}
    assert rg_engine.run("root", ["x"], []) == {}
    assert calls == []


def test_missing_engine_raises_runtime_error(monkeypatch: pytest.MonkeyPatch) -> None:
    def raise_missing(*args: object, **kwargs: object) -> None:
        raise FileNotFoundError("rg")

    monkeypatch.setattr(rg_engine.subprocess, "run", raise_missing)
    with pytest.raises(RuntimeError, match="ripgrep not found"):
        rg_engine.run("root", ["x"], ["a.txt"])


def test_engine_exit_two_raises_runtime_error(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(
        rg_engine.subprocess, "run", lambda *a, **k: fake_completed(2, stderr=b"regex error")
    )
    with pytest.raises(RuntimeError, match="exit 2"):
        rg_engine.run("root", ["x"], ["a.txt"])


def test_unparsable_stream_raises_runtime_error(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(
        rg_engine.subprocess, "run", lambda *a, **k: fake_completed(0, stdout=b"not-json\n")
    )
    with pytest.raises(RuntimeError, match="unparsable"):
        rg_engine.run("root", ["x"], ["a.txt"])


def test_non_match_events_ignored(monkeypatch: pytest.MonkeyPatch) -> None:
    stream = (
        json.dumps({"type": "begin", "data": {"path": {"text": "a.txt"}}})
        + "\n"
        + match_event("a.txt", 3, MARKER + " here\n")
        + json.dumps({"type": "end", "data": {"path": {"text": "a.txt"}}})
        + "\n"
    )
    monkeypatch.setattr(
        rg_engine.subprocess, "run", lambda *a, **k: fake_completed(0, stdout=stream.encode())
    )
    assert rg_engine.run("root", ["x"], ["a.txt"]) == {"a.txt": {3: MARKER + " here"}}


def test_bytes_fields_decoded_with_replacement(monkeypatch: pytest.MonkeyPatch) -> None:
    event = {
        "type": "match",
        "data": {
            "path": {"bytes": base64.b64encode("路径.txt".encode()).decode()},
            "lines": {"bytes": base64.b64encode(b"hit\xff here\n").decode()},
            "line_number": 9,
        },
    }
    monkeypatch.setattr(
        rg_engine.subprocess,
        "run",
        lambda *a, **k: fake_completed(0, stdout=(json.dumps(event) + "\n").encode()),
    )
    hits = rg_engine.run("root", ["x"], ["路径.txt"])
    assert hits == {"路径.txt": {9: "hit\ufffd here"}}


def test_multiline_hit_reports_start_line_and_span(tmp_path: Path) -> None:
    (tmp_path / "a.rs").write_bytes('info!(\n    "中文",\n    x = 1\n);\n'.encode())
    hits = rg_engine.run(str(tmp_path), [r"info!\([^)]*中[^)]*\)"], ["a.rs"], multiline=True)
    assert hits == {"a.rs": {1: 'info!(\n    "中文",\n    x = 1\n);'}}


def test_single_line_mode_never_spans_lines(tmp_path: Path) -> None:
    (tmp_path / "a.txt").write_bytes(("begin " + MARKER + "\nend\n").encode())
    hits = rg_engine.run(str(tmp_path), ["begin.*" + MARKER], ["a.txt"])
    assert hits == {"a.txt": {1: "begin " + MARKER}}
