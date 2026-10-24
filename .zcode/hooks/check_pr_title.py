#!/usr/bin/env python3
# ZCode PreToolUse 钩子：提交 PR 标题前用 scripts/hooks/check_commit_msg.py 校验。
# 由 .zcode/config.json 的 hooks.events.PreToolUse 调用；也可手动测试：
#   echo '<hook-json>' | python3 .zcode/hooks/check_pr_title.py
#
# 行为：
#   - 从 stdin 的钩子 JSON 读 tool_name 与 tool_input；
#   - mcp__github__create_pull_request / mcp__github__update_pull_request → 校验 tool_input.title；
#   - Bash → 按 shell 词法（shlex）分词后定位 gh pr create|edit，只在其后找 --title
#     （--title "x" / 'x' / =x 等价）；标题取不到（--fill/--web/--title-file、无 --title、
#     分词失败）或运行时才可知（值含 $、反引号）→ 放行；
#   - 校验串是 squash 合并产物的模拟：GitHub 合并时给标题追加 (#PR号)，按最长常见
#     占位 (#99999) 拼接后送校验，超限判定对齐合并后的真实形态；
#   - 标题校验失败 → check_commit_msg 的错误输出进 stderr，退出码 2 阻断本次工具调用；
#   - 标题不存在、不适用、或钩子自身任何异常 → 退出码 0 放行，不误伤正常提交。

import json
import os
import shlex
import sys

PROJ_DIR = os.environ.get("ZCODE_PROJECT_DIR") or os.environ.get("CLAUDE_PROJECT_DIR") or "."
CHECKER_DIR = os.path.join(PROJ_DIR, "scripts", "hooks")


def extract_title(data):
    tool = data.get("tool_name") or ""
    tool_input = data.get("tool_input") or {}
    if not isinstance(tool_input, dict):
        tool_input = {}

    if tool in ("mcp__github__create_pull_request", "mcp__github__update_pull_request"):
        return str(tool_input.get("title") or "").strip()

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
        if "--fill" in rest or "--web" in rest or any(x.startswith("--title-file") for x in rest):
            break
        title = ""
        for j, x in enumerate(rest):
            if x == "--title" and j + 1 < len(rest):
                title = rest[j + 1].strip()
                break
            if x.startswith("--title="):
                title = x[8:].strip()
                break
        # 值含 $ 或反引号：运行时才可知，静态判不了，放行
        if title and ("$" in title or "`" in title):
            title = ""
        return title
    return ""


def main():
    try:
        payload = sys.stdin.read()
    except Exception:
        return 0
    # 廉价预筛：载荷不含 PR 工具名与 gh pr 时不启动解析；宁可漏进全量解析（放行），
    # 不做误筛
    if not (
        "mcp__github__create_pull_request" in payload
        or "mcp__github__update_pull_request" in payload
        or "gh pr" in payload
    ):
        return 0

    try:
        data = json.loads(payload)
        title = extract_title(data) if isinstance(data, dict) else ""
    except Exception:
        return 0
    if not title or not os.path.isdir(CHECKER_DIR):
        return 0

    sys.path.insert(0, CHECKER_DIR)
    try:
        import check_commit_msg

        problem = check_commit_msg.run("%s (#99999)" % title)
    except Exception:
        return 0

    if problem:
        print(problem, file=sys.stderr)
        print(
            'PR 标题按 squash 合并产物（标题 + " (#99999)"）模拟校验未通过，已拦截本次提交：',
            file=sys.stderr,
        )
        print("  你提交的标题是: %s" % title, file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
