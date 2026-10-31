#!/usr/bin/env python3
"""记录目录标点门禁：对注释记录目录（docs/comments）的标记语言文档判标点
书写——直角引号出现即罚、半角标点紧邻汉字即罚、全角括号与弯引号在判定单
元内配对判定。判定先过解析树（markdown-it）：段落、标题、表格单元格、列
表项与块引用段落各自成判定单元，单元内硬换行合并为空白，行内代码、数学
段、超文本与图片剥除豁免，围栏与代码块整段不在判定面，配对不跨单元。判
定规则内联本脚本——本门禁是唯一消费方，判定不进语言规范库共用；作为注
释纪律门禁路由的第四个门禁并入，判罚经统一诊断出口打印，退出码按全脚本
统一约定。

受检面：记录目录（docs/comments）下的标记语言文档，与文档书写门禁的记录
目录豁免面互斥；其余标记语言文档不在本门禁。半角邻接的豁免：数字两侧天
然放行（日期、小数与千分位），半角句点后接一至六位字母数字到边界按文件名
形态放行、后接数字按章节号形态放行；连接号（en dash）、省略号与间隔号不
在半角邻接集——形态判罚有意不做，宁漏勿误。

用法：python3 scripts/hooks/comments/check_record_punct.py <repo-root> [file ...]，
其余参数为可选的文件清单（供单测传入临时样本）；缺省扫记录目录全部被跟
踪文件，清单为空按失败关闭退出。
"""

from __future__ import annotations

import os
import re
import sys

_HOOKS = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if _HOOKS not in sys.path:
    sys.path.insert(0, _HOOKS)

import gate_report  # noqa: E402
from language_spec.chinese import CJK  # noqa: E402
from language_spec.coderef import markdown_parser  # noqa: E402

RULE_PUNCT_CORNER = "PUNCT-CORNER"
RULE_PUNCT_ADJACENT = "PUNCT-ADJACENT"
RULE_PUNCT_UNPAIRED = "PUNCT-UNPAIRED"

CORNER_REMEDY = (
    "rewrite per content: curly quotes (inner curly for nesting), inline"
    " code for cited literals and glyphs, or bold for emphasis"
)
ADJACENT_REMEDY = "use the fullwidth form, or move the token into inline code"
UNPAIRED_REMEDY = "complete, remove, or reorder so every bracket nests cleanly"

# 直角引号字符面：出现即罚；字符事实住在代码里，注释不写字形防自触
CORNERS = "「」『』"
# 半角邻接集：成员紧邻汉字即罚；连接号、省略号与间隔号有意不在集内
ADJACENT_HW = ",.:;?!()\"'-"
# 配对三对：栈顶类型校验；直角引号由出现即罚覆盖，不进配对栈
PAIRS = {"（": "）", "“": "”", "‘": "’"}
CLOSERS = {close: open_ for open_, close in PAIRS.items()}
# 文件名与章节号豁免：半角句点后接一至六位字母数字且到边界
EXTENSION_TAIL = re.compile(r"[A-Za-z0-9]{1,6}(?![A-Za-z0-9.])")

_SKIP_INLINE = frozenset({"code_inline", "math_inline", "html_inline", "image"})


def handles(rel: str) -> bool:
    """文件类型是否归本门禁：记录目录下的标记语言文档，与文档书写门禁互斥。"""
    if os.path.splitext(rel)[1] != ".md":
        return False
    return rel.startswith("docs" + os.sep + "comments" + os.sep)


def unit_text(children: list) -> str:
    """判定单元取文：递归收行内记号的文本面，代码与数学等剥除，软换行连空白。"""
    parts = []
    for node in children:
        kind = node.type
        if kind in _SKIP_INLINE:
            continue
        if kind in ("softbreak", "hardbreak"):
            parts.append(" ")
        elif kind == "text":
            parts.append(node.content)
        elif node.children:
            parts.append(unit_text(node.children))
    return "".join(parts)


def iter_units(src: str):
    """遍历判定单元：解析树里全部行内记号各成一单元，随带起始行号。"""
    for token in markdown_parser().parse(src):
        if token.type == "inline" and token.map is not None:
            yield token.map[0] + 1, unit_text(token.children or [])


def corner_problems(path: str, line: int, text: str) -> list[gate_report.Finding]:
    """直角引号出现即罚：一单元一条，字符去重保序进消息，列号取首处。"""
    seen: list[str] = []
    column = 0
    for index, ch in enumerate(text):
        if ch in CORNERS:
            if ch not in seen:
                seen.append(ch)
            if not column:
                column = index + 1
    if not seen:
        return []
    return [
        gate_report.Finding(
            rule=RULE_PUNCT_CORNER,
            path=path,
            line=line,
            message="corner quotation marks found (%s)" % " ".join(seen),
            evidence=text,
            remedy=CORNER_REMEDY,
            column=column,
        )
    ]


def adjacent_marks(text: str) -> tuple[list[str], int]:
    """半角邻接命中：成员紧邻汉字即收，句点按文件名与章节号形态放行。"""
    marks: list[str] = []
    column = 0
    for index, ch in enumerate(text):
        if ch not in ADJACENT_HW:
            continue
        prev = text[index - 1] if index else ""
        nxt = text[index + 1] if index + 1 < len(text) else ""
        if not ((prev and CJK.match(prev)) or (nxt and CJK.match(nxt))):
            continue
        if ch == "." and EXTENSION_TAIL.match(text[index + 1 :]):
            continue
        if ch not in marks:
            marks.append(ch)
        if not column:
            column = index + 1
    return marks, column


def adjacent_problems(path: str, line: int, text: str) -> list[gate_report.Finding]:
    marks, column = adjacent_marks(text)
    if not marks:
        return []
    return [
        gate_report.Finding(
            rule=RULE_PUNCT_ADJACENT,
            path=path,
            line=line,
            message="halfwidth marks join Chinese without conversion (%s)" % " ".join(marks),
            evidence=text,
            remedy=ADJACENT_REMEDY,
            column=column,
        )
    ]


def pairing_hits(text: str) -> tuple[list[str], int]:
    """配对命中：统一栈配三对定界符，查单闭、多开与交错，各带形态描述。"""
    hits: list[str] = []
    column = 0
    stack: list[tuple[str, int]] = []
    for index, ch in enumerate(text):
        if ch in PAIRS:
            stack.append((ch, index))
            continue
        if ch not in CLOSERS:
            continue
        opener = CLOSERS[ch]
        if stack and stack[-1][0] == opener:
            stack.pop()
            continue
        match_at = next((i for i in range(len(stack) - 1, -1, -1) if stack[i][0] == opener), None)
        if match_at is None:
            hits.append("orphan closer %s" % ch)
        else:
            hits.append("interleaved closer %s does not match opener %s" % (ch, stack[-1][0]))
            del stack[match_at]
        if not column:
            column = index + 1
    for opener, index in stack:
        hits.append("unclosed opener %s" % opener)
        if not column:
            column = index + 1
    return hits, column


def pairing_problems(path: str, line: int, text: str) -> list[gate_report.Finding]:
    hits, column = pairing_hits(text)
    if not hits:
        return []
    return [
        gate_report.Finding(
            rule=RULE_PUNCT_UNPAIRED,
            path=path,
            line=line,
            message="unpaired annotation brackets or quotes (%s)" % "; ".join(hits[:3]),
            evidence=text,
            remedy=UNPAIRED_REMEDY,
            column=column,
        )
    ]


def punct_problems(path: str, src: str) -> list[gate_report.Finding]:
    """判定入口：逐单元过三规则，输出带规则标识的违规条目。"""
    out: list[gate_report.Finding] = []
    for line, text in iter_units(src):
        if not text.strip():
            continue
        out += corner_problems(path, line, text)
        out += adjacent_problems(path, line, text)
        out += pairing_problems(path, line, text)
    return out


def check(rel: str, src: str) -> list[gate_report.Finding]:
    """路由入口：与注释纪律门禁的同名签名一致，判罚走统一诊断出口。"""
    return punct_problems(rel, src)


def scan_files(root: str, explicit: list[str]) -> list[str]:
    """枚举受检文件：显式清单优先（供单测传样本），否则取版本库跟踪清单。"""
    if explicit:
        return [p if os.path.isabs(p) else os.path.join(root, p) for p in explicit]
    out = gate_report.run_git(["ls-files", "-z"], cwd=root).stdout
    return [os.path.join(root, p.decode("utf-8", "replace")) for p in out.split(b"\0") if p]


def collect(root: str, explicit: list[str]) -> list[gate_report.Finding]:
    """遍历记录目录聚合违规：判罚即时打印，返回违规清单。"""
    paths = scan_files(root, explicit)
    if not explicit and not [p for p in paths if handles(os.path.relpath(p, root))]:
        gate_report.fail(
            gate_report.Finding(
                rule=gate_report.RULE_EMPTY_SURFACE,
                path="",
                line=0,
                message="the record punctuation scan surface is empty: no tracked files found",
                remedy="run the gate from the workspace root",
            )
        )
        sys.exit(gate_report.EXIT_INFRA)
    found: list[gate_report.Finding] = []
    for path in paths:
        rel = os.path.relpath(path, root)
        if not handles(rel):
            continue
        try:
            with open(path, encoding="utf-8") as fh:
                src = fh.read()
        except (OSError, UnicodeDecodeError) as exc:
            gate_report.fail(
                gate_report.infra_failure("cannot read tracked file: %s" % rel, evidence=str(exc))
            )
            sys.exit(gate_report.EXIT_INFRA)
        for finding in check(rel, src):
            found.append(gate_report.fail(finding))
    return found


def usage_failure(detail: str) -> int:
    print("usage: check_record_punct.py [repo-root] [file ...]", file=sys.stderr)
    print("  %s" % detail, file=sys.stderr)
    return gate_report.EXIT_USAGE


def main() -> int:
    root = (
        sys.argv[1]
        if len(sys.argv) > 1
        else os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
    )
    explicit = sys.argv[2:]
    if len(sys.argv) > 1 and not os.path.isdir(root):
        return usage_failure("repo-root is not a directory: %s" % root)
    gate_report.banner("record punctuation gate", "repo root: %s" % root)
    found = collect(root, explicit)
    if not found:
        print("  ok: record punctuation is clean in the record directory")
    if found:
        gate_report.finish(
            found,
            "  rules: see docs/comments/gate_rules_comment.md for the punct domain",
        )
    return gate_report.EXIT_VIOLATION if found else gate_report.EXIT_PASS


if __name__ == "__main__":
    sys.exit(main())
