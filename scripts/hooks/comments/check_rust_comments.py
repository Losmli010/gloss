#!/usr/bin/env python3
"""注释纪律门禁·源码：本语言（Rust）源文件的注释判罚——只留中文文件头，文
档注释、行内注释、行尾注释与块注释一律不进源码；文件头必须存在、逐行含汉
字、主体全中文，不写代码引用与外部文档指引；不变量注释整段纯英文且须紧邻
不安全块（门禁校验邻接），放行标记豁免。词法层区分注释与字符串字面量，字
符串内的注释符不误报。由注释门禁路由调用，与语言规范库同源共用；本模块自
身注释同规。
"""

import os
import re
import sys

_HOOKS = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if _HOOKS not in sys.path:
    sys.path.insert(0, _HOOKS)

from language_spec.chinese import CJK, body_problems  # noqa: E402
from language_spec.coderef import MARKER  # noqa: E402
from language_spec.english import english_zone_problems  # noqa: E402


def handles(rel):
    """文件类型是否归本门禁：本语言（Rust）源文件。"""
    return rel.endswith(".rs")


def lex(src):
    """按字符扫描，产出注释事件：序号、起止行、种类、行首空白、是否在文件头
    前、紧随代码是否含不安全块与原文。紧随代码指注释后到空行、下一注释或文
    件尾之间的代码——不变量注释的邻接判定以它为准。"""
    newlines = [m.start() for m in re.finditer("\n", src)]

    def line_of(i):
        import bisect

        return bisect.bisect_right(newlines, i) + 1

    events = []
    i, n = 0, len(src)
    seq = 0
    code_seen = False  # 是否已越过文件头（见过代码项）
    line_starts_blank = True  # 当前行目前只有空白
    inner_attr_only = True  # 当前行目前只有内部属性声明（不算代码项）
    line_has_content = False  # 当前行是否已有注释或代码（空行判定，注释不算代码）
    word = []  # 当前代码词符（字母数字下划线，串与标点即断）
    run_unsafe = False  # 紧随上一条注释的代码段是否含不安全块
    pending = None  # 等待邻接判定的注释事件下标

    def settle():
        """把已积累的代码段判给等待中的注释；无等待者时弃段。"""
        nonlocal pending, run_unsafe
        if pending is not None:
            old = events[pending]
            events[pending] = (*old[:6], run_unsafe, old[7])
            pending = None
        run_unsafe = False

    def take_word():
        """收一个代码词符：是不安全块则记入当前代码段。"""
        nonlocal run_unsafe
        if word and "".join(word) == "unsafe":
            run_unsafe = True
        word.clear()

    while i < n:
        c = src[i]
        if c == "\n":
            i += 1
            take_word()
            if not line_has_content:
                settle()  # 空行是代码段边界：不安全块隔空行即不算紧随
            line_starts_blank = True
            inner_attr_only = True
            line_has_content = False
            continue
        if c in " \t\r":
            take_word()
            i += 1
            continue
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            take_word()
            settle()
            start = i
            indent_only = line_starts_blank
            before_code = code_seen
            j = i + 2
            while j < n and src[j] != "\n":
                j += 1
            body = src[i:j]
            if body.startswith("//!"):
                kind = "inner"
            elif body.startswith("///"):
                kind = "doc"
            else:
                kind = "line"
            events.append(
                (
                    seq,
                    line_of(start),
                    line_of(j - 1),
                    kind,
                    indent_only,
                    before_code,
                    False,
                    body,
                )
            )
            seq += 1
            pending = len(events) - 1
            line_has_content = True
            i = j
            continue
        if c == "/" and i + 1 < n and src[i + 1] == "*":
            take_word()
            settle()
            start = i
            before_code = code_seen
            saved_blank = line_starts_blank
            saved_attr = inner_attr_only
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
            end = i - 1
            events.append(
                (
                    seq,
                    line_of(start),
                    line_of(end),
                    "block",
                    saved_blank,
                    before_code,
                    False,
                    src[start:i],
                )
            )
            seq += 1
            pending = len(events) - 1
            line_starts_blank = saved_blank
            inner_attr_only = saved_attr
            line_has_content = True
            continue
        if c == '"':
            take_word()
            inner_attr_only = False
            line_starts_blank = False
            line_has_content = True
            i += 1
            while i < n:
                if src[i] == "\\":
                    i += 2
                elif src[i] == '"':
                    i += 1
                    break
                else:
                    i += 1
            continue
        if c in "rb" and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")):
            # 原始字符串与字节串前缀按井号界配对跳过
            k = i + 1
            if c == "b" and k < n and src[k] == "r":
                k += 1
            hashes = 0
            while k < n and src[k] == "#":
                hashes += 1
                k += 1
            if k < n and src[k] == '"':
                take_word()
                inner_attr_only = False
                line_starts_blank = False
                line_has_content = True
                i = k + 1
                while i < n:
                    if src[i] == '"':
                        m = 0
                        while m < hashes and i + 1 + m < n and src[i + 1 + m] == "#":
                            m += 1
                        if m == hashes:
                            i += 1 + hashes
                            break
                    i += 1
                continue
        if c == "'":
            m = re.match(r"'(\\.[^']*|[^'\\])'", src[i : i + 16])
            if m:
                take_word()
                inner_attr_only = False
                line_starts_blank = False
                line_has_content = True
                i += m.end()
                continue
            take_word()  # 生命周期标注不是字面量
            line_starts_blank = False
            line_has_content = True
            i += 1
            continue
        if c == "#" and inner_attr_only and src[i : i + 3] == "#![":
            # 内部属性声明：字符串感知地跳到配对方括号，不视为代码项
            take_word()
            line_starts_blank = False
            line_has_content = True
            depth = 1
            i += 3
            while i < n and depth:
                if src[i] == '"':
                    i += 1
                    while i < n and src[i] != '"':
                        i += 2 if src[i] == "\\" else 1
                    i += 1
                    continue
                if src[i] == "[":
                    depth += 1
                elif src[i] == "]":
                    depth -= 1
                i += 1
            continue
        # 其余一概视为代码
        inner_attr_only = False
        line_starts_blank = False
        code_seen = True
        line_has_content = True
        if c.isalnum() or c == "_":
            word.append(c)
        else:
            take_word()
        if c == ";":
            settle()  # 语句边界：不安全块隔一条完整语句即不算紧邻
        i += 1

    settle()
    return events


def header_violations(lines, rel):
    out = []
    merged = []
    for text in lines:
        body = text[3:].strip() if text.startswith("//!") else text.strip()
        if not body:
            continue
        if not CJK.search(body):
            out.append(f"{rel}: file header line without Chinese characters: {body[:60]}")
        if "`" in body:
            out.append(
                f"{rel}: file header contains a backtick code segment"
                " — move code references out of the header into the record file"
            )
        if "§" in body:
            out.append(
                f'{rel}: file header contains a "§ section number" style external'
                " reference — section numbers drift with documents;"
                " move sources to the record file"
            )
        merged.append(body.replace("`", " ").replace("§", " "))
    if merged:
        out += body_problems(rel, "\n".join(merged), "file header")
    return out


def block_tail_unsafe(events):
    """每条行注释取所在连续块最后一条的紧随判定：不变量注释的豁免看块尾。"""
    tail = [e[6] for e in events]
    block = []
    for idx, ev in enumerate(events):
        contiguous = ev[3] == "line" and block and ev[1] == events[block[-1]][2] + 1
        if contiguous:
            block.append(idx)
        else:
            value = events[block[-1]][6] if block else False
            for k in block:
                tail[k] = value
            block = [idx] if ev[3] == "line" else []
    value = events[block[-1]][6] if block else False
    for k in block:
        tail[k] = value
    return tail


def check(rel, src):
    problems = []
    events = lex(src)
    tail = block_tail_unsafe(events)
    header_lines = []
    header_found = False
    safety_open = False
    prev_end = None
    for idx, ev in enumerate(events):
        _seq, sline, eline, kind, indent_only, before_code, _after, text = ev
        if prev_end is not None and sline > prev_end + 1:
            safety_open = False  # 与上一条注释之间有代码或空行
        prev_end = eline
        if kind == "inner":
            if before_code:
                problems.append(
                    f"{rel}:{sline}: //! after the file header — only the header may use //!"
                )
            else:
                header_found = True
                header_lines.append(text)
            continue
        if kind == "doc":
            problems.append(
                f"{rel}:{sline}: /// doc comment — move the content to the record file, then delete"
            )
            continue
        if kind == "block":
            problems.append(
                f"{rel}:{sline}: block comment — move the content to the record file, then delete"
            )
            continue
        # 此处余下的都是行内注释
        if MARKER.search(text):
            continue
        if not indent_only:
            problems.append(
                f"{rel}:{sline}: trailing comment — move to the record file; allow markers exempt"
            )
            continue
        stripped = text.strip()
        body = stripped.lstrip("/").strip()
        if safety_open or body.startswith("SAFETY:"):
            if body.startswith("SAFETY:") and not tail[idx]:
                problems.append(
                    f"{rel}:{sline}: SAFETY comment not adjacent to an unsafe block"
                    " — the exemption holds only directly above one"
                )
                continue
            safety_open = True
            problems += english_zone_problems(f"{rel}:{sline}", body, "SAFETY comment")
            continue
        problems.append(
            f"{rel}:{sline}: inline comment — move to the record file (SAFETY and :allow exempt)"
        )
    if not header_found:
        problems.append(f"{rel}: missing file header //! comment")
    problems.extend(header_violations(header_lines, rel))
    return problems
