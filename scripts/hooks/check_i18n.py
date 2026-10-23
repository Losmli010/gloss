#!/usr/bin/env python3
# 界面文案统一进文案表门禁：界面词条只定义在 crates/gloss-app/i18n/*.toml，
# 只经 crates/gloss-app/src/ui/i18n.rs 装入，代码里不内嵌文案字面量。
# 本地 `just i18n`（pre-commit 的一部分）与 CI 的 I18n check job 共用此脚本，
# 保证本地与 CI 判定一致。
#
# 覆盖三条检查：
#   文案字面量拦截  —— gloss-app / 根包 / gloss-core / gloss-platform 的生产代码里，
#                      字符串 / char / 原始字符串字面量出现非 ASCII 字节即违规；
#                      #[cfg(test)] 模块、注释、#[...] 属性内部（clippy reason 等
#                      元信息）不在判罚面。crates/gloss-core/src/prompt.rs 是模型
#                      面向文案的统一处理点（prompts/ 模板资源 + 注入片段），整文件
#                      豁免。纯 ASCII 的英文硬编码不可由文本判定，靠人工评审。
#   装入点集中      —— include_str! / include_bytes! 引用 i18n 资源只允许出现在
#                      crates/gloss-app/src/ui/i18n.rs。
#   词条表文件唯一  —— 声明 gloss_ 前缀键的 TOML 只允许在 crates/gloss-app/i18n/ 下。
#
# 行尾 `i18n:allow` 放行所在行（同 secrets:allow 惯例），须注明缘由。
#
# 用法：scripts/hooks/check_i18n.py [仓库根]   # 缺省为本脚本的上一级目录

import os
import re
import sys

RULE = "界面文案统一进文案表"
OWNER = "crates/gloss-app/src/ui/i18n.rs"
CATALOG_DIR = "crates/gloss-app/i18n"
SCAN_DIRS = "gloss-app / 根包 / gloss-core / gloss-platform 的 src"
ALLOW_MARKER = "i18n:allow"


def rs_files(base):
    if not os.path.isdir(base):
        return
    for dirpath, dirnames, filenames in os.walk(base):
        dirnames[:] = sorted(d for d in dirnames if d != "tests")
        for name in sorted(filenames):
            if name.endswith(".rs"):
                yield os.path.join(dirpath, name)


def skip_blanks(src, i):
    n = len(src)
    while i < n:
        c = src[i]
        if c in " \t\r\n":
            i += 1
        elif c == "/" and i + 1 < n and src[i + 1] == "/":
            while i < n and src[i] != "\n":
                i += 1
        elif c == "/" and i + 1 < n and src[i + 1] == "*":
            depth = 1
            i += 2
            while i < n and depth:
                if src.startswith("/*", i):
                    depth += 1
                    i += 2
                elif src.startswith("*/", i):
                    depth -= 1
                    i += 2
                else:
                    i += 1
        else:
            break
    return i


def read_word(src, i):
    j = i
    while j < len(src) and (src[j].isalnum() or src[j] == "_"):
        j += 1
    return src[i:j], j


def attribute_span(src, i):
    """i 指向 '#'。是属性（#[..] 或 #![..]）时返回 (结束下标, 方括号内文本)，
    否则返回 (i, "")。方括号内的字符串 / char / 原始字符串按字面量整体跳过，
    其中的 ']' 不参与配对。"""
    n = len(src)
    j = i + 1
    if j < n and src[j] == "!":
        j += 1
    if j >= n or src[j] != "[":
        return i, ""
    depth = 1
    j += 1
    inner_start = j
    while j < n and depth:
        c = src[j]
        nxt = src[j + 1] if j + 1 < n else ""
        if c == '"':
            j += 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            j += 1
            continue
        if c == "r" and nxt in ('"', "#"):
            k = j + 1
            h = 0
            while k < n and src[k] == "#":
                h += 1
                k += 1
            if k < n and src[k] == '"':
                j = k + 1
                while j < n:
                    if src[j] == '"':
                        m = 0
                        while m < h and j + 1 + m < n and src[j + 1 + m] == "#":
                            m += 1
                        if m == h:
                            j += 1 + h
                            break
                    j += 1
                continue
        if c == "'":
            if nxt == "\\":
                j += 2
                while j < n and src[j] != "'":
                    j += 2 if src[j] == "\\" else 1
                j += 1
                continue
            if nxt and nxt != "'" and j + 2 < n and src[j + 2] == "'":
                j += 3
                continue
            j += 1
            continue
        if c == "[":
            depth += 1
        elif c == "]":
            depth -= 1
            if depth == 0:
                return j + 1, src[inner_start:j]
        j += 1
    return n, src[inner_start:n]


def test_mod_brace(src, i):
    """#[cfg(test)] 属性结束后，若紧跟一个带 body 的 mod 项（可带其他属性与
    pub 修饰），返回其 body '{' 的下标；否则返回 None（无 body 的 mod 声明、
    其他项均不算）。"""
    while True:
        i = skip_blanks(src, i)
        if i < len(src) and src[i] == "#":
            end, _ = attribute_span(src, i)
            if end == i:
                break
            i = end
            continue
        word, j = read_word(src, i)
        if word == "pub":
            i = j
            i = skip_blanks(src, i)
            if i < len(src) and src[i] == "(":
                depth = 1
                i += 1
                while i < len(src) and depth:
                    if src[i] == "(":
                        depth += 1
                    elif src[i] == ")":
                        depth -= 1
                    i += 1
            continue
        if word == "mod":
            i = j
            i = skip_blanks(src, i)
            _, j2 = read_word(src, i)
            if j2 == i:
                return None
            i = skip_blanks(src, j2)
            if i < len(src) and src[i] == "{":
                return i
            return None
        return None


def literal_lines(src):
    """生产代码（注释、属性、#[cfg(test)] mod 项之外）里含非 ASCII 字面量的行号。"""
    hits = set()
    i, n, line = 0, len(src), 1
    state = "code"
    hashes = 0
    block_depth = 0
    test_depth = 0
    test_pending = False
    while i < n:
        c = src[i]
        nxt = src[i + 1] if i + 1 < n else ""
        if c == "\n":
            line += 1
        if state == "line":
            if c == "\n":
                state = "code"
            i += 1
            continue
        if state == "block":
            if c == "/" and nxt == "*":
                block_depth += 1
                i += 2
                continue
            if c == "*" and nxt == "/":
                block_depth -= 1
                i += 2
                if block_depth == 0:
                    state = "code"
                continue
            i += 1
            continue
        if state in ("str", "raw"):
            if state == "str":
                if c == "\\":
                    # 续行符（反斜杠紧跟换行）：换行被转义吞掉，行计数要手动补上，
                    # 否则后续命中整段失步、i18n:allow 按行放行失效
                    if i + 1 < n and src[i + 1] == "\n":
                        line += 1
                    i += 2
                    continue
                if c == '"':
                    state = "code"
                    i += 1
                    continue
            elif c == '"':
                k = 0
                while k < hashes and i + 1 + k < n and src[i + 1 + k] == "#":
                    k += 1
                if k == hashes:
                    state = "code"
                    i += 1 + hashes
                    continue
            if ord(c) > 127 and test_depth == 0 and not test_pending:
                hits.add(line)
            i += 1
            continue
        if state == "char":
            if c == "\\":
                i += 2
                continue
            if c == "'":
                state = "code"
            elif ord(c) > 127 and test_depth == 0 and not test_pending:
                hits.add(line)
            i += 1
            continue
        if c == "/" and nxt == "/":
            state = "line"
            i += 2
            continue
        if c == "/" and nxt == "*":
            state = "block"
            block_depth = 1
            i += 2
            continue
        if c == "#":
            end, inner = attribute_span(src, i)
            if end > i:
                if inner.replace(" ", "").replace("\t", "") == "cfg(test)":
                    brace = test_mod_brace(src, end)
                    if brace is not None:
                        test_pending = True
                        line += src.count("\n", i, brace)
                        i = brace
                        continue
                line += src.count("\n", i, end)
                i = end
                continue
            i += 1
            continue
        if c == "r":
            j = i + 1
            h = 0
            while j < n and src[j] == "#":
                h += 1
                j += 1
            if j < n and src[j] == '"':
                state = "raw"
                hashes = h
                i = j + 1
                continue
            i += 1
            continue
        if c == '"':
            state = "str"
            i += 1
            continue
        if c == "'":
            if nxt == "\\":
                state = "char"
                i += 1
                continue
            if nxt and nxt != "'" and i + 2 < n and src[i + 2] == "'":
                state = "char"
                i += 1
                continue
            i += 1
            continue
        if test_pending:
            test_pending = False
            if c == "{":
                test_depth += 1
                i += 1
                continue
        if test_depth > 0:
            if c == "{":
                test_depth += 1
            elif c == "}":
                test_depth -= 1
        i += 1
    return hits


def literal_violations(root):
    """检查一：四个扫描面的生产代码字面量，返回违规描述列表。"""
    problems = []
    for base in (
        os.path.join(root, "crates", "gloss-app", "src"),
        os.path.join(root, "src"),
        os.path.join(root, "crates", "gloss-core", "src"),
        os.path.join(root, "crates", "gloss-platform", "src"),
    ):
        for path in rs_files(base):
            rel = os.path.relpath(path, root).replace(os.sep, "/")
            if rel == "crates/gloss-core/src/prompt.rs":
                continue
            with open(path, encoding="utf-8", errors="replace") as fh:
                src = fh.read()
            lines = src.split("\n")
            for ln in sorted(literal_lines(src)):
                if ALLOW_MARKER in lines[ln - 1]:
                    continue
                problems.append(
                    "约束「%s」：%s 第 %d 行内嵌文案字面量 —— 词条进 %s/，经 %s 的 Text 取用"
                    % (RULE, rel, ln, CATALOG_DIR, OWNER)
                )
    return problems


def scan_all_rs(root):
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = sorted(d for d in dirnames if d not in ("target", ".git"))
        for name in sorted(filenames):
            if name.endswith(".rs"):
                yield os.path.join(dirpath, name)


def scan_all_toml(root):
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = sorted(d for d in dirnames if d not in ("target", ".git"))
        for name in sorted(filenames):
            if name.endswith(".toml"):
                yield os.path.join(dirpath, name)


def run(root):
    """执行三条检查，返回 (oks, fails)。"""
    fails = literal_violations(root)

    # ---- 检查二：装入点集中 ----
    # 文案文件是编译期资源，装入与解析只允许发生在 i18n.rs 一处；
    # 别处 include 进来就绕开了类型化 Text 表与 zh/en 键一致性测试。
    include_files = 0
    include_re = re.compile(r"include_(str|bytes)!\([^)]*i18n")
    for f in scan_all_rs(root):
        include_files += 1
        if os.path.relpath(f, root).replace(os.sep, "/") == OWNER:
            continue
        with open(f, encoding="utf-8", errors="replace") as fh:
            for line in fh:
                if include_re.search(line):
                    fails.append(
                        "约束「%s」：%s 在装入点之外引用了 i18n 资源 —— 只允许 %s include"
                        % (RULE, os.path.relpath(f, root), OWNER)
                    )
                    break

    # ---- 检查三：词条表文件唯一 ----
    # 词条键统一 gloss_<模块>_<词条> 且顶层扁平；出现第二张表就会分叉出
    # 不受键一致性测试管辖的文案面。
    toml_files = 0
    toml_key_re = re.compile(r"^\s*gloss_[A-Za-z0-9_]+\s*=")
    for f in scan_all_toml(root):
        toml_files += 1
        if os.path.relpath(f, root).replace(os.sep, "/").startswith(CATALOG_DIR + "/"):
            continue
        with open(f, encoding="utf-8", errors="replace") as fh:
            for line in fh:
                if toml_key_re.match(line):
                    fails.append(
                        "约束「%s」：%s 在 %s/ 之外声明了词条键 —— 文案表只此一处"
                        % (RULE, os.path.relpath(f, root), CATALOG_DIR)
                    )
                    break

    oks = []
    if not fails:
        oks.append(
            "%s（%s 的生产代码字面量 + %d 个 .rs 的装入点 + %d 个 TOML 的词条键）"
            % (RULE, SCAN_DIRS, include_files, toml_files)
        )
    return oks, fails


def main():
    root = (
        os.path.abspath(sys.argv[1])
        if len(sys.argv) > 1
        else os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    )
    print("== i18n 文案表门禁（仓库根：%s）==" % root)
    oks, fails = run(root)
    for line in oks:
        print("  ✓ %s" % line)
    if fails:
        for line in fails:
            print("  ✗ %s" % line, file=sys.stderr)
        print("", file=sys.stderr)
        print("错误：有 %d 处违反「%s」（见上）。" % (len(fails), RULE), file=sys.stderr)
        print(
            "  - 是界面文案：词条加进 %s/zh.toml 与 en.toml（键名 gloss_<模块>_<词条>），"
            "%s 的 Text 补同名字段，代码经 Text 取用；" % (CATALOG_DIR, OWNER),
            file=sys.stderr,
        )
        print(
            "  - 非文案（图标字形、自检字样、列表符号等）：所在行尾加 %s 并注明缘由。"
            % ALLOW_MARKER,
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
