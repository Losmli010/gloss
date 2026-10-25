#!/usr/bin/env python3
"""纯文本门禁并轨入口：一个进程依序执行六条纯文本门禁（密钥、文档引用、
行为清单、约束、文案、注释），门禁顺序与组合配方的清单序一致，聚合输出即
六条门禁各自单跑输出的拼接，任一门禁违规时整体退出非零且后续门禁照常执
行、报告保持完整。六门禁共享一次版本库跟踪清单的枚举结果与两层（字节、
解码）惰性内容缓存：跟踪清单与约束门禁的工作区清单各只枚举一次、文件内
容只读一次，经缓存读取的解码
容错口径与通用换行翻译逐点对齐各门禁自行打开文件的语义。各门禁的运行入
口带可选内容装入器参数，缺省自行读取，单条配方与既有单测行为不变。取舍
见记录文件。

用法：以解释器运行本脚本，无参数；仓库根取本脚本位置向上回溯。
"""

from __future__ import annotations

import io
import os
import subprocess
import sys
from types import ModuleType
from typing import Any

_HOOKS = os.path.dirname(os.path.abspath(__file__))
_COMMENTS = os.path.join(_HOOKS, "comments")
for _entry in (_COMMENTS, _HOOKS):
    if _entry not in sys.path:
        sys.path.insert(0, _entry)

import check_agents_doc  # noqa: E402
import check_comments  # noqa: E402
import check_i18n  # noqa: E402
import check_secrets  # noqa: E402
import check_test_bdd  # noqa: E402
import constraints  # noqa: E402

REPO_ROOT = os.path.abspath(os.path.join(_HOOKS, "..", ".."))

GATES = (
    ("secrets", check_secrets),
    ("agents-doc", check_agents_doc),
    ("test-bdd", check_test_bdd),
    ("constraints", constraints),
    ("i18n", check_i18n),
    ("comments", check_comments),
)


class ContentLoader:
    """并轨内容装入器：跟踪清单一次枚举，字节与解码文本两层按需缓存。

    解码层逐点对齐文本模式打开文件的读取语义：统一码容错口径由调用方指
    定并分开缓存，通用换行翻译照常发生，读取失败按原样抛出。约束门禁的
    工作区清单枚举同样经本装入器缓存（枚举逻辑住在约束共享层）。"""

    def __init__(self, root: str) -> None:
        self.root = root
        self._tracked = None
        self._manifests = None
        self._bytes = {}
        self._text = {}

    def _resolve(self, path: str) -> str:
        return path if os.path.isabs(path) else os.path.join(self.root, path)

    def tracked(self) -> list[str]:
        """版本库跟踪清单：全部消费方共享同一次枚举结果。"""
        if self._tracked is None:
            out = subprocess.run(
                ["git", "-C", self.root, "ls-files", "-z"],
                capture_output=True,
                check=True,
            ).stdout
            self._tracked = [p.decode("utf-8", "replace") for p in out.split(b"\0") if p]
        return self._tracked

    def manifests(self, root: str) -> list[str]:
        """工作区清单枚举：经约束共享层的枚举逻辑取一次，此后共享同一结果。"""
        if self._manifests is None:
            self._manifests = constraints.manifests.manifests(root)
        return self._manifests

    def read_bytes(self, path: str) -> bytes:
        """字节层：按需读取并缓存，供逐字节数据与手工解码的消费方共用。"""
        key = self._resolve(path)
        if key not in self._bytes:
            with open(key, "rb") as fh:
                self._bytes[key] = fh.read()
        return self._bytes[key]

    def read_text(self, path: str, errors: str = "strict") -> str:
        """解码层：按容错口径分别缓存，返回内容与文本模式读取逐字节一致。"""
        key = (self._resolve(path), errors)
        if key not in self._text:
            with io.TextIOWrapper(
                io.BytesIO(self.read_bytes(key[0])), encoding="utf-8", errors=errors
            ) as reader:
                self._text[key] = reader.read()
        return self._text[key]


def run_gate(module: ModuleType, loader: Any) -> int:
    """执行单个门禁并返回其退出码；前置失败按该门禁失败计，不中断后续门禁。"""
    try:
        return module.main(loader=loader)
    except SystemExit as exc:
        if isinstance(exc.code, str):
            print(exc.code, file=sys.stderr)
        return 0 if exc.code in (None, 0) else 1


def main() -> int:
    if len(sys.argv) > 1:
        print("error: the merged text-gates entry takes no arguments", file=sys.stderr)
        print("  it always scans the repository it lives in;", file=sys.stderr)
        print("  run an individual gate recipe to scope a single check.", file=sys.stderr)
        return 1
    loader = ContentLoader(REPO_ROOT)
    failed = False
    for _name, module in GATES:
        if run_gate(module, loader) != 0:
            failed = True
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
