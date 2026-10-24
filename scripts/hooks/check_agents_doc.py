#!/usr/bin/env python3
# 指令文档引用一致性门禁：校验指令文档里提到的仓库事实确实存在。
# 本地提交前门禁与持续集成检查共用此脚本，保证本地与远端判定一致。
#
# 校验对象为仓库的指令文档，五类引用：
#   1. 全文任何任务配方的提及必须在配方清单里定义；
#   2. 行内反引号里的仓库路径（含分隔符）必须存在（文件或目录）；
#   3. 行内反引号里的裸文件名（配方清单或带常见扩展名）必须在仓库根存在；
#   4. 全文任何测试目标的提及必须存在对应的测试源码（根包或任一分包）；
#   5. 仓库各处对质量条目的指名引用必须真的在指令文档的「质量条目总表」段里。
#
# 扫描逐行进行，代码块内同样算数；路径只认行内反引号包裹的仓库相对路径。
# 占位符、仓库外路径、注释语法与链接不校验。
#
# 用法：以解释器运行本脚本，首参为指令文档路径，缺省为仓库根的指令文档。

import os
import re
import sys

RECIPE_RE = re.compile(r"(?:^|[^A-Za-z0-9_-])just ([a-z][a-z0-9-]*)")
BACKTICK_RE = re.compile(r"`([^`]+)`")
CARGO_TEST_RE = re.compile(r"--test ([A-Za-z0-9_-]+)")
RULE_NAME_RE = re.compile(r"\*\*([^*]+)\*\*")
REF_LINE_RE = re.compile(r"AGENTS\.md.*质量条目「")
REF_NAME_RE = re.compile(r"质量条目「([^」]+)」")
BARE_SUFFIXES = (".md", ".toml", ".py", ".rs", ".yml", ".json")

# 本脚本与其单测按构造就含质量条目引用模式，跳过自命中
SELF_EXEMPT = {"check_agents_doc.py", "check_agents_doc_test.py"}


def scan_lines(doc_path):
    with open(doc_path, encoding="utf-8") as fh:
        return fh.read().split("\n")


def rule_names(lines):
    """「质量条目总表」段里以 **名字** 起头的条目名。"""
    names = []
    in_section = False
    for line in lines:
        if line.startswith("### 质量条目总表"):
            in_section = True
            continue
        if in_section and line.startswith("## "):
            break
        if in_section:
            names.extend(RULE_NAME_RE.findall(line))
    return names


def scan_targets(root, target):
    candidates = [os.path.join(root, "tests", "%s.rs" % target)]
    crates = os.path.join(root, "crates")
    if os.path.isdir(crates):
        for crate in sorted(os.listdir(crates)):
            candidates.append(os.path.join(crates, crate, "tests", "%s.rs" % target))
    return any(os.path.isfile(c) for c in candidates)


def scan_quality_refs(root, names):
    """仓库各处（*.rs / *.toml / *.py / *.yml / justfile）对质量条目的指名引用核对。

    返回 (violations, ref_files, ref_count)。"""
    violations = []
    ref_files = 0
    ref_count = 0
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = sorted(d for d in dirnames if d not in ("target", ".git"))
        for name in sorted(filenames):
            if not (name.endswith((".rs", ".toml", ".py", ".yml")) or name == "justfile"):
                continue
            if name in SELF_EXEMPT:
                continue
            ref_files += 1
            path = os.path.join(dirpath, name)
            rel = os.path.relpath(path, root)
            with open(path, encoding="utf-8", errors="replace") as fh:
                for line in fh:
                    if not REF_LINE_RE.search(line):
                        continue
                    for cited in REF_NAME_RE.findall(line):
                        ref_count += 1
                        if cited not in names:
                            violations.append(
                                "%s: 引用了质量条目「%s」，但 AGENTS.md 的"
                                "质量条目总表里没有这个条目名" % (rel, cited)
                            )
    return violations, ref_files, ref_count


def run(doc_path):
    """按五类引用核对 doc_path，返回 (violations, checked, ref_files, ref_count)。"""
    root = os.path.dirname(os.path.abspath(doc_path))
    doc_name = os.path.basename(doc_path)
    lines = scan_lines(doc_path)
    justfile_lines = None
    justfile_path = os.path.join(root, "justfile")
    if os.path.isfile(justfile_path):
        with open(justfile_path, encoding="utf-8") as fh:
            justfile_lines = fh.read().split("\n")

    violations = []
    checked = 0

    for line_no, line in enumerate(lines, 1):
        # ---- 1. 任务配方 ----
        for recipe in RECIPE_RE.findall(line):
            checked += 1
            defined = False
            for jl in justfile_lines or []:
                if re.match(r"^%s(\s[^:]+)?:(\s|$)" % recipe, jl):
                    defined = True
                    break
            if not defined:
                violations.append(
                    "%s:%d  「just %s」在 justfile 中找不到同名配方" % (doc_name, line_no, recipe)
                )

        # ---- 2/3. 行内反引号里的仓库路径与裸文件名 ----
        for token in BACKTICK_RE.findall(line):
            if "*" in token or "<" in token or ">" in token or "//" in token:
                continue
            if token.startswith("~"):
                continue
            if "/" in token:
                checked += 1
                if not os.path.exists(os.path.join(root, token)):
                    violations.append("%s:%d  「%s」在仓库里不存在" % (doc_name, line_no, token))
            elif token == "justfile" or token.endswith(BARE_SUFFIXES):
                checked += 1
                if not os.path.exists(os.path.join(root, token)):
                    violations.append("%s:%d  「%s」在仓库根不存在" % (doc_name, line_no, token))

        # ---- 4. 测试目标 ----
        for target in CARGO_TEST_RE.findall(line):
            checked += 1
            if not scan_targets(root, target):
                violations.append(
                    "%s:%d  「cargo test --test %s」找不到 tests/%s.rs"
                    % (doc_name, line_no, target, target)
                )

    # ---- 5. 仓库各处对质量条目的指名引用必须存在 ----
    ref_violations, ref_files, ref_count = scan_quality_refs(root, rule_names(lines))
    violations.extend(ref_violations)
    return violations, checked, ref_files, ref_count


def main():
    if len(sys.argv) >= 2:
        arg = os.path.abspath(sys.argv[1])
    else:
        arg = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "AGENTS.md")
    if not os.path.isfile(arg):
        print("错误：找不到指令文件 %s" % arg, file=sys.stderr)
        print("  客户端只认 AGENTS.md（没有 AGENT.md 之类的别名），别改名或挪位。", file=sys.stderr)
        return 1
    violations, checked, ref_files, ref_count = run(arg)
    doc_name = os.path.basename(arg)
    if violations:
        for v in violations:
            print("  ✗ %s" % v, file=sys.stderr)
        print("", file=sys.stderr)
        print(
            "错误：%s 有 %d 处引用与其来源不符（见上）。" % (doc_name, len(violations)),
            file=sys.stderr,
        )
        print("  - 命令或路径确实变了：改文档，别让它描述一个不存在的世界；", file=sys.stderr)
        print("  - 只是举例或占位：换个措辞，或写成含 * < > ~ 的形式跳过校验；", file=sys.stderr)
        print(
            "  - 条目改了名字：把引用它的地方一起改（用 质量条目「名字」 指名）。", file=sys.stderr
        )
        return 1
    print(
        "✓ %s 引用一致（校验 %d 处：just 配方 / 仓库路径 / 测试目标；"
        "另在 %d 个文件里核对 %d 处质量条目名引用）" % (doc_name, checked, ref_files, ref_count)
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
