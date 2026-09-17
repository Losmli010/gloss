#!/usr/bin/env bash
# 安装本地 git hooks：把 core.hooksPath 指向 .githooks 目录，并给脚本加执行权限。
# 团队成员 clone 后只需运行一次：./scripts/install-hooks.sh
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

# 1. 给 hook 脚本加执行权限（一并覆盖 checker 的配套脚本与发布脚本）
chmod +x .githooks/pre-commit
chmod +x scripts/check-commit-msg.sh scripts/check-commit-msg.test.sh
chmod +x scripts/check-secrets.sh scripts/check-secrets.test.sh
chmod +x scripts/check-agents-doc.sh scripts/check-agents-doc.test.sh
chmod +x scripts/check-constraints.sh scripts/check-constraints.test.sh
chmod +x scripts/check-release-tag.sh scripts/check-release-tag.test.sh
chmod +x scripts/bundle-dmg.sh scripts/smoke-app.sh scripts/setup-dev.sh

# 2. 设置 hooksPath 指向 .githooks
git config core.hooksPath .githooks

echo "✓ git hooks 已启用"
echo "  hooksPath: $(git config core.hooksPath)"
echo "  生效的钩子: pre-commit（约束检查 + 文档引用校验 + fmt + clippy + 密钥扫描，经 just precommit）"
