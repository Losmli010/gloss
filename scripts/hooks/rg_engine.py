#!/usr/bin/env python3
"""正则检索工具（ripgrep）共享引擎：门禁对文件内容的正则检索一律委托引擎
单点执行，本模块是唯一调用点——表达式清单加文件清单一次子进程跑完，命中
整理成“路径到行号与行文”的嵌套映射返回，判定仍由调用方收口。多行模式供
跨行模式匹配的检查使用；清单为空直接返回空映射，不起子进程。引擎缺失与
异常流按失败关闭处置：调用方收到运行时异常后经统一诊断出口报无法完成判
定。本机依赖与钉版本见仓库工具清单，环境初始化负责校验在场。
"""

from __future__ import annotations

import base64
import json
import subprocess
from typing import Any


def _json_field(blob: dict[str, Any]) -> str:
    """引擎输出的取值字段：文本直取，字节按编码还原后容错解码。"""
    if "text" in blob:
        return blob["text"]
    return base64.b64decode(blob["bytes"]).decode("utf-8", "replace")


def run(
    root: str, expressions: list[str], files: list[str], multiline: bool = False
) -> dict[str, dict[int, str]]:
    """一次子进程跑全部表达式，命中整理成「路径到行号与行文」的嵌套映射。

    路径键与调用方传入的文件清单同形（相对或绝对由调用方定）；多行模式
    开启时跨行命中的行号取起始行，行文是命中跨住的全部行。"""
    if not files or not expressions:
        return {}
    cmd = ["rg", "--json", "--no-config"]
    if multiline:
        cmd.append("--multiline")
    for expression in expressions:
        cmd += ["-e", expression]
    cmd += ["--"]
    cmd += files
    try:
        proc = subprocess.run(cmd, cwd=root, capture_output=True)
    except FileNotFoundError:
        raise RuntimeError(
            "ripgrep not found: the text gates depend on it on every commit —"
            " install the pinned version from the repo tool list (just setup verifies it)"
        ) from None
    stderr = proc.stderr.decode("utf-8", "replace").strip()
    if proc.returncode == 2:
        raise RuntimeError("ripgrep engine failed (exit 2): %s" % stderr)
    if proc.returncode not in (0, 1):
        raise RuntimeError(
            "ripgrep engine exited unexpectedly (%d): %s" % (proc.returncode, stderr)
        )
    hits: dict[str, dict[int, str]] = {}
    # 事件流按换行符逐字切：通用换行语义会把行文里未转义的同类字符当切点撕裂事件
    for line in proc.stdout.decode("utf-8", "replace").split("\n"):
        if not line:
            continue
        try:
            event = json.loads(line)
            if event.get("type") != "match":
                continue
            data = event["data"]
            rel = _json_field(data["path"])
            text = _json_field(data["lines"])
            if text.endswith("\n"):
                text = text[:-1]
        except (ValueError, KeyError, TypeError) as exc:
            raise RuntimeError(
                "ripgrep produced an unparsable event stream (%r); refusing to"
                " continue — report this as a gate failure" % exc
            ) from None
        hits.setdefault(rel, {})[data["line_number"]] = text
    return hits
