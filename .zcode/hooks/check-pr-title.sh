#!/usr/bin/env bash
# ZCode PreToolUse 钩子：提交 PR 标题前用 scripts/hooks/check-commit-msg.sh 校验。
# 由 .zcode/config.json 的 hooks.events.PreToolUse 调用；也可手动测试：
#   echo '<hook-json>' | bash .zcode/hooks/check-pr-title.sh
#
# 行为：
#   - 从 stdin 的钩子 JSON 读 tool_name 与 tool_input；
#   - mcp__github__create_pull_request / mcp__github__update_pull_request → 校验 tool_input.title；
#   - Bash → 命令含 gh pr create|edit 且能提取 --title 的值时校验，提取不到则放行；
#   - 标题校验失败 → check-commit-msg 的错误输出进 stderr，退出码 2 阻断本次工具调用；
#   - 标题不存在、不适用、或钩子自身任何异常 → 退出码 0 放行，不误伤正常提交。
set -uo pipefail

PROJ_DIR="${ZCODE_PROJECT_DIR:-${CLAUDE_PROJECT_DIR:-.}}"
CHECK="$PROJ_DIR/scripts/hooks/check-commit-msg.sh"

PY="$(command -v python3 || true)"
PY="${PY:-/usr/bin/python3}"

INPUT="$(cat 2>/dev/null)" || INPUT=""

TITLE="$("$PY" - "$INPUT" <<'PYEOF' 2>/dev/null
import json, re, sys

try:
    data = json.loads(sys.argv[1])
except Exception:
    sys.exit(0)

tool = data.get("tool_name") or ""
ti = data.get("tool_input") or {}
if not isinstance(ti, dict):
    ti = {}

title = ""
if tool in ("mcp__github__create_pull_request", "mcp__github__update_pull_request"):
    title = str(ti.get("title") or "").strip()
elif tool == "Bash":
    cmd = str(ti.get("command") or "")
    if re.search(r"\bgh\s+pr\s+(create|edit)\b", cmd):
        m = re.search(r"--title(?:=|\s+)(\"[^\"]*\"|'[^']*'|\S+)", cmd)
        if m:
            v = m.group(1)
            if len(v) >= 2 and v[0] == v[-1] and v[0] in "\"'":
                v = v[1:-1]
            title = v.strip()

if title:
    sys.stdout.write(title)
PYEOF
)"

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
