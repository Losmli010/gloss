#!/usr/bin/env python3
"""注释纪律门禁（路由）：遍历版本库跟踪文件，按文件类型分派到源码、脚本
与配置、文档三个门禁；判定规则全部沉淀在语言规范库，与提交信息、拉取请
求内容门禁同源共用。本地提交前门禁与持续集成的钩子工作流共用此入口，保
证本地与远端判定一致；本脚本自身注释同规。

受检面：源码只留中文文件头，注释主体全中文、专名只进括号注记，不变量注
释纯英文；脚本与配置注释逐块同规，脚本文件头统一模块级文档字符串；标记
语言文档逐行受检，注释记录目录与提示词目录不在受检面。违规经统一诊断出
口带规则标识即时打印，退出码按全脚本统一约定；跟踪清单为空按失败关闭退
出，文件不可读属无法完成判定，同样失败关闭。

用法：以解释器运行本脚本，首参为仓库根，其余参数为可选的文件清单（供单测
传入临时样本）；缺省扫全部被跟踪文件。
"""

from __future__ import annotations

import os
import sys
from typing import Any

import check_docs
import check_rust_comments
import check_script_comments
import gate_report

# 门禁脚本在钩子目录下再下一层，仓库根要回溯三级；退回两级会把受检面缩到脚本目录
REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))

EMPTY_SURFACE_MESSAGE = "the comment scan surface is empty: no tracked files found"

EMPTY_SURFACE_REMEDY = (
    "run the gate from the workspace root; an empty tracked list means the comment"
    " gates cannot complete"
)

OK_LINE = (
    "ok comment discipline: sources keep only Chinese file headers, comment"
    " bodies fully Chinese, proper nouns only in bracket annotations, invariant"
    " comments pure English"
)


def scan_files(root: str, explicit: list[str], loader: Any = None) -> list[str]:
    """枚举受检文件：显式清单优先（供单测传样本），否则取版本库跟踪清单。

    跟踪清单走空字节分隔取原始字节再容错解码——非空分隔形态对非英文字
    节（ASCII）文件名做引号八进制转义，假路径过不了类型分派，文件被静默
    漏检。"""
    if explicit:
        return [p if os.path.isabs(p) else os.path.join(root, p) for p in explicit]
    if loader is not None:
        return [os.path.join(root, p) for p in loader.tracked()]
    out = gate_report.run_git(["ls-files", "-z"], cwd=root).stdout
    return [os.path.join(root, p.decode("utf-8", "replace")) for p in out.split(b"\0") if p]


def read_src(path: str, loader: Any) -> str:
    if loader is not None:
        return loader.read_text(path)
    with open(path, encoding="utf-8") as fh:
        return fh.read()


def collect(
    root: str, explicit: list[str], loader: Any = None
) -> tuple[list[str], list[gate_report.Finding]]:
    """遍历跟踪文件，按文件类型分派并聚合违规，违规随执行流即时经统一诊断
    出口打到标准错误，返回通过与违规条目两个清单；装入器在场时清单枚举与
    文件读取走共享缓存，缺省自行读取。"""
    gates = (check_rust_comments, check_script_comments, check_docs)
    paths = scan_files(root, explicit, loader)
    if not explicit and not paths:
        gate_report.fail(
            gate_report.Finding(
                rule=gate_report.RULE_EMPTY_SURFACE,
                path="",
                line=0,
                message=EMPTY_SURFACE_MESSAGE,
                remedy=EMPTY_SURFACE_REMEDY,
            )
        )
        sys.exit(gate_report.EXIT_INFRA)
    found: list[gate_report.Finding] = []
    for path in paths:
        rel = os.path.relpath(path, root)
        gate = next((gate for gate in gates if gate.handles(rel)), None)
        if gate is None:
            continue
        try:
            src = read_src(path, loader)
        except (OSError, UnicodeDecodeError) as exc:
            gate_report.fail(
                gate_report.infra_failure("cannot read tracked file: %s" % rel, evidence=str(exc))
            )
            sys.exit(gate_report.EXIT_INFRA)
        for finding in gate.check(rel, src):
            found.append(finding)
            gate_report.fail(finding)
    oks = [OK_LINE] if not found else []
    return oks, found


def run(root: str, explicit: list[str], loader: Any = None) -> list[str]:
    """遍历跟踪文件，按文件类型分派并聚合违规，返回渲染行（消息加位置）清
    单供调用方取用；装入器在场时清单枚举与文件读取走共享缓存，缺省自行读
    取。"""
    _oks, found = collect(root, explicit, loader)
    return [gate_report.render(finding) for finding in found]


def usage_failure(detail: str) -> int:
    print("usage: check_comments.py [repo-root] [file ...]", file=sys.stderr)
    print("  %s" % detail, file=sys.stderr)
    return gate_report.EXIT_USAGE


def main(loader: Any = None) -> int:
    root = sys.argv[1] if len(sys.argv) > 1 else REPO_ROOT
    explicit = sys.argv[2:]
    if len(sys.argv) > 1 and not os.path.isdir(root):
        return usage_failure("repo-root is not a directory: %s" % root)
    print(f"== comment discipline gate (repo root: {root}) ==")
    oks, found = collect(root, explicit, loader)
    for line in oks:
        print(f"  ✓ {line}")
    if found:
        print("", file=sys.stderr)
        gate_report.summary(found)
        print(
            "  rules: see this script's header; move target docs/comments/<file>_comment.md",
            file=sys.stderr,
        )
        return gate_report.EXIT_VIOLATION
    return gate_report.EXIT_PASS


if __name__ == "__main__":
    sys.exit(main())
