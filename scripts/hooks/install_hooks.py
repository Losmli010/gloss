#!/usr/bin/env python3
"""安装本地版本库钩子：把钩子目录配置指向仓内的钩子目录，并给仍按路径直接
调用的脚本加执行权限。
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
    "scripts/release/check_release_tag.py",
    "scripts/release/gen_manifest.py",
    "scripts/release/bundle_dmg.py",
    "scripts/release/smoke_app.py",
    "scripts/perf/app-metrics.py",
    "scripts/perf/flamegraph.py",
    "scripts/perf/perf-collect.py",
    "scripts/perf/perf-chart.py",
    "scripts/perf/perf-report.py",
]


def main():
    for rel in EXEC_FILES:
        path = os.path.join(ROOT, rel)
        if not os.path.isfile(path):
            print("✗ missing script %s (incomplete checkout?)" % rel, file=sys.stderr)
            return 1
        os.chmod(path, os.stat(path).st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)

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
        "  active hooks: pre-commit (derived from staged changes: just precommit"
        " on compile-affecting changes, just text-gates otherwise)"
    )
    print(
        "             commit-msg (Conventional Commits check, same as the CI commit message check)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
