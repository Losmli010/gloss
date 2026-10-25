#!/usr/bin/env python3
"""语言规范·代码引用：正文与代码混用的记号规则。结构性记号先豁免——行内
反引号、链接、强调、围栏、表格、列表与勾选框、日期、行尾放行标记——剥净
后无可判语言的纯记号段放行；残留代码段判罚，主体落中文区判定。判罚以带规
则标识的违规条目返回，位置由调用方按路径与行号传入，标识登记于统一登记表。
行级复合判定与标记语言文档逐行遍历是两个入口，被注释、提交信息与拉取请求
内容门禁复用；文档结构遍历只服务文档子门禁，判标题跳级、同文档重名标题与
结尾围栏不闭合三罚；本模块自身注释同规。

模块名避开解释器标准库的交互解释器模块。
"""

from __future__ import annotations

import os
import re
import sys

_HOOKS = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if _HOOKS not in sys.path:
    sys.path.insert(0, _HOOKS)

import gate_report  # noqa: E402

from language_spec.chinese import CJK, body_problems  # noqa: E402

RULE_PROSE_CODEREF = "PROSE-CODEREF"
RULE_PROSE_SECTION_REF = "PROSE-SECTION-REF"
RULE_PROSE_LANGUAGE = "PROSE-LANGUAGE"
RULE_DOC_HEADING_JUMP = "COMMENT-DOC-HEADING-JUMP"
RULE_DOC_HEADING_DUP = "COMMENT-DOC-HEADING-DUP"
RULE_DOC_FENCE_UNCLOSED = "COMMENT-DOC-FENCE-UNCLOSED"

CODEREF_REMEDY = (
    "wrap the reference in backticks (exempt on prose surfaces) or move it to the record file"
)
SECTION_REF_REMEDY = "move the section-number citation to the record file"
LANGUAGE_REMEDY = "rewrite the text in Chinese, keeping proper nouns in bracket annotations"
HEADING_JUMP_REMEDY = "insert the missing intermediate heading or step this one down"
HEADING_DUP_REMEDY = "rename one of the repeated headings so document anchors stay unique"
FENCE_UNCLOSED_REMEDY = "close the code fence with a matching fence line before the end"

# 各门禁共用的行尾放行标记：紧贴所在行并注明缘由即豁免
MARKER = re.compile(r"\b(?:secrets|lines|i18n|constraints|deps):allow\b")

DATE_RE = re.compile(r"\d{4}-\d{2}-\d{2}")


def strip_links(text: str) -> str:
    """链接与图片逐层剥除：徽章是套着图片的嵌套链接，先剥内层图片再剥外层。"""
    for pattern in (r"!\[[^\]]*\]\([^)]*\)", r"\[[^\]]*\]\([^)]*\)"):
        while True:
            stripped = re.sub(pattern, " ", text)
            if stripped == text:
                break
            text = stripped
    return text


def prose_line_problems(path: str, line: int, text: str) -> list[gate_report.Finding]:
    """单行表述判定：结构性记号豁免后落中文区判定，主体必须中文。"""
    text = text.strip()
    if not text:
        return []
    text = re.sub(r"^(?:[-*+]|\d+[.、]|>)\s+", "", text)  # 列表与引用记号非表述
    text = re.sub(r"^\[[ xX]\]\s+", "", text)  # 任务列表记号：勾选框非表述
    text = re.sub(r"`[^`]*`", " ", text)  # 行内代码：命令、路径与标识符豁免
    text = strip_links(text)
    text = text.replace("**", " ")  # 强调记号：包裹的仍是中文表述，记号非内容
    text = DATE_RE.sub(" ", text)
    if re.fullmatch(r"[A-Za-z0-9_]+", text.strip()):
        return []  # 整段标识符（如类别键）豁免
    if not re.search(r"[A-Za-z\u4e00-\u9fff]", text):
        return []  # 剥去代码与日期后只剩数字符号（纯命令格、分隔符）无可判语言
    if "`" in text:
        return [
            gate_report.Finding(
                rule=RULE_PROSE_CODEREF,
                path=path,
                line=line,
                message="text contains a code segment — move references and samples to the record"
                " files",
                evidence=text,
                remedy=CODEREF_REMEDY,
            )
        ]
    if "§" in text:
        return [
            gate_report.Finding(
                rule=RULE_PROSE_SECTION_REF,
                path=path,
                line=line,
                message='text contains a "§ section number" style external reference — cite it in'
                " the record files",
                evidence=text,
                remedy=SECTION_REF_REMEDY,
            )
        ]
    if not CJK.search(text):
        return [
            gate_report.Finding(
                rule=RULE_PROSE_LANGUAGE,
                path=path,
                line=line,
                message="text must be written in Chinese: %s" % text[:40],
                evidence=text,
                remedy=LANGUAGE_REMEDY,
            )
        ]
    return body_problems(path, line, text, "text")


def markdown_problems(path: str, src: str) -> list[gate_report.Finding]:
    """标记语言文档逐行受检：代码围栏、行内反引号与链接豁免；表格首列与纯
    标识符小节标题是双向核对的键，豁免；日期是语言中立数据格式，放行。"""
    out = []
    in_fence = False
    for lineno, line in enumerate(src.splitlines(), 1):
        if line.strip().startswith("```"):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        out += markdown_line_problems(path, lineno, line)
    return out


def markdown_structure_problems(path: str, src: str) -> list[gate_report.Finding]:
    """标记语言文档结构受检：标题层级较上一个标题一次只加深一级，标题文本
    全文档不重名，代码围栏到文末必须闭合；围栏开合翻位与逐行遍历同一口径，
    不闭合按开启行判罚。"""
    out: list[gate_report.Finding] = []
    fence_line = 0
    prev_level = 0
    seen: dict[str, int] = {}
    for lineno, line in enumerate(src.splitlines(), 1):
        stripped = line.strip()
        if stripped.startswith("```"):
            fence_line = 0 if fence_line else lineno
            continue
        if fence_line:
            continue
        heading = re.match(r"^(#+)\s*(.*)$", stripped)
        if not heading:
            continue
        level = len(heading.group(1))
        title = heading.group(2).strip()
        if prev_level and level > prev_level + 1:
            out.append(
                gate_report.Finding(
                    rule=RULE_DOC_HEADING_JUMP,
                    path=path,
                    line=lineno,
                    message="heading level jumps from %d to %d" % (prev_level, level),
                    evidence=stripped,
                    remedy=HEADING_JUMP_REMEDY,
                )
            )
        if title:
            first = seen.setdefault(title, lineno)
            if first != lineno:
                out.append(
                    gate_report.Finding(
                        rule=RULE_DOC_HEADING_DUP,
                        path=path,
                        line=lineno,
                        message="heading repeats an earlier heading in the same document",
                        evidence=title,
                        remedy=HEADING_DUP_REMEDY,
                    )
                )
        prev_level = level
    if fence_line:
        out.append(
            gate_report.Finding(
                rule=RULE_DOC_FENCE_UNCLOSED,
                path=path,
                line=fence_line,
                message="code fence opened here is never closed by the end of the document",
                evidence=src.splitlines()[fence_line - 1].strip(),
                remedy=FENCE_UNCLOSED_REMEDY,
            )
        )
    return out


def markdown_line_problems(path: str, lineno: int, line: str) -> list[gate_report.Finding]:
    stripped = line.strip()
    if not stripped:
        return []
    heading = re.match(r"^#+\s*(.*)$", stripped)
    if heading:
        key = re.sub(r"（[^）]*）\s*$", "", heading.group(1)).strip()
        if re.fullmatch(r"[A-Za-z0-9_]+", key):
            return []  # 小节标题即测试名，双向核对的键
        stripped = heading.group(1)
    if stripped.startswith("|"):
        cells = stripped.strip("|").split("|")
        if all(re.fullmatch(r"[-: ]*", cell) for cell in cells):
            return []  # 表格分隔行
        out = []
        for cell in cells[1:]:
            out += prose_line_problems(path, lineno, cell)
        return out
    return prose_line_problems(path, lineno, stripped)
