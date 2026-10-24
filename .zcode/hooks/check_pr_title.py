''''exec python3 -- "$0" "$@" # '''  # fmt: skip
#!/usr/bin/env python3
# ZCode 提交前钩子：拉取请求标题在提交前先过一遍提交信息规范校验。
# 由客户端共享配置的提交前事件调用；手动测试时向标准输入喂入钩子载荷。
#
# 行为：
#   - 从标准输入的钩子载荷读工具名与工具入参；
#   - 拉取请求的新建与编辑工具 → 校验其中的标题字段；
#   - 命令行调用 → 按外壳词法分词后定位建链或改题的命令，只在其后找标题旗标
#     （几种传值写法等价）；标题取不到（自动填充、开网页、外部标题文件、
#     无旗标、分词失败）或运行时才可知（值含变量展开与反引号）→ 放行；
#   - 校验串是压缩合并产物的模拟：远端合并时给标题追加编号后缀，按最长常见
#     占位拼接后送校验，超限判定对齐合并后的真实形态；
#   - 标题校验失败 → 校验器的错误输出进标准错误，退出码 2 阻断本次工具调用；
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
