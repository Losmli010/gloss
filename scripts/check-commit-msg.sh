#!/usr/bin/env bash
# 校验 commit message 是否符合 Conventional Commits 规范。
# CI 的 commitlint workflow 与本地手动校验（`just lint-commit <file>`）共用此脚本，保证校验逻辑一致。
# 注意：本地钩子不做消息校验——git 跑 pre-commit 时消息还没落盘，读到的会是上一条提交的消息。
#
# 用法：
#   scripts/check-commit-msg.sh <commit-message-file-or-text>
#   - 传文件路径：读取文件内容作为 commit message（本地 hook 场景）
#   - 传 "-" ：从 stdin 读取（CI 场景）
set -euo pipefail

# ---- 配置区 ----
# 允许的 type 前缀
ALLOWED_TYPES="feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert"
# header 最大长度（type + scope + subject 合计，按字节计）
MAX_SUBJECT_LEN=72

# ---- 读取 commit message ----
if [ "$#" -lt 1 ]; then
  echo "用法: $0 <commit-message-file|->" >&2
  exit 2
fi

if [ "$1" = "-" ]; then
  MSG="$(cat)"
else
  MSG="$(cat "$1")"
fi

# 去掉注释行（git 会忽略 # 开头的行）
MSG="$(printf '%s\n' "$MSG" | sed '/^#/d')"

# 去掉开头的空白行（这样第一行一定是 header）。
# 用 awk 的 NF 判断空行，跨平台（GNU/BSD/mawk）行为一致，
# 避免 sed 的 [[:space:]] 字符类在不同 locale/sed 实现下的差异。
MSG="$(printf '%s\n' "$MSG" | awk 'NF { found=1 } found { print }')"

if [ -z "$MSG" ]; then
  echo "错误：commit message 为空" >&2
  exit 1
fi

# 取第一行（header）
HEADER="$(printf '%s\n' "$MSG" | head -n 1)"

# ---- 校验 header 格式：type(scope): subject ----
if ! printf '%s' "$HEADER" | grep -Eq "^(${ALLOWED_TYPES})(\([a-zA-Z0-9._-]+\))?!?: "; then
  echo "错误：commit message 不符合 Conventional Commits 规范" >&2
  echo "" >&2
  echo "  规范格式: <type>[optional scope]: <subject>" >&2
  echo "  type 可选: ${ALLOWED_TYPES}" >&2
  echo "  示例:" >&2
  echo "    feat: 添加翻译浮层" >&2
  echo "    fix(core): 修复选区读取崩溃" >&2
  echo "    chore(deps): 升级 egui 到 0.31" >&2
  echo "    docs: 补充使用说明" >&2
  echo "" >&2
  echo "  你提交的是: ${HEADER}" >&2
  exit 1
fi

# ---- 校验 subject 非空 ----
# 提取 subject 部分（去掉 "type(scope)!: " 前缀）
SUBJECT="$(printf '%s' "$HEADER" | sed -E 's/^[a-z]+(\([a-zA-Z0-9._-]+\))?!?: //')"
if [ -z "$SUBJECT" ]; then
  echo "错误：commit 标题缺少主题（冒号后应有内容）" >&2
  echo "  你提交的是: ${HEADER}" >&2
  exit 1
fi

# ---- 校验 subject 长度（用字节数，跨 bash 版本一致）----
HEADER_LEN="$(printf '%s' "$HEADER" | wc -c | tr -d ' ')"
if [ "$HEADER_LEN" -gt "$MAX_SUBJECT_LEN" ]; then
  echo "错误：commit 标题过长（${HEADER_LEN} 字节，上限 ${MAX_SUBJECT_LEN}）" >&2
  echo "  建议精简，把细节放到正文" >&2
  echo "  你提交的是: ${HEADER}" >&2
  exit 1
fi

# ---- 校验 body 与 header 之间有空行（若有 body）----
LINE_COUNT="$(printf '%s\n' "$MSG" | wc -l | tr -d ' ')"
if [ "$LINE_COUNT" -gt 1 ]; then
  SECOND_LINE="$(printf '%s\n' "$MSG" | sed -n '2p')"
  if [ -n "$SECOND_LINE" ]; then
    echo "错误：commit 标题后应有一个空行，再写正文" >&2
    echo "  第二行应为空行，你写的是: ${SECOND_LINE}" >&2
    exit 1
  fi
fi

echo "✓ commit message 校验通过: ${HEADER}"
exit 0
