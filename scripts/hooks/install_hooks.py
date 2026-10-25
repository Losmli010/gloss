#!/usr/bin/env python3
"""安装本地版本库钩子：把钩子目录配置指向仓内的钩子目录，并给脚本加执行权限。
团队成员取得仓库副本后只需运行一次本脚本。
"""

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
    # 一、给钩子脚本加执行权限（一并覆盖各检查脚本、发布脚本与二分测量驱动）
    for rel in EXEC_FILES:
        path = os.path.join(ROOT, rel)
        if not os.path.isfile(path):
            print("✗ missing script %s (incomplete checkout?)" % rel, file=sys.stderr)
            return 1
        os.chmod(path, os.stat(path).st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)

    # 二、把钩子目录配置指向仓内的钩子目录
    subprocess.run(["git", "-C", ROOT, "config", "core.hooksPath", ".githooks"], check=True)

    print("✓ git hooks enabled")
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
        "  active hooks: pre-commit (constraints + doc references + BDD inventory"
        " + i18n catalog + fmt + clippy + secrets scan, via just precommit)"
    )
    print(
        "             commit-msg (Conventional Commits check, same as the CI commit message check)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
