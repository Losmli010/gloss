#!/usr/bin/env python3
"""语言规范·代码引用：正文与代码混用的记号规则。结构性记号先豁免——行内
反引号、链接、强调、围栏、表格、列表与勾选框、日期、数学记号段、模板占
位符段、行尾放行标记——剥净后无可判语言的纯记号段放行；残留代码段判罚，
主体按区参数落中文区或英文区判定，区归属由使用方按文档路径把关。行内反
引号的内容按批判定：出现汉字或全角字符落英文判罚，不含且剥首尾空白后呈
数字主导形态（计数、计量、版本段、进制与区间）落数字形态判罚——数字与
百分比归正文，反引号只装逐字可复制的机器记号（token），同段两判罚互斥。
判罚以带规则标识的违规条目返回，位置由调用方按路径与行号传入，标识登记
于统一登记表。行级复合判定与标记语言文档逐行遍历是两个入口，被注释、提
交信息与拉取请求内容门禁复用；文档遍历的块级边界取自解析树
（markdown-it），
代码块（含缩进块与波浪围栏）、数学块与超文本标记块整段豁免，文档的结构判
罚（标题层级、重名标题、围栏闭合）移交排版门禁工具（rumdl）；围栏内的整
行注释按注释纪律受检（文档检查子项），围栏内其余内容是字面代码不判罚；未
登记注释语言的围栏（含双横线注释形态的查询语言）不受检，章节号引用在围栏
注释面不查，围栏注释子项不随区参数分派——已知盲区与取舍见记录文件；本模
块自身注释同规。

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

from language_spec.chinese import (  # noqa: E402
    _GLOSS_CLOSE,
    _GLOSS_OPEN,
    CJK,
    CJKWISE,
    DIRECTIVE_SEG,
    SEGMENT_SPLIT,
    SEGMENT_TRIM,
    body_problems,
    is_command_seg,
)
from language_spec.english import english_zone_problems  # noqa: E402

RULE_PROSE_CODEREF = "PROSE-CODEREF"
RULE_PROSE_SECTION_REF = "PROSE-SECTION-REF"
RULE_PROSE_LANGUAGE = "PROSE-LANGUAGE"
RULE_DOC_CODE_LANGUAGE = "COMMENT-DOC-CODE-LANGUAGE"
RULE_DOC_CODE_CODEREF = "COMMENT-DOC-CODE-CODEREF"
RULE_DOC_CODE_ENGLISH = "COMMENT-DOC-CODE-ENGLISH"
RULE_DOC_CODE_NUMERIC = "COMMENT-DOC-CODE-NUMERIC"

# 表述区归属的两个取值：中文区主体判定是缺省，英文区判据由使用方按文档路
# 径分派（提示词英文模板目录）；区只决定主体判定，结构性判罚与豁免面两区共用
ZONE_CHINESE = "chinese"
ZONE_ENGLISH = "english"

CODEREF_REMEDY = (
    "wrap the reference in backticks (exempt on prose surfaces) or move it to the record file"
)
SECTION_REF_REMEDY = "move the section-number citation to the record file"
LANGUAGE_REMEDY = "rewrite the text in Chinese, keeping proper nouns in bracket annotations"
ENGLISH_DOC_REMEDY = "rewrite the whole line in English"
CODE_LANGUAGE_REMEDY = (
    "rewrite the code block comment in Chinese, keeping proper nouns in bracket annotations"
)
CODE_CODEREF_REMEDY = "drop the reference or move the sample to the record file"
CODE_ENGLISH_REMEDY = "keep inline code English-only; write the Chinese wording in the prose"
CODE_NUMERIC_REMEDY = (
    "move numbers and percentages to the prose; backticks hold machine tokens only"
)

# 围栏语言到注释形态：井号系、双斜线系与标记注释三族；围栏语言不在清单内
# （纯文本、数据格式、空白信息串）不收注释——井号或双斜线在这些语言里不是
# 注记。只收整行注释：行尾注释混在字面代码里，字面量误报面大，留给人工评审。
HASH_FENCE_LANGS = frozenset(
    {
        "bash",
        "sh",
        "shell",
        "zsh",
        "console",
        "python",
        "py",
        "toml",
        "yaml",
        "yml",
        "ini",
        "conf",
        "ruby",
        "perl",
    }
)
SLASH_FENCE_LANGS = frozenset(
    {
        "rust",
        "rs",
        "c",
        "cpp",
        "h",
        "hpp",
        "java",
        "js",
        "jsx",
        "javascript",
        "ts",
        "tsx",
        "typescript",
        "go",
        "swift",
        "kt",
        "kotlin",
        "scala",
        "php",
        "dart",
        "zig",
    }
)
TAG_FENCE_LANGS = frozenset({"html", "xml", "svg"})

# 围栏注释语言的并集：整行注释子项只对这些语言的围栏开启
FENCE_COMMENT_LANGS = HASH_FENCE_LANGS | SLASH_FENCE_LANGS | TAG_FENCE_LANGS

# 各门禁共用的行尾放行标记：紧贴所在行并注明缘由即豁免
MARKER = re.compile(r"\b(?:secrets|lines|i18n|constraints|deps|langspec):allow\b")

# 列表与引用记号：段落头的有序列表记号，顿号是中文序号的点形态——结构记号
# 正则住代码（计划 §7.3），字面量随 langspec:allow 放行
LIST_MARKER = re.compile(r"^(?:[-*+]|\d+[.、]|>)\s+")  # langspec:allow 顿号属列表记号正则

DATE_RE = re.compile(r"\d{4}-\d{2}-\d{2}")

# 行内反引号的内容只许英文：中文属表述，写进正文——反引号豁免面以英文为前提
CODE_SPAN = re.compile(r"`([^`]*)`")

# 行内反引号数字形态：数字主导的机器记号（token）即判罚，规格由四字符集
# 加结构规则构成——前缀符号与井号前缀至多一个，数字主体（十进制段或进制
# 段）与分隔交替，单位或符号后缀至多一个；单位大小写敏感，白名单外放行
# （宁漏勿误）
NUMERIC_TOKEN = re.compile(
    r"^[<>~+±≤≥-]?#?"
    r"(?:\d[\d,_]*(?:\.\d+)*(?:[eE][+-]?\d+)?|0[xXbBoO][0-9a-fA-F_]+)"
    r"(?:\.\.\=?\d+|[.:\/-]\d+)*"
    r"(?:\s?(?:ms|us|ns|min|h|KB|MB|GB|KiB|MiB|GiB|bp|x)|[%‰±<>~+=-])?$"  # langspec:allow 数字附随符号属形态结构记号，非字符事实私持
)

# 数学记号段豁免：美元号包裹的算式段，内容限算式字符、不含汉字与美元号；
# 单美元号的普通文本（金额等）构不成段，不豁免；双美元号与单美元号同一字
# 符类——行内的双美元号按解析器本不算数学块，放宽会把散文整段吞进豁免
MATH_SPAN_BODY = r"[A-Za-z0-9\s\\{}()\[\]^_+=*/<>,.;:|\-]+"
MATH_SPAN = re.compile(r"\$\$%(b)s\$\$|\$%(b)s\$" % {"b": MATH_SPAN_BODY})

# 模板占位符段豁免：双花括号包裹的占位符名是模板语法记号，语言中立，裸形
# 态与行内代码形态同豁；名限标识符字符，与渲染端的占位符替换语法同形，内
# 嵌空白或汉字的写法构不成段，不豁免
PLACEHOLDER_SPAN = re.compile(r"\{\{[A-Za-z0-9_.-]+\}\}")

# 注记括号尾注：标题尾部携带的全角括号注记（限定符等）随注记记号剥除——
# 括号对自语言规范数据源派生，与注记容器同一字符事实
ANNOTATION_TAIL = re.compile(
    re.escape(_GLOSS_OPEN)
    + "[^"
    + re.escape(_GLOSS_CLOSE)
    + "]*"
    + re.escape(_GLOSS_CLOSE)
    + r"\s*$"
)

# 引用记号前缀：块引用内的围栏内容行带着引用记号落地，剥后再认注释形态
QUOTE_PREFIX = re.compile(r"^(?:\s*>[ \t]?)+")


def strip_links(text: str) -> str:
    """链接与图片逐层剥除：徽章是套着图片的嵌套链接，先剥内层图片再剥外层。"""
    for pattern in (r"!\[[^\]]*\]\([^)]*\)", r"\[[^\]]*\]\([^)]*\)"):
        while True:
            stripped = re.sub(pattern, " ", text)
            if stripped == text:
                break
            text = stripped
    return text


def prose_line_problems(
    path: str, line: int, text: str, zone: str = ZONE_CHINESE
) -> list[gate_report.Finding]:
    """单行表述判定：结构性记号豁免后按区落主体判定——中文区主体必须中文，
    英文区主体必须纯英文。"""
    text = text.strip()
    if not text:
        return []
    text = LIST_MARKER.sub("", text)  # 列表与引用记号非表述
    text = re.sub(r"^\[[ xX]\]\s+", "", text)  # 任务列表记号：勾选框非表述
    text = re.sub(r"`[^`]*`", " ", text)  # 行内代码：命令、路径与标识符豁免
    text = PLACEHOLDER_SPAN.sub(" ", text)  # 模板占位符段：双花括号占位符是模板语法记号，豁免
    text = MATH_SPAN.sub(" ", text)  # 数学记号段：算式是语言中立记号，豁免
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
                message=(
                    "text contains a code segment — move references and samples to the record files"  # langspec:allow 消息排版破折号，非判罚事实
                ),
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
                message=(  # langspec:allow 消息排版破折号，非判罚事实
                    'text contains a "§ section number" style external reference — cite it in the record files'  # langspec:allow 消息排版破折号，非判罚事实
                ),
                evidence=text,
                remedy=SECTION_REF_REMEDY,
            )
        ]
    if zone == ZONE_ENGLISH:
        return english_zone_problems(path, line, text, "text", ENGLISH_DOC_REMEDY)
    if not CJK.search(text):
        return [
            gate_report.Finding(
                rule=RULE_PROSE_LANGUAGE,
                path=path,
                line=line,
                message="text must be written in Chinese",
                evidence=text,
                remedy=LANGUAGE_REMEDY,
            )
        ]
    return body_problems(path, line, text, "text")


def fence_language(info: str) -> str:
    """围栏信息串取语言名：首段小写，花括号属性与标题参数剥除。"""
    first = re.split(r"[\s,{]", info.strip())
    return first[0].lower() if first and first[0] else ""


def fence_comment_body(line: str, lang: str) -> str | None:
    """围栏行按语言取整行注释的表述体：非注释行返回空表值（None），空注释
    返回空串。井号系认行首井号（解释器声明除外），双斜线系认行首双斜线并
    归一化文档注释前缀（三斜线与斜线叹号），标记注释族只收单行成对的标记
    注释。"""
    stripped = line.strip()
    if lang in HASH_FENCE_LANGS:
        if stripped.startswith("#") and not stripped.startswith("#!"):
            return stripped.lstrip("#").strip()
    elif lang in SLASH_FENCE_LANGS:
        if stripped.startswith("//"):
            return stripped[2:].lstrip("/!").strip()
    elif lang in TAG_FENCE_LANGS:
        if stripped.startswith("<!--") and stripped.endswith("-->") and len(stripped) > 7:
            return stripped[4:-3].strip()
    return None


def fence_comment_problems(path: str, lineno: int, line: str, lang: str) -> list:
    """围栏内整行注释按注释纪律受检：放行标记与不变量注释豁免，纯指令段与
    可运行命令段放行，其余落中文区判定。"""
    body = fence_comment_body(line, lang)
    if body is None:
        return []
    findings = gate_report.allow_reason_findings_for_line(path, lineno, body)
    if MARKER.search(body) or body.startswith("SAFETY:"):
        return findings
    body = MATH_SPAN.sub(" ", body)  # 数学记号段：算式是语言中立记号，豁免
    segs = [seg.strip(SEGMENT_TRIM) for seg in SEGMENT_SPLIT.split(body)]
    segs = [seg for seg in segs if seg]
    if not segs or all(DIRECTIVE_SEG.match(seg) or is_command_seg(seg) for seg in segs):
        return findings
    evidence = body
    if "`" in body:
        findings.append(
            gate_report.Finding(
                rule=RULE_DOC_CODE_CODEREF,
                path=path,
                line=lineno,
                message=(  # langspec:allow 消息排版破折号，非判罚事实
                    "code block comment contains a code segment — move references and samples to the record files"  # langspec:allow 消息排版破折号，非判罚事实
                ),
                evidence=evidence,
                remedy=CODE_CODEREF_REMEDY,
            )
        )
        return findings
    if not CJK.search(body):
        findings.append(
            gate_report.Finding(
                rule=RULE_DOC_CODE_LANGUAGE,
                path=path,
                line=lineno,
                message="code block comment must be written in Chinese",
                evidence=evidence,
                remedy=CODE_LANGUAGE_REMEDY,
            )
        )
        return findings
    return findings + body_problems(path, lineno, body, "code block comment")


_PARSER = None


def markdown_parser():
    """标记语言解析器单点：解析器（markdown-it）按通用标记规范解析，开表格
    装任务列表与数学记号插件；块级边界供遍历分类，解释器库缺席属门禁底座
    缺失，由环境初始化校验在场、运行时按异常失败关闭。"""
    global _PARSER
    if _PARSER is None:
        from markdown_it import MarkdownIt
        from mdit_py_plugins.dollarmath import dollarmath_plugin
        from mdit_py_plugins.tasklists import tasklists_plugin

        parser = MarkdownIt("commonmark", {"html": True}).enable("table")
        dollarmath_plugin(parser)
        tasklists_plugin(parser)
        _PARSER = parser
    return _PARSER


def markdown_problems(path: str, src: str, zone: str = ZONE_CHINESE) -> list[gate_report.Finding]:
    """标记语言文档逐行受检：块级边界取自解析树——代码块（含缩进块与波浪
    围栏）、数学块与超文本标记块整段豁免；围栏内已登记语言的整行注释作为
    文档检查子项照注释纪律判定，其余内容是字面代码放行；其余行照原文逐行
    判，行内豁免（反引号、链接、数学记号段等）由行级判定承载，表述区归属
    由调用方按文档路径传入。"""
    # 行模型以解析器为单点：先做与解析器相同的换行归一化再按换行切分——
    # 通用的按行切分另认纵向制表与统一码行分隔符等边界，两种行模型错位
    # 会让豁免窗口整体偏移
    lines = src.replace("\r\n", "\n").replace("\r", "\n").split("\n")
    exempt = [False] * len(lines)
    fence_lang_by_line: dict[int, str] = {}
    for token in markdown_parser().parse(src):
        if token.map is None:
            continue
        start, end = token.map
        if token.type in ("fence", "code_block"):
            lang = fence_language(token.info)
            if token.type == "fence" and lang in FENCE_COMMENT_LANGS:
                # 围栏开行与闭行豁免，内容行交注释子项；未闭合时闭行不存在，
                # 末行按内容行受检
                exempt[start] = True
                closed = len(token.content.splitlines()) == end - start - 2
                if closed:
                    exempt[end - 1] = True
                for offset in range(start + 1, end - 1 if closed else end):
                    fence_lang_by_line[offset] = lang
            else:
                exempt[start:end] = [True] * (end - start)
        elif token.type in ("math_block", "html_block"):
            exempt[start:end] = [True] * (end - start)
    out: list[gate_report.Finding] = []
    for offset, line in enumerate(lines):
        if exempt[offset]:
            continue
        lang = fence_lang_by_line.get(offset)
        if lang is not None:
            out += fence_comment_problems(path, offset + 1, QUOTE_PREFIX.sub("", line), lang)
        else:
            out += markdown_line_problems(path, offset + 1, line, zone)
    return out


def numeric_span_finding(path: str, lineno: int, span: str) -> gate_report.Finding | None:
    """行内反引号数字形态判定：剥首尾空白后按序三步——空段或含汉字全角归英
    文规则放行，字母或下划线开头是标识符放行，其余整体匹配数字形态，命中判
    罚、未命中放行（宁漏勿误：规格覆盖不了的长尾留人工评审）。"""
    token = span.strip()
    if not token or CJKWISE.search(token):
        return None
    if token[0].isalpha() or token[0] == "_":
        return None
    if not NUMERIC_TOKEN.fullmatch(token):
        return None
    return gate_report.Finding(
        rule=RULE_DOC_CODE_NUMERIC,
        path=path,
        line=lineno,
        message=(
            "inline code holds a bare number — numbers and percentages belong in the prose"  # langspec:allow 消息排版破折号，非判罚事实
        ),
        evidence=token,
        remedy=CODE_NUMERIC_REMEDY,
    )


def markdown_line_problems(
    path: str, lineno: int, line: str, zone: str = ZONE_CHINESE
) -> list[gate_report.Finding]:
    stripped = line.strip()
    if not stripped:
        return []
    out = []
    for span in CODE_SPAN.findall(line):
        if CJKWISE.search(span):
            out.append(
                gate_report.Finding(
                    rule=RULE_DOC_CODE_ENGLISH,
                    path=path,
                    line=lineno,
                    message=(  # langspec:allow 消息排版破折号，非判罚事实
                        "inline code contains Chinese — backticks hold English-only code; write the Chinese in the prose"  # langspec:allow 消息排版破折号，非判罚事实
                    ),
                    evidence=span,
                    remedy=CODE_ENGLISH_REMEDY,
                )
            )
            continue
        numeric = numeric_span_finding(path, lineno, span)
        if numeric is not None:
            out.append(numeric)
    if re.match(r" {0,3}\[[^\]]+\]:", line):
        return out  # 引用定义：行内链接的块级同族记号，结构性豁免，反引号判罚先于豁免
    heading = re.match(r"^#+\s*(.*)$", stripped)
    if heading:
        key = ANNOTATION_TAIL.sub("", heading.group(1)).strip()
        if re.fullmatch(r"[A-Za-z0-9_]+", key):
            return out  # 小节标题即测试名，双向核对的键
        stripped = heading.group(1)
    if stripped.startswith("|"):
        cells = stripped.strip("|").split("|")
        if all(re.fullmatch(r"[-: ]*", cell) for cell in cells):
            return out  # 表格分隔行
        cells_problems = []
        for cell in cells[1:]:
            cells_problems += prose_line_problems(path, lineno, cell, zone)
        return out + cells_problems
    return out + prose_line_problems(path, lineno, stripped, zone)
