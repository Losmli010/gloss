#!/usr/bin/env python3
"""语言规范·中文区：中文表述怎么写。中文主体只允许汉字、白名单字符、阿拉
伯数字与空白；专名与外文术语收进紧跟的全角括号作注记——注记括号按容器判
定，许嵌套、内部不做正文白名单检查只判单一语言（目标语言文字段与外文段不
混现），不配对独立判罚且内容落回正文受检；汉字与拉丁字母或数字直接相邻判
罚（盘古之白），繁体独有字形判罚（简体判定，豁免仅代码引用），注记括号里
的纯外文内容过长或带虚词走软约束候选（括号锚点连贯，警告不计违规）。字符
事实单源于核心库内嵌的语言规范数据目录（crates/gloss-core/langspec/）的判
定数据与简繁对串两文件，生产侧判定器自同一组文件编译期嵌入派生，两侧
注释互指——本模块运行时装入并派生各字符面，文件缺席或损坏当场失败，数据
面移位不静默破坏门禁。工具指令段与可直接运行的命令段整段豁免。判罚以带规
则标识的违规条目返回，软约束候选（advisory）走警告通道不计违规，位置由调
用方按路径与行号传入，标识登记于统一登记表。被注释、提交信息与拉取请求内
容门禁复用；本模块自身注释同规。

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
RULE_PROSE_PANGU = "PROSE-PANGU"
RULE_PROSE_TRADITIONAL = "PROSE-TRADITIONAL"
RULE_PROSE_ANCHOR = "PROSE-ANCHOR"

FOREIGN_REMEDY = "put proper nouns in fullwidth bracket annotations and write the rest in Chinese"
GLOSS_MIXED_REMEDY = (
    "keep each bracket single-language: pure foreign terms or pure Chinese;"
    " move prose back into the body"
)
GLOSS_UNPAIRED_REMEDY = "complete or remove the unpaired bracket"
PANGU_REMEDY = "keep one halfwidth space between Chinese and Latin letters or digits"
TRADITIONAL_REMEDY = "rewrite traditional-only glyphs in simplified Chinese"
ANCHOR_REMEDY = (
    "brackets anchor a Chinese term and are not sentence parts — write the"
    " prose so it stands with the bracket removed"
)

# 字符事实单源：生产侧判定器自同一数据文件编译期嵌入，两侧注释互指；装入
# 失败当场失败不静默降级——数据面移位必须显式修路径，不许门禁带病运行。
_SPEC_PATH = (
    Path(__file__).resolve().parents[3] / "crates" / "gloss-core" / "langspec" / "zh_en.toml"
)

# 简繁对串单源：与生产侧共享同一数据文件（生产侧字符映射模块编译期嵌入），
# 每对前字是繁体独有字形；对串良构（成对、繁简有别、升序不重）由核心库锁
# 定测试背书，此处装入失败当场失败不静默降级。
_PAIRS_PATH = (
    Path(__file__).resolve().parents[3]
    / "crates"
    / "gloss-core"
    / "langspec"
    / "simplified_pairs.txt"
)

_TRADITIONAL = frozenset(_PAIRS_PATH.read_text(encoding="utf-8").strip()[0::2])


def _load_spec() -> dict:
    with open(_SPEC_PATH, "rb") as handle:
        return tomllib.load(handle)


_SPEC = _load_spec()


def _face(text: str) -> frozenset:
    return frozenset(text)


def _ranges(text: str, name: str) -> tuple[tuple[int, int], ...]:
    pairs = []
    for token in text.split():
        start, end = token.split("-")
        lo, hi = int(start, 16), int(end, 16)
        if lo > hi:
            raise ValueError(f"{name}: range {token!r} is reversed")
        pairs.append((lo, hi))
    return tuple(pairs)


def _ranges_class(ranges: tuple[tuple[int, int], ...]) -> str:
    return "".join("\\U%08x-\\U%08x" % pair for pair in ranges)


_RANGES = {
    name: _ranges(_SPEC["ranges"][name], name)
    for name in ("latin", "han", "kana", "hangul", "cyrillic", "greek")
}

ZH_PUNCT = _face(_SPEC["zh"]["punct"])
# 白名单由场景标点、破折号省略号家族与中性数学面并成：与生产侧中文允许表
# 同源同集，家族面在开发侧并入白名单（两侧都不判，机制差异见登记表）
WHITELIST = ZH_PUNCT | _face(_SPEC["shared"]["dash_ellipsis"]) | _face(_SPEC["shared"]["math"])
# 数字附随两向只服务中文区路径（英文区不查附随），取共用面并中文场景面的
# 并集——场景差异成员（摄氏华氏、万分率与全角人民币符）住场景面，派生处
# 合面与生产侧中文附随面同构；两点省略号出族随家族面收缩自动跟随
ATTACHED_AFTER_DIGIT = _face(_SPEC["shared"]["attached_after_digit"]) | _face(
    _SPEC["zh"]["attached_after_digit"]
)
ATTACHED_BEFORE_DIGIT = _face(_SPEC["shared"]["attached_before_digit"]) | _face(
    _SPEC["zh"]["attached_before_digit"]
)
ATTACHED_BETWEEN_DIGITS = _face(_SPEC["shared"]["attached_between_digits"])
_GLOSS_OPEN, _GLOSS_CLOSE = _SPEC["shared"]["brackets"]["zh"]
# 表情事实与生产侧同源互指：开发侧不设独立表情判定——表情在白名单外，字符
# 白名单天然全覆盖；事实自数据源装入供对账锁定（表情事实全部落在白名单外），
# 生产侧自同一数据面判表情违约，机制差异见登记表。
EMOJI_RANGES = _ranges(_SPEC["shared"]["emoji_ranges"], "shared.emoji_ranges")
EMOJI_BARE = _face(_SPEC["shared"]["emoji_bare"])
EMOJI_VARIANTS = _face(_SPEC["shared"]["emoji_variants"])

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
SEGMENT_TRIM = " \t\n─*-"


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


def _is_ascii_alnum(ch: str) -> bool:
    return ch.isascii() and ch.isalnum()


def pangu_pairs(seg: str) -> list[str]:
    """段内汉字与拉丁字母或数字直接相邻的相邻对（去重保序）：空白隔开不放
    行邻接，与生产侧盘古之白同判——相邻即罚，须留一个半角空格。"""
    out = []
    for left, right in zip(seg, seg[1:]):
        mixed = (CJK.match(left) and _is_ascii_alnum(right)) or (
            _is_ascii_alnum(left) and CJK.match(right)
        )
        if mixed and left + right not in out:
            out.append(left + right)
    return out


def chinese_zone_problems(path: str, line: int, zone: str, label: str) -> list[gate_report.Finding]:
    """中文主体的字符级检查：只允许汉字、白名单字符、阿拉伯数字与空白；盘
    古之白与简体判定随段同查。"""
    out = []
    for seg in SEGMENT_SPLIT.split(zone):
        seg = seg.strip(SEGMENT_TRIM)
        if not seg or DIRECTIVE_SEG.match(seg) or is_command_seg(seg):
            continue
        bad = []
        for index, ch in enumerate(seg):
            if CJK.match(ch):
                continue
            if (
                ch in WHITELIST
                or _is_ascii_digit(ch)
                # 不换行空格与全角空格是空白——表格列加宽靠不可断行的空白撑住
                or ch in " \t\n\r\u00a0\u3000"
            ):
                continue
            if is_attached_symbol(seg, index):
                continue
            if ch not in bad:
                bad.append(ch)
        traditional = traditional_glyphs(seg)
        if bad:
            shown = " ".join(bad[:3])
            out.append(
                gate_report.Finding(
                    rule=RULE_PROSE_FOREIGN,
                    path=path,
                    line=line,
                    message="%s body contains characters outside the derived whitelist"
                    " (%s) — put proper nouns in fullwidth bracket annotations,"
                    " rewrite the rest in Chinese" % (label, shown),
                    evidence=seg,
                    remedy=FOREIGN_REMEDY,
                )
            )
        if traditional:
            shown = " ".join(traditional[:3])
            out.append(
                gate_report.Finding(
                    rule=RULE_PROSE_TRADITIONAL,
                    path=path,
                    line=line,
                    message="%s body contains traditional-only glyphs (%s) — write"
                    " simplified Chinese" % (label, shown),
                    evidence=seg,
                    remedy=TRADITIONAL_REMEDY,
                )
            )
        pairs = pangu_pairs(seg)
        if pairs:
            shown = " ".join(pairs[:3])
            out.append(
                gate_report.Finding(
                    rule=RULE_PROSE_PANGU,
                    path=path,
                    line=line,
                    message="%s body joins Chinese with Latin letters or digits without"
                    " a space (%s)" % (label, shown),
                    evidence=seg,
                    remedy=PANGU_REMEDY,
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


# 括号锚点连贯软约束（警告级候选，宁漏勿误）：括号是锚点对照不是句子成分，
# 高置信启发式只抓塞英文退化形态——虚词小清单（冠词、介词、连词与系动词）
# 命中短语的句子性，长度阈值加空白分词抓无虚词的长外文插语；两条件任一命中
# 即出候选，不计门禁通过与否。斜杠与路径形态的术语清单、单词塞入形态机判
# 不可靠，都不出候选，归人工清查与评审兜底。
ANCHOR_FUNCTION_WORDS = frozenset(
    {
        "a",
        "an",
        "the",
        "of",
        "to",
        "in",
        "on",
        "at",
        "by",
        "for",
        "with",
        "from",
        "and",
        "or",
        "but",
        "is",
        "are",
        "was",
        "were",
        "be",
    }
)
ANCHOR_MAX_LENGTH = 24

ANCHOR_WORD = re.compile(r"[A-Za-z0-9']+")


def anchor_reasons(gloss: str) -> list[str]:
    """锚点连贯的命中缘由：任一高置信启发式命中即返回——外文词两个以上且含
    虚词，或空白分词三段以上且内容长度超阈值；候选只打印不判罚。"""
    words = [word.lower() for word in ANCHOR_WORD.findall(gloss)]
    out = []
    if len(words) >= 2 and any(word in ANCHOR_FUNCTION_WORDS for word in words):
        out.append("reads like a phrase with a function word")
    if len(gloss.split()) >= 3 and len(gloss) > ANCHOR_MAX_LENGTH:
        out.append("too long to be a term gloss")
    return out


def traditional_glyphs(text: str) -> list[str]:
    """文本里的繁体独有字形（去重保序）：与生产侧同用共享对串逐一比对（对
    串键面即繁体字形，不再按区段过滤——扩展区繁形与生产侧同判），括号容器
    与正文同判（开发侧无引文容器，豁免仅代码引用）。"""
    out = []
    for ch in text:
        if ch in _TRADITIONAL and ch not in out:
            out.append(ch)
    return out


def gloss_problems(path: str, line: int, gloss: str, label: str) -> list[gate_report.Finding]:
    """注记括号单语判定：容器内部不做正文白名单检查——目标语言文字段与外
    文段（连续两字符以上）混现即判罚，纯外文与纯中文放行；简体判定随容器
    内汉字同查，纯外文内容另过锚点连贯启发式，命中以软约束候选返回（警告
    不计违规）。"""
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
    out = []
    if has_target and has_foreign:
        out.append(
            gate_report.Finding(
                rule=RULE_PROSE_GLOSS_MIXED,
                path=path,
                line=line,
                message="%s annotation bracket mixes foreign and Chinese runs"
                " — brackets hold a single language" % label,
                evidence=gloss,
                remedy=GLOSS_MIXED_REMEDY,
            )
        )
    glyphs = traditional_glyphs(gloss)
    if glyphs:
        shown = " ".join(glyphs[:3])
        out.append(
            gate_report.Finding(
                rule=RULE_PROSE_TRADITIONAL,
                path=path,
                line=line,
                message="%s annotation bracket contains traditional-only glyphs (%s) — write"
                " simplified Chinese" % (label, shown),
                evidence=gloss,
                remedy=TRADITIONAL_REMEDY,
            )
        )
    if has_foreign and not has_target:
        out.extend(
            gate_report.Finding(
                rule=RULE_PROSE_ANCHOR,
                path=path,
                line=line,
                message="%s annotation bracket %s — brackets anchor a Chinese term,"
                " not sentence parts" % (label, reason),
                evidence=gloss,
                remedy=ANCHOR_REMEDY,
                advisory=True,
            )
            for reason in anchor_reasons(gloss)
        )
    return out


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
