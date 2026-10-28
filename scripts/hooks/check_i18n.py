#!/usr/bin/env python3
"""界面文案统一进文案表门禁：界面词条只定义在应用分包的文案目录里，
只经统一装入点装入，代码里不内嵌文案字面量，词条本身守语言规范。
本地提交前门禁与持续集成检查共用此脚本，保证本地与远端判定一致。

覆盖四条检查：
  文案字面量拦截  —— 各生产代码里，字符串、单字符与原始字符串字面量出现
                     非英文字节（ASCII）即违规；字符串字面量落在界面渲染
                     入口（显示调用点）上即便纯英文也违规——界面文案只许
                     出自文案表；测试模块、注释与属性内部（静态检查的
                     元信息）不在判罚面。模型面向文案的统一处理点（模板
                     资源加注入片段）整文件豁免；非文案字形与自检样张用
                     行尾放行标记豁免，显示调用点之外的纯英文硬编码
                     不可由文本判定，靠人工评审。
  装入点集中      —— 编译期引用文案资源只允许出现在统一装入点一处。
  词条表文件唯一  —— 声明词条前缀键的配置文件只允许在文案目录下。
  词条语言规范    —— 中文表词条必须纯中文，英文表词条必须纯英文；字符面
                     取语言规范库同源定义（拉丁面与全角面）。模板占位符、
                     地址方案符与快捷键符号是记号不是外文，不判罚；判罚
                     口径只锁最低必要规范：中文表是中文，英文表是英文，
                     无任何词条豁免。

行尾放行标记豁免所在行，须注明缘由、缺失即判罚。违规报告经统一诊断出口打印：每条带规
则标识、位置、证据行与修复指引，违规随执行流即时输出；字面量扫描面的四个
源码目录与文案目录两份词条表任一缺失按失败关闭退出。退出码按全脚本统一约定。

用法：python3 scripts/hooks/check_i18n.py <repo-root>，缺省为本脚本的上一级目录。
"""

from __future__ import annotations

import os
import re
import sys
from collections.abc import Iterator
from typing import Any

import gate_report
import rg_engine
from language_spec.chinese import CJKWISE, LATIN

RULE = "ui copy lives in the i18n catalog"
OWNER = "crates/gloss-app/src/ui/i18n.rs"
CATALOG_DIR = "crates/gloss-app/i18n"
CATALOG_FILES = ("zh.toml", "en.toml")
SCAN_DIRS = "gloss-app / root crate / gloss-core / gloss-platform src"
ALLOW_MARKER = "i18n:allow"

RULE_I18N_LITERAL = "I18N-LITERAL"
RULE_I18N_LOADPOINT = "I18N-LOADPOINT"
RULE_I18N_CATALOG = "I18N-CATALOG"
RULE_I18N_LANGUAGE = "I18N-LANGUAGE"

LITERAL_REMEDY = (
    "add the entry to the catalog (zh.toml and en.toml, key gloss_<module>_<item>)"
    " and read it via Text; not copy (icon glyphs, selftest strings, list bullets,"
    " etc.): add i18n:allow at the end of the line with a reason"
)
LOADPOINT_REMEDY = (
    "move the include into the single load point; read the copy via Text at the use site"
)
CATALOG_REMEDY = "move the entry table into %s/" % CATALOG_DIR

ZH_LANGUAGE_REMEDY = "rewrite the zh entry in Chinese, keeping placeholders and address schemes"
EN_LANGUAGE_REMEDY = "rewrite the en entry in English, keeping placeholders and symbol references"

LANGUAGE_MESSAGE_ZH = "the zh entry must be pure Chinese"
LANGUAGE_MESSAGE_EN = "the en entry must be pure English"

ENTRY_LINE_RE = re.compile(r'^\s*(gloss_[A-Za-z0-9_]+)\s*=\s*"(.*)"\s*$')

TEMPLATE_RE = re.compile(r"\{\{[^}]*\}\}")
ADDRESS_SCHEME_RE = re.compile(r"[A-Za-z][A-Za-z0-9+.-]*://")

DISPLAY_SITE_RE = re.compile(
    r'\b(?:ui\.label|ui\.button|RichText::new|selected_text|on_hover_text|hint_text)\(\s*"'
    r'|[^_\w]\.text\(\s*"'
)

SCAN_BASES = (
    os.path.join("crates", "gloss-app", "src"),
    "src",
    os.path.join("crates", "gloss-core", "src"),
    os.path.join("crates", "gloss-platform", "src"),
)

EXEMPT_FILE = "crates/gloss-core/src/prompt.rs"

EMPTY_SURFACE_MESSAGE = "the copy scan surface is incomplete: missing source directories: %s"

EMPTY_SURFACE_REMEDY = (
    "run the gate from the workspace root; a missing source directory means the"
    " literal scan cannot complete"
)

INCLUDE_EXPRESSION = r"include_(str|bytes)!\([^)]*i18n"

CATALOG_KEY_RE = re.compile(r"^\s*gloss_[A-Za-z0-9_]+\s*=")


def rs_files(base: str) -> Iterator[str]:
    if not os.path.isdir(base):
        return
    for dirpath, dirnames, filenames in os.walk(base):
        dirnames[:] = sorted(d for d in dirnames if d != "tests")
        for name in sorted(filenames):
            if name.endswith(".rs"):
                yield os.path.join(dirpath, name)


def skip_blanks(src: str, i: int) -> int:
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


def read_word(src: str, i: int) -> tuple[str, int]:
    j = i
    while j < len(src) and (src[j].isalnum() or src[j] == "_"):
        j += 1
    return src[i:j], j


def attribute_span(src: str, i: int) -> tuple[int, str]:
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


def test_mod_brace(src: str, i: int) -> int | None:
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


def literal_lines(src: str) -> tuple[set[int], set[int]]:
    """生产代码（注释、属性、#[cfg(test)] mod 项之外）里的字面量行号，返回
    （非 ASCII 字面量行号，字符串字面量行号）两个集合；前者是任意位置的
    判罚面，后者供显示调用点检查用——纯英文的界面文案只能靠调用点形状判定。"""
    hits = set()
    sites = set()
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
            if test_depth == 0 and not test_pending:
                if ord(c) > 127:
                    hits.add(line)
                sites.add(line)
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
    return hits, sites


def read_lines(path: str, loader: Any = None) -> list[str]:
    if loader is not None:
        return loader.read_text(path, errors="replace").split("\n")
    with open(path, encoding="utf-8", errors="replace") as fh:
        return fh.read().split("\n")


def guard_scan_dirs(root: str) -> None:
    """字面量扫描面守卫：四个源码目录任一缺失即失败关闭退出——目录缺席若静默
    跳过，对应扫描面悄然缩水，等于没检查到却放行。"""
    missing = [
        base.replace(os.sep, "/")
        for base in SCAN_BASES
        if not os.path.isdir(os.path.join(root, base))
    ]
    if not missing:
        return
    gate_report.fail(
        gate_report.Finding(
            rule=gate_report.RULE_EMPTY_SURFACE,
            path="",
            line=0,
            message=EMPTY_SURFACE_MESSAGE % ", ".join(missing),
            remedy=EMPTY_SURFACE_REMEDY,
        )
    )
    sys.exit(gate_report.EXIT_INFRA)


def literal_findings(root: str, loader: Any = None) -> list[gate_report.Finding]:
    """检查一：四个扫描面的生产代码字面量，逐行全量返回违规条目——非 ASCII
    字面量任意位置即拦，字符串字面量落在显示调用点（界面渲染入口）上即便
    纯英文也拦，界面文案只许出自文案表。"""
    out = []
    for base in SCAN_BASES:
        for path in rs_files(os.path.join(root, base)):
            rel = os.path.relpath(path, root).replace(os.sep, "/")
            if rel == EXEMPT_FILE:
                continue
            lines = read_lines(path, loader)
            hits, sites = literal_lines("\n".join(lines))
            for ln in sorted(hits | sites):
                if ln not in hits and not DISPLAY_SITE_RE.search(lines[ln - 1]):
                    continue
                line = lines[ln - 1]
                if ALLOW_MARKER in line:
                    if gate_report.allow_reason_missing(ALLOW_MARKER, line):
                        out.append(gate_report.allow_reason_finding(ALLOW_MARKER, rel, ln, line))
                    continue
                message = "embeds a copy literal" if ln in hits else "embeds a display literal"
                out.append(
                    gate_report.Finding(
                        rule=RULE_I18N_LITERAL,
                        path=rel,
                        line=ln,
                        message="%s line %d %s" % (rel, ln, message),
                        evidence=line,
                        remedy=LITERAL_REMEDY,
                    )
                )
    return out


def scan_all_rs(root: str) -> Iterator[str]:
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = sorted(d for d in dirnames if d not in ("target", ".git"))
        for name in sorted(filenames):
            if name.endswith(".rs"):
                yield os.path.join(dirpath, name)


def scan_all_toml(root: str) -> Iterator[str]:
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = sorted(d for d in dirnames if d not in ("target", ".git"))
        for name in sorted(filenames):
            if name.endswith(".toml"):
                yield os.path.join(dirpath, name)


def loadpoint_findings(root: str, loader: Any = None) -> tuple[int, list[gate_report.Finding]]:
    """检查二：装入点集中——文案文件是编译期资源，装入与解析只允许发生在统
    一装入点一处，别处编译期引入就绕开了类型化文案表与双语键一致性测试；
    内容检索委托共享引擎，逐行全量报告，返回扫描的 rs 文件数与违规条目。"""
    all_rs = [os.path.relpath(f, root).replace(os.sep, "/") for f in scan_all_rs(root)]
    targets = [f for f in all_rs if f != OWNER]
    try:
        hits = rg_engine.run(root, [INCLUDE_EXPRESSION], targets)
    except RuntimeError as exc:
        gate_report.fail(
            gate_report.infra_failure("cannot complete the i18n load point scan: %s" % exc)
        )
        sys.exit(gate_report.EXIT_INFRA)
    out = []
    for rel in targets:
        file_hits = hits.get(rel, {})
        for no in sorted(file_hits):
            out.append(
                gate_report.Finding(
                    rule=RULE_I18N_LOADPOINT,
                    path=rel,
                    line=no,
                    message="%s line %d references the i18n resource outside the load"
                    " point — only %s may include it" % (rel, no, OWNER),
                    evidence=file_hits[no],
                    remedy=LOADPOINT_REMEDY,
                )
            )
    return len(all_rs), out


def catalog_findings(root: str, loader: Any = None) -> tuple[int, list[gate_report.Finding]]:
    """检查三：词条表文件唯一——词条键统一为「模块加词条」的前缀形态且顶层扁
    平，出现第二张表就会分叉出不受键一致性测试管辖的文案面；逐行全量报告，
    返回扫描的 TOML 文件数与违规条目。"""
    out = []
    scanned = 0
    for f in scan_all_toml(root):
        scanned += 1
        rel = os.path.relpath(f, root).replace(os.sep, "/")
        if rel.startswith(CATALOG_DIR + "/"):
            continue
        for no, line in enumerate(read_lines(f, loader), 1):
            if CATALOG_KEY_RE.match(line):
                out.append(
                    gate_report.Finding(
                        rule=RULE_I18N_CATALOG,
                        path=rel,
                        line=no,
                        message="%s line %d declares an entry key outside %s/"
                        " — the catalog lives in one place only" % (rel, no, CATALOG_DIR),
                        evidence=line,
                        remedy=CATALOG_REMEDY,
                    )
                )
    return scanned, out


def guard_catalog_files(root: str) -> None:
    """词条表守卫：文案目录两份词条表任一缺失即失败关闭退出——词条表缺席
    若静默跳过，语言规范检查悄然落空，等于没检查到却放行。"""
    missing = [
        name for name in CATALOG_FILES if not os.path.isfile(os.path.join(root, CATALOG_DIR, name))
    ]
    if not missing:
        return
    gate_report.fail(
        gate_report.Finding(
            rule=gate_report.RULE_EMPTY_SURFACE,
            path="",
            line=0,
            message="the language scan surface is incomplete: missing catalog files: %s"
            % ", ".join(missing),
            remedy="restore %s/%s; a missing catalog file means the entry language"
            " scan cannot complete" % (CATALOG_DIR, ", ".join(missing)),
        )
    )
    sys.exit(gate_report.EXIT_INFRA)


def language_findings(root: str, loader: Any = None) -> tuple[int, list[gate_report.Finding]]:
    """检查四：词条语言规范——中文表词条必须纯中文（剔模板占位符与地址方
    案符两类记号后不得残留英文字母），英文表词条必须纯英文，无任何词条豁
    免；字符面取语言规范库同源定义，逐行全量报告，返回扫描的词条数与违
    规条目。"""
    out: list[gate_report.Finding] = []
    scanned = 0
    for name in CATALOG_FILES:
        path = os.path.join(root, CATALOG_DIR, name)
        rel = "%s/%s" % (CATALOG_DIR, name)
        zh = name.startswith("zh")
        for no, line in enumerate(read_lines(path, loader), 1):
            match = ENTRY_LINE_RE.match(line)
            if match is None:
                continue
            value = match.group(2)
            scanned += 1
            if zh:
                stripped = TEMPLATE_RE.sub("", value)
                stripped = ADDRESS_SCHEME_RE.sub("", stripped)
                if not LATIN.search(stripped):
                    continue
                out.append(
                    gate_report.Finding(
                        rule=RULE_I18N_LANGUAGE,
                        path=rel,
                        line=no,
                        message="%s line %d: %s" % (rel, no, LANGUAGE_MESSAGE_ZH),
                        evidence=value,
                        remedy=ZH_LANGUAGE_REMEDY,
                    )
                )
            elif CJKWISE.search(value):
                out.append(
                    gate_report.Finding(
                        rule=RULE_I18N_LANGUAGE,
                        path=rel,
                        line=no,
                        message="%s line %d: %s" % (rel, no, LANGUAGE_MESSAGE_EN),
                        evidence=value,
                        remedy=EN_LANGUAGE_REMEDY,
                    )
                )
    return scanned, out


def render(finding: gate_report.Finding) -> str:
    """违规渲染行：消息加位置，作为运行函数返回清单里的条目。"""
    loc = gate_report.location(finding)
    return "%s (%s)" % (finding.message, loc) if loc else finding.message


def collect(root: str, loader: Any = None) -> tuple[list[str], list[gate_report.Finding]]:
    """执行四条检查，违规随执行流即时经统一诊断出口打到标准错误，返回通过与
    违规条目两个清单；装入器在场时文件读取走共享缓存，缺省自行读取。"""
    guard_scan_dirs(root)
    found: list[gate_report.Finding] = []

    # ---- 检查一：文案字面量拦截 ----
    for finding in literal_findings(root, loader):
        found.append(finding)
        gate_report.fail(finding)

    # ---- 检查二：装入点集中 ----
    include_files, loadpoint = loadpoint_findings(root, loader)
    for finding in loadpoint:
        found.append(finding)
        gate_report.fail(finding)

    # ---- 检查三：词条表文件唯一 ----
    toml_files, catalog = catalog_findings(root, loader)
    for finding in catalog:
        found.append(finding)
        gate_report.fail(finding)

    # ---- 检查四：词条语言规范 ----
    guard_catalog_files(root)
    entries, language = language_findings(root, loader)
    for finding in language:
        found.append(finding)
        gate_report.fail(finding)

    oks = []
    if not found:
        oks.append(
            "%s (%s production literals + %d .rs include sites + %d TOML entry-key files"
            " + %d entries language-checked)"
            % (RULE, SCAN_DIRS, include_files, toml_files, entries)
        )
    return oks, found


def run(root: str, loader: Any = None) -> tuple[list[str], list[str]]:
    """执行四条检查，返回通过与失败两个清单；失败清单收渲染行（消息加位置）
    供调用方取用，装入器在场时文件读取走共享缓存，缺省自行读取。"""
    oks, found = collect(root, loader)
    return oks, [render(finding) for finding in found]


def usage_failure(detail: str) -> int:
    print("usage: check_i18n.py [repo-root]", file=sys.stderr)
    print("  %s" % detail, file=sys.stderr)
    return gate_report.EXIT_USAGE


def main(loader: Any = None) -> int:
    if len(sys.argv) > 2:
        return usage_failure("too many arguments: expected at most one repo-root")
    root = (
        os.path.abspath(sys.argv[1])
        if len(sys.argv) > 1
        else os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    )
    if len(sys.argv) > 1 and not os.path.isdir(root):
        return usage_failure("repo-root is not a directory: %s" % root)
    print("== i18n catalog gate (repo root: %s) ==" % root)
    oks, found = collect(root, loader)
    # 违规已在扫描时即时输出，这里只补汇总与结论
    for line in oks:
        print("  ok: %s" % line)
    if found:
        print("", file=sys.stderr)
        gate_report.summary(found)
        return gate_report.EXIT_VIOLATION
    return gate_report.EXIT_PASS


if __name__ == "__main__":
    sys.exit(main())
