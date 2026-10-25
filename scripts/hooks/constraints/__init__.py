"""约束门禁（路由）：聚合五个领域模块的判定，领域按数据源聚批划分——架构
与评测隔离、日志出口与语言、界面边界、清单卫生、源码卫生。聚合保持既有
形态：违规随执行流即时打到标准错误，通过项由命令入口汇总打印，退出语义
不变；本地约束配方与持续集成的同名检查作业共用此包，保证本地与远端判定
一致。领域划分与聚合序的取舍见记录文件。
"""

from __future__ import annotations

import os
import sys
from typing import Any

from . import architecture, logging, manifest_hygiene, manifests, source_hygiene, ui_boundary

MODULES = (architecture, logging, ui_boundary, manifest_hygiene, source_hygiene)


def run(root: str, loader: Any = None) -> tuple[list[str], list[str]]:
    """逐模块聚合判定，返回通过与失败两个清单；缺省自建单次扫描的内容缓存
    （清单与文件各只读一次），装入器在场时共享其缓存。"""
    print("== constraint checks (repo root: %s) ==" % root)
    scan = loader if loader is not None else manifests.ContentCache(root)
    oks = []
    fails = []
    for module in MODULES:
        module_oks, module_fails = module.run(root, scan)
        oks.extend(module_oks)
        fails.extend(module_fails)
    return oks, fails


def main(loader: Any = None) -> int:
    root = (
        os.path.abspath(sys.argv[1])
        if len(sys.argv) > 1
        else os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
    )
    oks, fails = run(root, loader)
    for line in oks:
        print("  ✓ %s" % line)
    if fails:
        print("", file=sys.stderr)
        print(
            "error: %d checks failed the automated gates (see above)." % len(fails), file=sys.stderr
        )
        print("  - rule stays: fix the code;", file=sys.stderr)
        print(
            "  - rule really changed: update AGENTS.md and related configs first, then"
            " this script; never just one side.",
            file=sys.stderr,
        )
        return 1
    print("✓ constraint checks passed (%d checks)" % len(oks))
    return 0
