#!/usr/bin/env python3
"""注释纪律门禁（路由）：遍历版本库跟踪文件，按文件类型分派到源码、脚本
与配置、文档三个门禁；判定规则全部沉淀在语言规范库，与提交信息、拉取请
求内容门禁同源共用。本地提交前门禁与持续集成的钩子工作流共用此入口，保
证本地与远端判定一致；本脚本自身注释同规。

受检面：源码只留中文文件头，注释主体全中文、专名只进括号注记，不变量注
释纯英文；脚本与配置注释逐块同规，脚本文件头统一模块级文档字符串；标记
语言文档逐行受检，注释记录目录与提示词目录不在受检面。

用法：以解释器运行本脚本，首参为仓库根，其余参数为可选的文件清单（供单测
传入临时样本）；缺省扫全部被跟踪文件。
"""

import os
import subprocess
import sys

import check_docs
import check_rust_comments
import check_script_comments


def scan_files(root, explicit):
    if explicit:
        return [p if os.path.isabs(p) else os.path.join(root, p) for p in explicit]
    out = subprocess.run(
        ["git", "-C", root, "ls-files"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    return [os.path.join(root, p) for p in out.splitlines() if p]


def run(root, explicit):
    """遍历跟踪文件，按文件类型分派并聚合违规。"""
    gates = (check_rust_comments, check_script_comments, check_docs)
    problems = []
    for path in scan_files(root, explicit):
        rel = os.path.relpath(path, root)
        gate = next((gate for gate in gates if gate.handles(rel)), None)
        if gate is None:
            continue
        try:
            with open(path, encoding="utf-8") as fh:
                src = fh.read()
        except (OSError, UnicodeDecodeError) as exc:
            problems.append(f"{rel}: unreadable ({exc})")
            continue
        problems.extend(gate.check(rel, src))
    return problems


def main():
    root = (
        sys.argv[1]
        if len(sys.argv) > 1
        else os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    )
    explicit = sys.argv[2:]
    print(f"== comment discipline gate (repo root: {root}) ==")
    problems = run(root, explicit)
    if problems:
        for p in problems:
            print(f"  ✗ {p}", file=sys.stderr)
        print(f"✗ comment discipline gate: {len(problems)} violations", file=sys.stderr)
        print(
            "  rules: see this script's header; move target docs/comments/<file>_comment.md",
            file=sys.stderr,
        )
        sys.exit(1)
    print(
        "  ok comment discipline: sources keep only Chinese file headers, comment"
        " bodies fully Chinese, proper nouns only in bracket annotations, invariant"
        " comments pure English"
    )


if __name__ == "__main__":
    main()
