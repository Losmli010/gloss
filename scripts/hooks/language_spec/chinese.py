#!/usr/bin/env python3
"""语言规范·中文区：中文表述怎么写。中文主体只允许汉字、白名单字符、阿拉
伯数字与空白；专名与外文术语收进紧跟的全角括号作注记——注记括号按容器判
定，许嵌套、内部不做正文白名单检查只判单一语言（目标语言文字段与外文段不
混现），不配对独立判罚且内容落回正文受检。字符事实单源于核心库内嵌的语言
规范数据文件（crates/gloss-core/langspec/zh_en.toml），生产侧判定器自同一
文件编译期嵌入派生，两侧注释互指——本模块运行时装入并派生各字符面，文件
缺席或损坏当场失败，数据面移位不静默破坏门禁。工具指令段与可直接运行的命
令段整段豁免。判罚以带规则标识的违规条目返回，位置由调用方按路径与行号传
入，标识登记于统一登记表。被注释、提交信息与拉取请求内容门禁复用；本模块
自身注释同规。

"""

from __future__ import annotations

import os
import re
import sys
from pathlib import Path

import tomllib

_HOOKS = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if _HOOKS not in sys.path:
    sys.path.insert(0, _HOOKS)

import gate_report  # noqa: E402

RULE_PROSE_FOREIGN = "PROSE-FOREIGN"
RULE_PROSE_GLOSS_MIXED = "PROSE-GLOSS-MIXED"
RULE_PROSE_GLOSS_UNPAIRED = "PROSE-GLOSS-UNPAIRED"

FOREIGN_REMEDY = "put proper nouns in fullwidth bracket annotations and write the rest in Chinese"
GLOSS_MIXED_REMEDY = (
    "keep each bracket single-language: pure foreign terms or pure Chinese;"
    " move prose back into the body"
)
GLOSS_UNPAIRED_REMEDY = "complete or remove the unpaired bracket"

# 字符事实单源：生产侧判定器自同一数据文件编译期嵌入，两侧注释互指；装入
# 失败当场失败不静默降级——数据面移位必须显式修路径，不许门禁带病运行。
_SPEC_PATH = (
    Path(__file__).resolve().parents[3] / "crates" / "gloss-core" / "langspec" / "zh_en.toml"
)


def _load_spec() -> dict:
    with open(_SPEC_PATH, "rb") as handle:
        return tomllib.load(handle)


_SPEC = _load_spec()


def _face(text: str) -> frozenset:
    return frozenset(text)


def _ranges(text: str) -> tuple[tuple[int, int], ...]:
    pairs = []
    for token in text.split():
        start, end = token.split("-")
        pairs.append((int(start, 16), int(end, 16)))
    return tuple(pairs)


def _ranges_class(ranges: tuple[tuple[int, int], ...]) -> str:
    return "".join("\\U%08x-\\U%08x" % pair for pair in ranges)


_RANGES = {
    name: _ranges(_SPEC["ranges"][name])
    for name in ("latin", "han", "kana", "hangul", "cyrillic", "greek")
}

ZH_PUNCT = _face(_SPEC["zh"]["punct"])
# 白名单由场景标点、破折号省略号家族与中性数学面并成：与生产侧中文允许表
# 同源同集，家族面在开发侧并入白名单（两侧都不判，机制差异见登记表）
WHITELIST = ZH_PUNCT | _face(_SPEC["shared"]["dash_ellipsis"]) | _face(_SPEC["shared"]["math"])
ATTACHED_AFTER_DIGIT = _face(_SPEC["shared"]["attached_after_digit"])
ATTACHED_BEFORE_DIGIT = _face(_SPEC["shared"]["attached_before_digit"])
ATTACHED_BETWEEN_DIGITS = _face(_SPEC["shared"]["attached_between_digits"])
_GLOSS_OPEN, _GLOSS_CLOSE = _SPEC["shared"]["brackets"]["zh"]

LATIN = re.compile(r"[A-Za-z]")
CJK = re.compile("[" + _ranges_class(_RANGES["han"]) + "]")
# 汉字区段自数据源派生（三段口径）；中文标点与全角形式是宽度启发面——英
# 文区与行内代码出现即非纯英文。组合逻辑住代码，字符事实住数据源
CJKWISE = re.compile(
    "[" + _ranges_class(_RANGES["han"]) + "\u3000-\u303f\uff00-\uffef\u2018\u2019\u201c\u201d]"
)

# 工具指令段整段豁免：解释器声明，以及格式化与静态检查指令及其外文参数。
DIRECTIVE_SEG = re.compile(
    r"^[!—–\-\s]*#!"
    r"|^[\s—–\-]*(?:noqa|fmt|ruff|isort|pylint|type|shellcheck|yamllint"
    r"|actionlint|prettier|markdownlint)\b(?:[:\s][A-Za-z0-9_+=,.\s\-]*)?$"
)
# 可直接运行的命令段整段豁免：解释器或仓库工具名开头、全外文参数的段——
# 脚本用法行写可复制的命令本体，混入中文即失去可复制性，故整段放行；参数
# 区须含命令形态的结构性记号（双横旗标、路径分隔、占位与选项括号、选项冒
# 号），句点之类的散文标点不算，以工具词开头的英文散文句不豁免。已知残余
# 面：段中含路径分隔等记号的英文散文句仍会豁免，取舍见记录文件。
COMMAND_SEG = re.compile(
    r"^(?:python3(?:\.\d+)?|python|cargo|just|git|rg|bash|sh|make|curl)"
    r"(?:[ \t]+[-A-Za-z0-9_+=,./:@%<>[\]*?|]+)+$"
)
COMMAND_ARG_HINT = re.compile(r"--|/|<|\[|:")


def is_command_seg(seg: str) -> bool:
    """段是否为可直接运行的命令：命令形态全配，且参数区带结构性字符。"""
    return COMMAND_SEG.match(seg) is not None and COMMAND_ARG_HINT.search(seg) is not None


# 表述按中文标点与破折号省略号家族切段：切分正则由数据源的标点集与家族面
# 构造，换行是机械边界——切分只是判定粒度，多切无害，两面都在白名单内，协
# 同由派生保证；家族面入切分是指令段豁免的既有粒度前提（破折号尾随的中文
# 补注自成段）。
SEGMENT_SPLIT = re.compile(
    "[" + re.escape(_SPEC["zh"]["punct"] + _SPEC["shared"]["dash_ellipsis"]) + r"\n]+"
)
SEGMENT_TRIM = " \t\n─*-·-"


def _is_ascii_digit(ch: str) -> bool:
    return "0" <= ch <= "9"


def is_attached_symbol(text: str, index: int) -> bool:
    """数字附随面按三族邻接放行（前邻数字、后邻数字、数字夹住），其余位置
    按标点判——字符面与邻接语义都与生产侧同规，事实自数据源派生。"""
    ch = text[index]
    prev = text[index - 1] if index > 0 else ""
    nxt = text[index + 1] if index + 1 < len(text) else ""
    if ch in ATTACHED_AFTER_DIGIT and _is_ascii_digit(prev):
        return True
    if ch in ATTACHED_BEFORE_DIGIT and _is_ascii_digit(nxt):
        return True
    return ch in ATTACHED_BETWEEN_DIGITS and _is_ascii_digit(prev) and _is_ascii_digit(nxt)


def chinese_zone_problems(path: str, line: int, zone: str, label: str) -> list[gate_report.Finding]:
    """中文主体的字符级检查：只允许汉字、白名单字符、阿拉伯数字与空白。"""
    out = []
    for seg in SEGMENT_SPLIT.split(zone):
        seg = seg.strip(SEGMENT_TRIM)
        if not seg or DIRECTIVE_SEG.match(seg) or is_command_seg(seg):
            continue
        bad = []
        for index, ch in enumerate(seg):
            if (
                CJK.match(ch)
                or ch in WHITELIST
                or _is_ascii_digit(ch)
                # 不换行空格与全角空格是空白——表格列加宽靠不可断行的空白撑住
                or ch in " \t\n\r\u00a0\u3000"
            ):
                continue
            if is_attached_symbol(seg, index):
                continue
            if ch not in bad:
                bad.append(ch)
        if bad:
            shown = " ".join(bad[:3])
            out.append(
                gate_report.Finding(
                    rule=RULE_PROSE_FOREIGN,
                    path=path,
                    line=line,
                    message="%s body contains foreign letters or halfwidth punctuation"
                    " (%s) — put proper nouns in fullwidth bracket annotations,"
                    " rewrite the rest in Chinese" % (label, shown),
                    evidence=seg,
                    remedy=FOREIGN_REMEDY,
                )
            )
    return out


def _script_class(ch: str) -> str:
    if "a" <= ch <= "z" or "A" <= ch <= "Z":
        return "latin"
    code = ord(ch)
    for name in ("kana", "han", "hangul", "cyrillic", "greek", "latin"):
        if any(start <= code <= end for start, end in _RANGES[name]):
            return name
    return "other"


def script_runs(text: str) -> list[tuple[str, int]]:
    """连续文字段：同文字类的相邻字母连成段，段长供两字符阈值判定——与生
    产侧的运行段同构，归类次序也一致（假名、汉字、谚文、西里尔、希腊、拉
    丁扩展，残余归其他）。"""
    runs = []
    index = 0
    while index < len(text):
        if not text[index].isalpha():
            index += 1
            continue
        kind = _script_class(text[index])
        begin = index
        while index < len(text) and text[index].isalpha() and _script_class(text[index]) == kind:
            index += 1
        runs.append((kind, index - begin))
    return runs


def gloss_problems(path: str, line: int, gloss: str, label: str) -> list[gate_report.Finding]:
    """注记括号单语判定：容器内部不做正文白名单检查——目标语言文字段与外
    文段（连续两字符以上）混现即判罚，纯外文与纯中文放行。"""
    gloss = gloss.strip()
    if not gloss:
        return []
    has_target = False
    has_foreign = False
    for kind, length in script_runs(gloss):
        if length < 2:
            continue
        if kind == "han":
            has_target = True
        else:
            has_foreign = True
    if has_target and has_foreign:
        return [
            gate_report.Finding(
                rule=RULE_PROSE_GLOSS_MIXED,
                path=path,
                line=line,
                message="%s annotation bracket mixes foreign and Chinese runs"
                " — brackets hold a single language" % label,
                evidence=gloss,
                remedy=GLOSS_MIXED_REMEDY,
            )
        ]
    return []


GLOSS_UNPAIRED_TAIL = (
    " annotation brackets are unpaired — brackets must wrap annotations in matched"
    " pairs; unclosed bracket content stays in the prose and is checked as body"
)


def split_glosses(body: str) -> tuple[list[str], str, bool]:
    """按栈配对全角括号，切成注记与主体：返回（注记清单，主体残文，是否不
    配对）。括号许嵌套：各层自成一注记，父注记里以空白占位隔开子容器——子
    容器单判，父注记只判直接内容；不配对（含单边闭括号）置真，其内容落回
    主体受检。"""
    glosses = []
    residual = []
    unpaired = False
    stack: list[list[str]] = []
    for ch in body:
        if ch == _GLOSS_OPEN:
            stack.append([])
        elif ch == _GLOSS_CLOSE:
            if not stack:
                unpaired = True
                residual.append(ch)
            else:
                glosses.append("".join(stack.pop()))
                if stack:
                    stack[-1].append(" ")
                else:
                    residual.append(" ")
        else:
            (stack[-1] if stack else residual).append(ch)
    if stack:
        unpaired = True
        residual.append(" ".join("".join(buf) for buf in stack))
    return glosses, "".join(residual), unpaired


def body_problems(path: str, line: int, body: str, label: str) -> list[gate_report.Finding]:
    """整条表述的分域检查：先摘注记括号，再查主体与各注记；不配对括号独立
    判罚，其内容已随残文进主体检查。"""
    out = []
    glosses, residual, unpaired = split_glosses(body)
    evidence = body.replace("\n", " ")
    if unpaired:
        out.append(
            gate_report.Finding(
                rule=RULE_PROSE_GLOSS_UNPAIRED,
                path=path,
                line=line,
                message="%s%s" % (label, GLOSS_UNPAIRED_TAIL),
                evidence=evidence,
                remedy=GLOSS_UNPAIRED_REMEDY,
            )
        )
    for gloss in glosses:
        out += gloss_problems(path, line, gloss, label)
    out += chinese_zone_problems(path, line, residual, label)
    return out
