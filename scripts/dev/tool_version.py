#!/usr/bin/env python3
"""按工具名打印钉版本的小助手：版本单点住仓库根版本清单，持续集成安装与环境
初始化校验运行时同读该清单取值，消掉版本散抄。只服务钉全版本的条目——记下限
与显式浮动的条目没有可打印的全版本，按非零退出拒绝。
"""

from __future__ import annotations

import os
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
TOOLS_PATH = os.path.join(ROOT, "tools.toml")


def parse_sections(path: str) -> dict[str, dict[str, str]]:
    """把清单文件解析成「段名到键值表」的映射；只认段头与单行赋值两种结构。"""
    sections = {}
    current = None
    with open(path, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            stripped = line.strip()
            if not stripped or stripped.startswith("#"):
                continue
            if stripped.startswith("[") and stripped.endswith("]"):
                current = stripped[1:-1].strip()
                sections.setdefault(current, {})
                continue
            if current and "=" in stripped:
                key, _, value = stripped.partition("=")
                sections[current][key.strip()] = value.strip().strip('"')
    return sections


def pinned_version(name: str, path: str = TOOLS_PATH) -> str | None:
    """返回某工具钉的全版本；清单里没有该工具或未钉全版本时返回空值。"""
    entry = parse_sections(path).get(name)
    if not entry:
        return None
    return entry.get("version")


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: %s <tool>" % argv[0], file=sys.stderr)
        return 2
    version = pinned_version(argv[1])
    if version is None:
        print("error: no pinned version for tool %r in the tool list" % argv[1], file=sys.stderr)
        return 1
    print(version)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
