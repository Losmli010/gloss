"""约束门禁（路由）：聚合五个领域模块的判定，领域按数据源聚批划分——架构
与评测隔离、日志出口与语言、界面边界、清单卫生、源码卫生。聚合保持既有形
态：违规随执行流即时经统一诊断出口打到标准错误，通过项由命令入口汇总打印，
退出码按全脚本统一约定（零为通过、一为发现违规、二为用法错误、三为无法完成
判定）；本地约束配方与持续集成的同名检查作业共用此包，保证本地与远端判定
一致。领域划分、聚合序与单文件报告上限的取舍见记录文件。
"""

from __future__ import annotations

import os
import sys
from typing import Any

import gate_report

from . import architecture, logging, manifest_hygiene, manifests, source_hygiene, ui_boundary

MODULES = (architecture, logging, ui_boundary, manifest_hygiene, source_hygiene)


def run(
    root: str, loader: Any = None, report: manifests.Reporter | None = None
) -> tuple[list[str], list[str]]:
    """逐模块聚合判定，返回通过与失败两个清单；缺省自建单次扫描的内容缓存
    与违规报告器（清单与文件各只读一次、单文件报告设上限），装入器与报告器
    在场时共享。"""
    rep = report if report is not None else manifests.Reporter()
    print("== constraint checks (repo root: %s) ==" % root)
    scan = loader if loader is not None else manifests.ContentCache(root)
    oks = []
    for module in MODULES:
        module_oks, _module_fails = module.run(root, scan, rep)
        oks.extend(module_oks)
    return oks, rep.lines


def usage_failure(detail: str) -> int:
    print("usage: constraints/__main__.py [repo-root]", file=sys.stderr)
    print("  %s" % detail, file=sys.stderr)
    return gate_report.EXIT_USAGE


def main(loader: Any = None) -> int:
    if len(sys.argv) > 2:
        return usage_failure("too many arguments: expected at most one repo-root")
    root = (
        os.path.abspath(sys.argv[1])
        if len(sys.argv) > 1
        else os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
    )
    if len(sys.argv) > 1 and not os.path.isdir(root):
        return usage_failure("repo-root is not a directory: %s" % root)
    report = manifests.Reporter()
    oks, fails = run(root, loader, report)
    for line in oks:
        print("  ✓ %s" % line)
    if fails:
        print("", file=sys.stderr)
        report.declare_capped()
        gate_report.summary(report.findings)
        print("  - rule stays: fix the code;", file=sys.stderr)
        print(
            "  - rule really changed: update AGENTS.md and related configs first, then"
            " this script; never just one side.",
            file=sys.stderr,
        )
        return gate_report.EXIT_VIOLATION
    print("✓ constraint checks passed (%d checks)" % len(oks))
    return gate_report.EXIT_PASS
