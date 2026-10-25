''''exec python3 -- "$0" "$@" # '''  # fmt: skip

#!/usr/bin/env python3
from __future__ import annotations

"""编码助手（ZCode）提交前钩子：拉取请求的标题与正文在提交前先过一遍提交
信息规范与书写规则校验。由客户端共享配置的提交前事件调用；手动测试时向
标准输入喂入钩子载荷。

行为：
  - 从标准输入的钩子载荷读工具名与工具入参；
  - 拉取请求的新建与编辑工具 → 校验其中的标题与正文字段；
  - 命令行调用 → 按外壳词法分词后定位建链或改题的命令，只在其后找标题与
    正文旗标（几种传值写法等价）；旗标取不到（自动填充、开网页、外部文件、
    无旗标、分词失败）或运行时才可知（值含变量展开与反引号）→ 放行；
  - 标题校验是压缩合并产物的模拟：远端合并时给标题追加编号后缀，按最长
    常见占位拼接后送校验，超限判定对齐合并后的真实形态；正文按书写规则
    与模板必需小节判定；
  - 校验失败 → 校验器的错误输出进标准错误，退出码 2 阻断本次工具调用；
  - 字段不存在、不适用、或钩子自身任何异常 → 退出码 0 放行，不误伤正常
    提交。
"""

import json  # noqa: E402 —— 跨壳引导串占了文档字符串位
import os  # noqa: E402
import shlex  # noqa: E402
import sys  # noqa: E402
from typing import Any  # noqa: E402

PROJ_DIR = os.environ.get("ZCODE_PROJECT_DIR") or os.environ.get("CLAUDE_PROJECT_DIR") or "."
CHECKER_DIR = os.path.join(PROJ_DIR, "scripts", "hooks")
PR_TOOLS = ("mcp__github__create_pull_request", "mcp__github__update_pull_request")


def extract_field(data: dict[str, Any], field: str) -> str:
    """从钩子载荷抽指定字段：接口工具取字段值，命令行按旗标定位。"""
    tool = data.get("tool_name") or ""
    tool_input = data.get("tool_input") or {}
    if not isinstance(tool_input, dict):
        tool_input = {}
    if tool in PR_TOOLS:
        return str(tool_input.get(field) or "").strip()
    if tool != "Bash":
        return ""
    try:
        toks = shlex.split(str(tool_input.get("command") or ""))
    except ValueError:
        return ""
    for i in range(len(toks) - 2):
        if not (
            (toks[i] == "gh" or toks[i].endswith("/gh"))
            and toks[i + 1] == "pr"
            and toks[i + 2] in ("create", "edit")
        ):
            continue
        rest = []
        for x in toks[i + 3 :]:
            if x in (";", "&&", "||", "|", "&"):
                break
            rest.append(x)
        if (
            "--fill" in rest
            or "--web" in rest
            or any(x.startswith("--%s-file" % field) for x in rest)
        ):
            return ""
        value = ""
        for j, x in enumerate(rest):
            if x == "--%s" % field and j + 1 < len(rest):
                value = rest[j + 1].strip()
                break
            if x.startswith("--%s=" % field):
                value = x[len(field) + 3 :].strip()
                break
        # 值含美元符或反引号：运行时才可知，静态判不了，放行
        if value and ("$" in value or "`" in value):
            return ""
        return value
    return ""


def main() -> int:
    try:
        payload = sys.stdin.read()
    except Exception:
        return 0
    # 廉价预筛：载荷不含拉取请求工具名与建链命令片段时不启动解析；宁可漏进
    # 全量解析（放行），不做误筛
    if not (
        "mcp__github__create_pull_request" in payload
        or "mcp__github__update_pull_request" in payload
        or "gh pr" in payload
    ):
        return 0

    try:
        data = json.loads(payload)
    except Exception:
        return 0
    if not isinstance(data, dict) or not os.path.isdir(CHECKER_DIR):
        return 0
    title = extract_field(data, "title")
    body = extract_field(data, "body")
    if not title and not body:
        return 0

    sys.path.insert(0, CHECKER_DIR)
    problems = []
    try:
        if title:
            import check_commit_msg

            simulated = check_commit_msg.run(check_commit_msg.simulate_squash(title))
            if simulated:
                problems.append("\n".join(simulated))
        if body:
            import check_pr_prose

            problems += check_pr_prose.run(PROJ_DIR, body)
    except Exception:
        return 0

    if problems:
        for p in problems:
            print(p, file=sys.stderr)
        print(
            "PR content failed the pre-submission check (squash-merge simulation for"
            " the title, prose rules and template sections for the body);"
            " blocking this submission:",
            file=sys.stderr,
        )
        print("  your title: %s" % (title or "(absent)"), file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
