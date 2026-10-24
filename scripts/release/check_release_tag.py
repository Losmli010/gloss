#!/usr/bin/env python3
# 校验发布标签与版本单点一致：标签必须带版本前缀加三段数字，去掉前缀后等于
# 根清单工作区段的版本。防止标签与产物版本错位地发出去。
# 本地打标签前手动跑（走校验配方）；发布工作流构建产物前跑同一条脚本。

import os
import re
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))


def workspace_version(root):
    """版本只认 [workspace.package] 段里的 version 赋值（约束「版本单点维护」）；
    继承行 version.workspace 与依赖段的 version 都不误读。"""
    path = os.path.join(root, "Cargo.toml")
    if not os.path.isfile(path):
        return None
    in_section = False
    with open(path, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            stripped = line.strip()
            if stripped.startswith("["):
                in_section = stripped == "[workspace.package]"
                continue
            if in_section and stripped.split()[:1] == ["version"]:
                return stripped.split("=", 1)[1].strip().strip('"')
    return None


def run(tag, root):
    """校验 tag 与 root 的版本单点一致，返回 (tag 版本, cargo 版本, 错误串或 None)。"""
    if not tag.startswith("v"):
        return None, None, '错误：tag "%s" 缺 v 前缀（例：v0.1.0）' % tag
    # 标签格式收紧为前缀加恰好三段纯数字（挡住段数不对、带通配、漏前缀）。
    version = tag[1:]
    parts = version.split(".")
    if len(parts) != 3 or not all(re.fullmatch(r"[0-9]+", p) for p in parts):
        return None, None, '错误：tag "%s" 不符合 v<主>.<次>.<补丁> 格式（例：v0.1.0）' % tag

    cargo_version = workspace_version(root)
    if not cargo_version:
        return (
            None,
            None,
            (
                "错误：没能从 Cargo.toml 的 [workspace.package] 段读到 version\n"
                "  版本单点被挪走了？先确认 [workspace.package] 的 version 还在。"
            ),
        )
    if version != cargo_version:
        return (
            version,
            cargo_version,
            (
                "错误：版本号不一致：tag=%s，Cargo.toml=%s\n"
                "  把根 Cargo.toml [workspace.package] 的 version 改成 %s 后重新打 tag。"
                % (version, cargo_version, version)
            ),
        )
    return version, cargo_version, None


def main():
    if len(sys.argv) != 2:
        print("用法: %s <tag>（例：v0.1.0）" % sys.argv[0], file=sys.stderr)
        return 2
    version, cargo_version, error = run(sys.argv[1], ROOT)
    if error:
        print(error, file=sys.stderr)
        return 1
    print("tag version:   %s" % version)
    print("cargo version: %s" % cargo_version)
    print("✓ 版本号一致")
    return 0


if __name__ == "__main__":
    sys.exit(main())
