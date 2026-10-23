#!/usr/bin/env python3
# 安装本地 git hooks：把 core.hooksPath 指向 .githooks 目录，并给脚本加执行权限。
# 团队成员 clone 后只需运行一次：./scripts/hooks/install_hooks.py

import os
import stat
import subprocess
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))

EXEC_FILES = [
    ".githooks/pre-commit",
    ".githooks/commit-msg",
    "scripts/hooks/check_commit_msg.py",
    "scripts/hooks/check_secrets.py",
    "scripts/hooks/check_agents_doc.py",
    "scripts/hooks/check_constraints.py",
    "scripts/hooks/check_test_bdd.py",
    "scripts/hooks/check_i18n.py",
    "scripts/release/check_release_tag.py",
    "scripts/release/gen_manifest.py",
    "scripts/release/bundle_dmg.py",
    "scripts/release/smoke_app.py",
    "scripts/dev/setup_dev.py",
    "scripts/perf/bisect_measure.py",
]


def main():
    # 1. 给 hook 脚本加执行权限（一并覆盖 checker、发布脚本与二分测量驱动）
    for rel in EXEC_FILES:
        path = os.path.join(ROOT, rel)
        if not os.path.isfile(path):
            print("✗ 缺少脚本 %s（仓库不完整？）" % rel, file=sys.stderr)
            return 1
        os.chmod(path, os.stat(path).st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)

    # 2. 设置 hooksPath 指向 .githooks
    subprocess.run(["git", "-C", ROOT, "config", "core.hooksPath", ".githooks"], check=True)

    print("✓ git hooks 已启用")
    print(
        "  hooksPath: %s"
        % subprocess.run(
            ["git", "-C", ROOT, "config", "core.hooksPath"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()
    )
    print(
        "  生效的钩子: pre-commit（约束检查 + 文档引用校验 + BDD 清单 + 文案表 "
        "+ fmt + clippy + 密钥扫描，经 just precommit）"
    )
    print(
        "             commit-msg（Conventional Commits 校验，与 CI 的 Commit message check 同款）"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
