#!/usr/bin/env bash
# 安装本地 git hooks：把 core.hooksPath 指向 .githooks 目录，并给脚本加执行权限。
# 团队成员 clone 后只需运行一次：./scripts/install-hooks.sh
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

# 1. 给 hook 脚本加执行权限
chmod +x .githooks/pre-commit .githooks/commit-msg .githooks/pre-push
chmod +x scripts/check-commit-msg.sh

# 2. 设置 hooksPath 指向 .githooks
git config core.hooksPath .githooks

echo "✓ git hooks 已启用"
echo "  hooksPath: $(git config core.hooksPath)"
echo "  生效的钩子: pre-commit（格式检查）、commit-msg（规范校验）、pre-push（质量门禁）"
