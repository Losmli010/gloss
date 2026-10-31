"""约束门禁·语言规范：字符事实单源——语言判罚实现不私持语言字符字面量。

语言规范数据源（crates/gloss-core/langspec/zh_en.toml）是字符事实的唯一出
处，判罚实现里出现数据源字符面的成员即判罚——私有字符事实会绕过单源，复
刻同字异判的漂移。判罚面是两侧的判罚实现：生产侧判罚树（lang_gate.rs）
（lang_gate/），内联测试模块不计——夹具是样本文本不是字符事实；开发侧语
言规范库的判罚模块（english）（coderef）与包初始化脚本，（chinese）是开
发侧派生模块按派生处豁免，测试文件随测试目标豁免。判罚模式自数据源动态
构建，取各字符面的非（ASCII）成员；变体选择符不可见，不作判罚对象。正则
组合与结构记号豁免面按计划仍住代码，行尾放行标记（langspec:allow）加缘由
后豁免；数据源缺失或损坏按失败关闭退出。字面量提取复用文案门禁的本语言
（Rust）状态机与脚本语法树的常量收集。违规经统一诊断出口带规则标识报告，
证据为命中行不截断。判定口径与取舍见记录文件。
"""

from __future__ import annotations

import ast
import os
import sys
from typing import Any

import check_i18n
import gate_report
import tomllib

from . import manifests

RULE_LANGSPEC_LITERAL = "LANGSPEC-LITERAL"

LANGSPEC_LITERAL_REMEDY = (
    "derive the face from the langspec data source (crates/gloss-core/langspec/)"
    " instead of holding a private copy, or add langspec:allow with a reason for"
    " structural marks and message typography"
)

DATA_SOURCE = os.path.join("crates", "gloss-core", "langspec", "zh_en.toml")

ALLOW_MARKER = "langspec:allow"

# 判罚模式读的数据面：场景标点两份、中文场景附随面两份加共用面九份；西文
# 标点与数字夹住面不含非英文字符，变体选择符经下方不可见选择符剔除，三面
# 对本模式无贡献，读入只为口径完整
FACE_KEYS = (
    ("zh", "punct"),
    ("zh", "attached_after_digit"),
    ("zh", "attached_before_digit"),
    ("en", "punct"),
    ("shared", "math"),
    ("shared", "attached_after_digit"),
    ("shared", "attached_before_digit"),
    ("shared", "attached_between_digits"),
    ("shared", "dash_ellipsis"),
    ("shared", "curly_quotes"),
    ("shared", "sentence_finals"),
    ("shared", "emoji_bare"),
    ("shared", "emoji_variants"),
)

# 变体选择符不可见，混进模式会让证据行与命中都对不上号；变体选择符脱离基
# 字符单独成字面量的形态属病态，不作判罚对象
INVISIBLE_SELECTORS = frozenset({"\ufe0e", "\ufe0f"})

PROD_JUDGE_FILE = os.path.join("crates", "gloss-core", "src", "pipeline", "lang_gate.rs")
PROD_JUDGE_DIR = os.path.join("crates", "gloss-core", "src", "pipeline", "lang_gate")
DEV_JUDGE_DIR = os.path.join("scripts", "hooks", "language_spec")

# 开发侧派生模块按派生处豁免（运行时装入数据源并派生各字符面，判定公式与
# 基准锁定同住此模块），测试文件随测试目标豁免（基准字面量住测试模块）
DEV_DERIVATION_MODULE = "chinese.py"

MISSING_DATA_MESSAGE = "the langspec data source is missing: %s" % DATA_SOURCE

MISSING_DATA_REMEDY = (
    "restore %s; without it the character faces cannot be read and the"
    " literal scan cannot be built" % DATA_SOURCE
)

EMPTY_SURFACE_MESSAGE = "the language-judgment scan surface is empty: no judgment files found"

EMPTY_SURFACE_REMEDY = (
    "run the gate from the workspace root; an empty judgment surface means the"
    " literal scan cannot complete"
)


def face_chars(root: str, loader: Any = None) -> frozenset[str]:
    """判罚模式：数据源各字符面的非 ASCII 成员并集（剔除不可见变体选择符）。

    数据源缺失或损坏按失败关闭退出——判罚面建不出来时宁可拦门禁，不放行扫
    描。面键缺失按损坏处置（数据形态由核心库完整性测试前置锁定）。"""
    path = os.path.join(root, DATA_SOURCE)
    if not os.path.isfile(path):
        gate_report.fail(
            gate_report.Finding(
                rule=gate_report.RULE_EMPTY_SURFACE,
                path=DATA_SOURCE,
                line=0,
                message=MISSING_DATA_MESSAGE,
                remedy=MISSING_DATA_REMEDY,
            )
        )
        sys.exit(gate_report.EXIT_INFRA)
    try:
        data = tomllib.loads(manifests.read_text(path, loader))
        chars = set()
        for section, key in FACE_KEYS:
            chars.update(data[section][key])
    except (tomllib.TOMLDecodeError, KeyError, TypeError) as exc:
        gate_report.fail(
            gate_report.infra_failure(
                "cannot read the character faces from %s: %s" % (DATA_SOURCE, exc)
            )
        )
        sys.exit(gate_report.EXIT_INFRA)
    return frozenset(ch for ch in chars if ord(ch) > 127 and ch not in INVISIBLE_SELECTORS)


def prod_judge_files(root: str) -> list[str]:
    """生产侧判罚树：lang_gate 门面加 lang_gate/ 目录全部本语言（Rust）源码。"""
    files = []
    facade = os.path.join(root, PROD_JUDGE_FILE)
    if os.path.isfile(facade):
        files.append(facade)
    files.extend(manifests.walk_rs(os.path.join(root, PROD_JUDGE_DIR)))
    return files


def dev_judge_files(root: str) -> list[str]:
    """开发侧判罚模块：语言规范库目录下除派生模块与测试文件外的脚本。"""
    base = os.path.join(root, DEV_JUDGE_DIR)
    if not os.path.isdir(base):
        return []
    out = []
    for name in sorted(os.listdir(base)):
        path = os.path.join(base, name)
        if not name.endswith(".py") or not os.path.isfile(path):
            continue
        if name == DEV_DERIVATION_MODULE or name.endswith("_test.py"):
            continue
        out.append(path)
    return out


def rs_face_lines(src: str, faces: frozenset[str]) -> dict[int, tuple[int, str]]:
    """本语言（Rust）源码里命中判罚模式的（起始行，结束行，片段）：字面量提
    取复用文案门禁的状态机（注释、属性与内联测试模块天然出界），片段内容按
    行归集，行号即所在行（单行口径）。"""
    texts: dict[int, str] = {}
    check_i18n.literal_lines(src, texts)
    return {
        line: (line, fragment)
        for line, fragment in texts.items()
        if any(ch in faces for ch in fragment)
    }


def py_face_lines(src: str, faces: frozenset[str]) -> dict[int, tuple[int, str]]:
    """脚本源码里命中判罚模式的（起始行，结束行，片段）：语法树收集字符串
    常量，文档字符串（模块与类、函数首语句）随注释豁免，其余常量逐个核对，
    跨行常量携带整体行界供放行标记核对。"""
    tree = ast.parse(src)
    docstrings = set()
    for node in ast.walk(tree):
        if isinstance(node, (ast.Module, ast.ClassDef, ast.FunctionDef, ast.AsyncFunctionDef)):
            body = node.body
            if (
                body
                and isinstance(body[0], ast.Expr)
                and isinstance(body[0].value, ast.Constant)
                and isinstance(body[0].value.value, str)
            ):
                docstrings.add(id(body[0].value))
    hits: dict[int, tuple[int, str]] = {}
    for node in ast.walk(tree):
        if not isinstance(node, ast.Constant) or not isinstance(node.value, str):
            continue
        if id(node) in docstrings:
            continue
        if any(ch in faces for ch in node.value):
            end = node.end_lineno or node.lineno
            hits[node.lineno] = (end, node.value)
    return hits


def run(
    root: str, loader: Any = None, report: manifests.Reporter | None = None
) -> tuple[list[str], list[str]]:
    """执行语言字符字面量单源约束，返回通过与失败两个清单。"""
    rep = report if report is not None else manifests.Reporter()
    faces = face_chars(root, loader)
    oks = []
    scanned = 0
    violations = 0
    for path, extractor in (
        *[(f, rs_face_lines) for f in prod_judge_files(root)],
        *[(f, py_face_lines) for f in dev_judge_files(root)],
    ):
        scanned += 1
        src = manifests.read_text(path, loader)
        lines = src.split("\n")
        for no, (end, _fragment) in sorted(extractor(src, faces).items()):
            span = "\n".join(lines[no - 1 : end])
            if ALLOW_MARKER in span:
                for marked in range(no, end + 1):
                    line = lines[marked - 1] if marked <= len(lines) else ""
                    if ALLOW_MARKER in line and gate_report.allow_reason_missing(
                        ALLOW_MARKER, line
                    ):
                        rep.note(
                            gate_report.allow_reason_finding(
                                ALLOW_MARKER, manifests.rel(root, path), marked, line
                            )
                        )
                        violations += 1
                continue
            line = lines[no - 1] if no <= len(lines) else ""
            violations += 1
            rep.note(
                gate_report.Finding(
                    rule=RULE_LANGSPEC_LITERAL,
                    path=manifests.rel(root, path),
                    line=no,
                    message="language character literal outside the data source and"
                    " derivation sites",
                    evidence=line,
                    remedy=LANGSPEC_LITERAL_REMEDY,
                )
            )
    if scanned == 0:
        gate_report.fail(
            gate_report.Finding(
                rule=gate_report.RULE_EMPTY_SURFACE,
                path="",
                line=0,
                message=EMPTY_SURFACE_MESSAGE,
                remedy=EMPTY_SURFACE_REMEDY,
            )
        )
        sys.exit(gate_report.EXIT_INFRA)
    oks.append(
        "language spec literals single-sourced (%d judgment files scanned, %d violations)"
        % (scanned, violations)
    )
    return oks, rep.lines
