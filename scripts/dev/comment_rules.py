#!/usr/bin/env python3
"""注释纪律规则速查：按类别打印判罚脚本的文件头（模块级文档字符串）与脚本
路径。类别到脚本的对应只此一份，注释纪律一节只写类别名、不写路径；放行标
记另附常量现值。用法：python3 scripts/dev/comment_rules.py，可带类别名只读
给出的类别，类别名见不带参数时打印的索引。脚本缺失、文件头缺失或不可解析
均属无法完成读取，失败关闭退出。
"""

from __future__ import annotations

import ast
import os
import sys
from typing import NamedTuple

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
HOOKS = os.path.join(ROOT, "scripts", "hooks")


class Category(NamedTuple):
    slug: str
    title: str
    paths: tuple[str, ...]
    markers: bool = False


CATEGORIES = (
    Category("route", "受检面", ("comments/check_comments.py",)),
    Category("rust", "源码", ("comments/check_rust_comments.py",)),
    Category("script", "脚本与配置", ("comments/check_script_comments.py",)),
    Category("docs", "标记语言文档", ("comments/check_docs.py",)),
    Category(
        "shared",
        "共用判据",
        (
            "language_spec/chinese.py",
            "language_spec/english.py",
            "language_spec/coderef.py",
        ),
    ),
    Category("allow", "放行标记", ("gate_report.py",), markers=True),
)


def header(path: str) -> str:
    with open(path, encoding="utf-8") as fh:
        tree = ast.parse(fh.read(), filename=path)
    return (ast.get_docstring(tree, clean=False) or "").strip()


def allow_markers(path: str) -> list[str]:
    with open(path, encoding="utf-8") as fh:
        tree = ast.parse(fh.read(), filename=path)
    for node in tree.body:
        if not isinstance(node, ast.Assign):
            continue
        if any(getattr(target, "id", "") == "ALLOW_REASON_MARKERS" for target in node.targets):
            return [str(item) for item in ast.literal_eval(node.value)]
    return []


def matches(category: Category, name: str) -> bool:
    return name in (category.slug, category.title)


def print_index() -> None:
    print("类别与脚本：")
    for category in CATEGORIES:
        for rel in category.paths:
            print("%s  %s" % (category.title, os.path.join("scripts", "hooks", rel)))
    print()


def main(argv: list[str]) -> int:
    unknown = [name for name in argv if not any(matches(c, name) for c in CATEGORIES)]
    if unknown:
        print("unknown category: %s" % ", ".join(unknown), file=sys.stderr)
        print("known categories: %s" % " ".join(c.title for c in CATEGORIES), file=sys.stderr)
        return 2
    picked = [c for c in CATEGORIES if not argv or any(matches(c, name) for name in argv)]
    if not argv:
        print_index()
    unreadable = []
    for category in picked:
        for rel in category.paths:
            path = os.path.join(HOOKS, rel)
            shown = os.path.relpath(path, ROOT)
            try:
                text = header(path)
            except (OSError, SyntaxError) as exc:
                unreadable.append("%s: %s" % (shown, exc))
                continue
            if not text:
                unreadable.append("%s: empty file header" % shown)
                continue
            print("== %s · %s ==" % (category.title, shown))
            print(text)
            print()
            markers = allow_markers(path) if category.markers else []
            if markers:
                print("ALLOW_REASON_MARKERS:")
                print("\n".join(markers))
                print()
    if unreadable:
        for item in unreadable:
            print("cannot read rules: %s" % item, file=sys.stderr)
        return 3
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
