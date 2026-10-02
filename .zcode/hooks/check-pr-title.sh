#!/usr/bin/env bash
# ZCode PreToolUse 钩子：提交 PR 标题前用 scripts/hooks/check-commit-msg.sh 校验。
# 由 .zcode/config.json 的 hooks.events.PreToolUse 调用；也可手动测试：
#   echo '<hook-json>' | bash .zcode/hooks/check-pr-title.sh
#
# 行为：
#   - 从 stdin 的钩子 JSON 读 tool_name 与 tool_input；
#   - mcp__github__create_pull_request / mcp__github__update_pull_request → 校验 tool_input.title；
#   - Bash → 按 shell 词法（shlex）分词后定位 gh pr create|edit，只在其后找 --title
#     （--title "x" / 'x' / =x 等价）；标题取不到（--fill/--web/--title-file、无 --title、
#     分词失败）或运行时才可知（值含 $、反引号）→ 放行；
#   - 标题校验失败 → check-commit-msg 的错误输出进 stderr，退出码 2 阻断本次工具调用；
#   - 标题不存在、不适用、或钩子自身任何异常 → 退出码 0 放行，不误伤正常提交。
set -uo pipefail

PROJ_DIR="${ZCODE_PROJECT_DIR:-${CLAUDE_PROJECT_DIR:-.}}"
CHECK="$PROJ_DIR/scripts/hooks/check-commit-msg.sh"

PY="$(command -v python3 || true)"
PY="${PY:-/usr/bin/python3}"

INPUT="$(cat 2>/dev/null)" || INPUT=""

# 廉价预筛：载荷不含 PR 工具名与 gh pr 时不启动 python；宁可漏进全量解析（放行），不做误筛
case "$INPUT" in
  *mcp__github__create_pull_request*|*mcp__github__update_pull_request*|*"gh pr"*) ;;
  *) exit 0 ;;
esac

# python 代码经 -c 单引号传入：命令替换内嵌 heredoc 时，正文里的 $ 与反引号会被 bash
# 误当作替换起点解析；单引号 + 用 \x60 表示反引号可彻底规避
TITLE="$("$PY" -c '
import json, shlex, sys

try:
    data = json.loads(sys.argv[1])
except Exception:
    sys.exit(0)

if not isinstance(data, dict):
    sys.exit(0)

tool = data.get("tool_name") or ""
ti = data.get("tool_input") or {}
if not isinstance(ti, dict):
    ti = {}

title = ""
if tool in ("mcp__github__create_pull_request", "mcp__github__update_pull_request"):
    title = str(ti.get("title") or "").strip()
elif tool == "Bash":
    try:
        toks = shlex.split(str(ti.get("command") or ""))
    except ValueError:
        toks = []
    for i in range(len(toks) - 2):
        if (toks[i] == "gh" or toks[i].endswith("/gh")) and toks[i + 1] == "pr" and toks[i + 2] in ("create", "edit"):
            rest = toks[i + 3:]
            if "--fill" in rest or "--web" in rest or any(x.startswith("--title-file") for x in rest):
                break
            for j, x in enumerate(rest):
                if x == "--title":
                    if j + 1 < len(rest):
                        title = rest[j + 1].strip()
                    break
                if x.startswith("--title="):
                    title = x[8:].strip()
                    break
            break

if title and ("$" in title or "\x60" in title):
    title = ""

if title:
    sys.stdout.write(title)
' "$INPUT" 2>/dev/null)"

if [ -z "$TITLE" ] || [ ! -f "$CHECK" ]; then
  exit 0
fi

CHECK_ERR="$(printf '%s\n' "$TITLE" | bash "$CHECK" - 2>&1 >/dev/null)"
CHECK_RC=$?

if [ "$CHECK_RC" -ne 0 ]; then
  printf '%s\n' "$CHECK_ERR" >&2
  echo "PR 标题未通过 check-commit-msg 校验，已拦截本次提交（与 CI 对 PR 标题的校验一致）：" >&2
  echo "  你提交的标题是: ${TITLE}" >&2
  exit 2
fi

exit 0
