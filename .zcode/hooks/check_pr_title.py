''''exec python3 -- "$0" "$@" # '''  # fmt: skip

#!/usr/bin/env python3
from __future__ import annotations

"""编码助手（ZCode）提交前钩子：拉取请求的标题与正文、评审人对拉取请求提交
的评审结果，在提交前先过一遍提交信息规范、书写规则与两份模板的校验。由客
户端共享配置的提交前事件调用；手动测试时向标准输入喂入钩子载荷。

行为：
  - 从标准输入的钩子载荷读工具名与工具入参；
  - 拉取请求的新建与编辑工具 → 校验其中的标题与正文字段（正文模板）；
  - 评审提交工具 → 校验正文（评审结果模板）；
  - 命令行调用 → 按外壳词法分词后定位建链、改题或评审的命令，只在其后找
    标题与正文旗标（长旗标与短旗标的空间、等号、连写等价）；旗标取不到
    （自动填充、开网页、外部文件、无旗标、分词失败）或运行时才可知（值含
    变量展开与反引号）→ 放行；
  - 标题校验是压缩合并产物的模拟：远端合并时给标题追加编号后缀，按最长
    常见占位拼接后送校验，超限判定对齐合并后的真实形态；拉取请求正文按书
    写规则与正文模板必需小节判定，评审结果按评审模板骨架、声明核对与书写
    规则判定；
  - 校验失败 → 校验器的错误输出进标准错误，退出码 2 阻断本次工具调用；
  - 字段不存在、不适用、或钩子自身任何异常 → 退出码 0 放行，不误伤正常
    提交。
"""

import json  # noqa: E402 —— 跨壳引导串占了文档字符串位
import os  # noqa: E402
import shlex  # noqa: E402
import sys  # noqa: E402

PROJ_DIR = os.environ.get("ZCODE_PROJECT_DIR") or os.environ.get("CLAUDE_PROJECT_DIR") or "."
CHECKER_DIR = os.path.join(PROJ_DIR, "scripts", "hooks")
PR_TOOLS = ("mcp__github__create_pull_request", "mcp__github__update_pull_request")
REVIEW_TOOLS = (
    "mcp__github__create_and_submit_pull_request_review",
    "mcp__github__submit_pending_pull_request_review",
)
GH_SUBS = ("create", "edit", "review")
SHELL_BREAKS = (";", "&&", "||", "|", "&")


def runtime_safe(value: str) -> str:
    """值含变量展开或反引号即运行时才可知，静态判不了，放行返回空。"""
    value = value.strip()
    if "$" in value or "`" in value:
        return ""
    return value


def pull_flag(rest: list[str], long_name: str, short_name: str) -> str:
    """从旗标残余里取值：长旗标与短旗标的空间、等号与连写等价；取不到返回空。"""
    for j, x in enumerate(rest):
        if x == long_name and j + 1 < len(rest):
            return runtime_safe(rest[j + 1])
        if x.startswith(long_name + "="):
            return runtime_safe(x[len(long_name) + 1 :])
        if x == short_name and j + 1 < len(rest):
            return runtime_safe(rest[j + 1])
        if x.startswith(short_name + "="):
            return runtime_safe(x[len(short_name) + 1 :])
        if x.startswith(short_name) and len(x) > len(short_name):
            return runtime_safe(x[len(short_name) :].lstrip("="))
    return ""


def fields_after_subcommand(rest: list[str], sub: str) -> tuple[str, str, str]:
    """子命令之后的字段抽取：建链改链取标题与正文，评审只取正文；自动填充、
    开网页与外部文件旗标在，静态取不到值，整体放行。"""
    if "--fill" in rest or "-f" in rest or "--web" in rest or "-w" in rest:
        return "", "", ""
    if sub == "review":
        for marker in ("--body-file", "-F"):
            if marker in rest:
                return "", "", ""
        return "", "", pull_flag(rest, "--body", "-b")
    title = ""
    if "--title-file" not in rest and "-T" not in rest:
        title = pull_flag(rest, "--title", "-t")
    body = ""
    if "--body-file" not in rest and "-F" not in rest:
        body = pull_flag(rest, "--body", "-b")
    return title, body, ""


def extract(data: dict) -> tuple[str, str, str]:
    """抽取标题、拉取请求正文与评审结果正文：接口工具直接取入参字段，命令行
    分词后按子命令与旗标定位；三者都取不到返回空串。"""
    tool = data.get("tool_name") or ""
    tool_input = data.get("tool_input") or {}
    if not isinstance(tool_input, dict):
        tool_input = {}
    if tool in PR_TOOLS:
        return (
            str(tool_input.get("title") or "").strip(),
            str(tool_input.get("body") or "").strip(),
            "",
        )
    if tool in REVIEW_TOOLS:
        return "", "", str(tool_input.get("body") or "").strip()
    if tool != "Bash":
        return "", "", ""
    try:
        toks = shlex.split(str(tool_input.get("command") or ""))
    except ValueError:
        return "", "", ""
    for i in range(len(toks) - 2):
        if not (
            (toks[i] == "gh" or toks[i].endswith("/gh"))
            and toks[i + 1] == "pr"
            and toks[i + 2] in GH_SUBS
        ):
            continue
        rest = []
        for x in toks[i + 3 :]:
            if x in SHELL_BREAKS:
                break
            rest.append(x)
        return fields_after_subcommand(rest, toks[i + 2])
    return "", "", ""


def main() -> int:
    try:
        payload = sys.stdin.read()
    except Exception:
        return 0
    # 廉价预筛：载荷不含拉取请求工具名与建链或评审命令片段时不启动解析；宁
    # 可漏进全量解析（放行），不做误筛
    if not (
        "mcp__github__create_pull_request" in payload
        or "mcp__github__update_pull_request" in payload
        or "pull_request_review" in payload
        or "gh pr" in payload
    ):
        return 0

    try:
        data = json.loads(payload)
    except Exception:
        return 0
    if not isinstance(data, dict) or not os.path.isdir(CHECKER_DIR):
        return 0
    title, body, review = extract(data)
    if not title and not body and not review:
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
        if review:
            import check_review_result

            problems += check_review_result.run(PROJ_DIR, review)
    except Exception:
        return 0

    if problems:
        for p in problems:
            print(p, file=sys.stderr)
        print(
            "PR or review content failed the pre-submission check (squash-merge"
            " simulation for the title, prose rules and template sections for the"
            " pull request body, review template for the review result);"
            " blocking this submission:",
            file=sys.stderr,
        )
        print("  your title: %s" % (title or "(absent)"), file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
