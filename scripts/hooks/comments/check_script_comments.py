#!/usr/bin/env python3
"""注释纪律门禁·脚本与配置：井号系脚本与配置、钩子目录脚本、任务配方文
件、忽略清单与站点资源的注释判罚——主体全中文，专名只进括号注记，路径、
命令、旗标、标识符与链接都算代码引用一并判罚；相邻注释行聚成一块受检，
换行的括号与表述跨行不误报；工具指令行、解释器声明与放行标记豁免，脚本
文件头统一写成模块级文档字符串，判罚与注释同规。字面数据不误报：壳层脚
本的就地文档正文、配置（YAML）里非运行键的块标量正文整行豁免，脚本资源
里跨行的模板字面量按串跳过。由注释门禁路由调用，与语言规范库同源共用；
本模块自身注释同规。
"""

from __future__ import annotations

import ast
import io
import os
import re
import sys
import tokenize
from typing import AbstractSet

_HOOKS = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if _HOOKS not in sys.path:
    sys.path.insert(0, _HOOKS)

from language_spec.chinese import CJK, DIRECTIVE_SEG, SEGMENT_SPLIT, SEGMENT_TRIM, body_problems  # noqa: E402
from language_spec.coderef import MARKER  # noqa: E402

# 源码之外的注释文件：井号系脚本与配置、钩子目录脚本、任务配方文件与忽略清单。
HASH_SUFFIXES = {".py", ".toml", ".yml", ".yaml", ".sh"}
HASH_NAMES = {"justfile", ".gitignore"}
# 站点资源：脚本用双斜线与斜线星注释，样式用斜线星，页面用标记注释。
WEB_SUFFIXES = {".js", ".css", ".html"}


def handles(rel: str) -> bool:
    """文件类型是否归本门禁：井号系脚本与配置、钩子目录脚本与站点资源。"""
    name = os.path.basename(rel)
    ext = os.path.splitext(name)[1]
    return (
        ext in HASH_SUFFIXES
        or name in HASH_NAMES
        or rel.startswith(".githooks/")
        or ext in WEB_SUFFIXES
    )


def hash_comments(
    src: str, line_start_only: bool = False, skip: AbstractSet[int] = frozenset()
) -> list[tuple[int, str]]:
    """井号系注释抽取：引号感知；井号须在行首或空白之后；多行字符串整段跳过。

    skip 里的行整行豁免——块标量正文与 heredoc 正文是字面数据，井号不是注释。"""
    out = []
    in_ml = None
    for lineno, line in enumerate(src.splitlines(), 1):
        if lineno in skip:
            in_ml = None  # 字面数据行不延续串状态
            continue
        j, n = 0, len(line)
        in_str = None
        comment_at = None
        while j < n:
            if in_ml:
                k = line.find(in_ml, j)
                if k == -1:
                    j = n
                else:
                    in_ml = None
                    j = k + 3
                continue
            c = line[j]
            if in_str:
                if c == "\\" and in_str == '"':
                    j += 2
                    continue
                if c == in_str:
                    in_str = None
                j += 1
                continue
            if c in "\"'":
                if line.startswith(c * 3, j):
                    in_ml = c * 3
                    j += 3
                    continue
                in_str = c
                j += 1
                continue
            if c == "#" and (j == 0 or line[j - 1] in " \t"):
                if line_start_only and j != 0:
                    break
                comment_at = j
                break
            j += 1
        if comment_at is not None:
            out.append((lineno, line[comment_at:]))
    return out


def python_comments(src: str, skip: AbstractSet[int] = frozenset()) -> list[tuple[int, str]]:
    """注释走标准词法（Python）取准确集合；语法异常时退回井号扫描。"""
    try:
        out = []
        for tok in tokenize.generate_tokens(io.StringIO(src).readline):
            if tok.type == tokenize.COMMENT:
                out.append((tok.start[0], tok.string))
        return out
    except (tokenize.TokenError, IndentationError, SyntaxError):
        return hash_comments(src, skip=skip)


# 块标量头：键、竖线或折行号与缩进指示；运行键的正文是壳层脚本，注释照查
YAML_BLOCK_RE = re.compile(r"^(\s*)([\w.-]+):\s*[|>][0-9+-]*\s*(?:#.*)?$")


def yaml_scalar_skip(src: str) -> set[int]:
    """配置（YAML）块标量正文行清单：除运行键（承载壳层脚本的键）外，正
    文的井号是字面内容不是注释。

    块必须有收口（更浅缩进的实质行）才生效——未收口的疑似块头不豁免，宁误
    报不漏检。"""
    skip = set()
    lines = src.splitlines()
    i = 0
    while i < len(lines):
        m = YAML_BLOCK_RE.match(lines[i])
        if not m or m.group(2) == "run":
            i += 1
            continue
        key_indent = len(m.group(1))
        k = i + 1
        while k < len(lines):
            if lines[k].strip() and len(lines[k]) - len(lines[k].lstrip(" ")) <= key_indent:
                break
            k += 1
        if k < len(lines):
            skip.update(range(i + 2, k + 1))
        i = max(k, i + 1)
    return skip


def heredoc_skip(src: str) -> set[int]:
    """壳层脚本就地文档正文行清单：定界符收口之前的行是字面数据。

    定界符须真的出现才生效——未收口的疑似起始不豁免，宁误报不漏检。"""
    skip = set()
    lines = src.splitlines()
    i = 0
    while i < len(lines):
        line = lines[i]
        j, n = 0, len(line)
        in_str = None
        tag = None
        while j < n:
            c = line[j]
            if in_str:
                if c == in_str:
                    in_str = None
                j += 1
                continue
            if c in "\"'":
                in_str = c
                j += 1
                continue
            m = re.match(r"<<-?\s*([A-Za-z_][A-Za-z0-9_]*)", line[j:])
            if m:
                tag = m.group(1)
                break
            j += 1
        if tag:
            k = i + 1
            while k < len(lines) and lines[k].strip() != tag:
                k += 1
            if k < len(lines):
                skip.update(range(i + 2, k + 1))
            i = k
        i += 1
    return skip


def web_comments(src: str, suffix: str) -> list[tuple[int, str]]:
    """站点资源注释抽取：页面认标记注释（引号感知），脚本与样式认双斜线与
    斜线星注释；脚本的反引号模板字面量可跨行，串状态随行保留。"""
    out = []
    closer = None
    in_tpl = False  # 模板字面量未闭：后续行整行仍在串内
    for lineno, line in enumerate(src.splitlines(), 1):
        j, n = 0, len(line)
        in_str = "`" if in_tpl else None
        while j < n:
            if closer:
                k = line.find(closer, j)
                if k == -1:
                    out.append((lineno, line))
                    break
                out.append((lineno, line[: k + len(closer)]))
                j = k + len(closer)
                closer = None
                continue
            c = line[j]
            if suffix == ".html":
                if in_str:
                    if c == in_str:
                        in_str = None
                    j += 1
                    continue
                if c in "\"'":
                    in_str = c
                    j += 1
                    continue
                if line.startswith("<!--", j):
                    k = line.find("-->", j + 4)
                    if k == -1:
                        out.append((lineno, line[j:]))
                        closer = "-->"
                        break
                    out.append((lineno, line[j : k + 3]))
                    j = k + 3
                    continue
                j += 1
                continue
            if in_str:
                if c == "\\" and in_str in '"`':
                    j += 2
                    continue
                if c == in_str:
                    in_str = None
                j += 1
                continue
            if c in "\"'`":
                in_str = c
                j += 1
                continue
            if c == "/" and j + 1 < n:
                nxt = line[j + 1]
                if nxt == "/" and suffix != ".css":
                    out.append((lineno, line[j:]))
                    break
                if nxt == "*":
                    k = line.find("*/", j + 2)
                    if k == -1:
                        out.append((lineno, line[j:]))
                        closer = "*/"
                        break
                    out.append((lineno, line[j : k + 2]))
                    j = k + 2
                    continue
            j += 1
        in_tpl = suffix == ".js" and in_str == "`"
    return out


def web_body(text: str) -> str:
    """剥去站点注释的包裹符，取纯表述。"""
    body = text.strip()
    for lead in ("<!--", "/*", "//"):
        if body.startswith(lead):
            body = body[len(lead) :]
            break
    body = body.lstrip("*").strip()
    for tail in ("-->", "*/"):
        if body.endswith(tail):
            body = body[: -len(tail)]
            break
    return body.strip()


def module_docstring_problems(rel: str, src: str) -> list[str]:
    """模块级文档字符串按文件头同规：内容走注释同款判罚，解释器引导串豁免。

    文档字符串是字面量不是注释，词法抽取收不到它；这里按语法树补一刀，
    防止文件头换一种写法就逃出纪律。文件头取开头连续的字符串语句：跨壳
    脚本先有解释器引导串（豁免、跳过），其后紧跟真正的文件头；遇到首个
    非字符串语句即止。语法异常的退回路径本就只管井号注释，此处一并跳过。
    """
    try:
        tree = ast.parse(src)
    except SyntaxError:
        return []
    out = []
    for node in tree.body:
        if not (
            isinstance(node, ast.Expr)
            and isinstance(node.value, ast.Constant)
            and isinstance(node.value.value, str)
        ):
            break
        text = node.value.value
        if re.search(r"exec\s+python", text):
            continue  # 解释器引导串：跨壳脚本的井号叹号替身，同解释器声明豁免
        loc = f"{rel}:{node.lineno}"
        if "`" in text:
            out.append(
                f"{loc}: file header contains a code segment"
                " — move references and examples to the record file"
            )
        elif "§" in text:
            out.append(
                f'{loc}: file header contains a "§ section number" style external reference'
                " — move sources to the record file"
            )
        elif not CJK.search(text):
            out.append(f"{loc}: file header must be written in Chinese: {text[:40]}")
        else:
            out += body_problems(loc, text, "file header")
        break  # 只看文件头：首个非豁免字符串语句之后不再看
    return out


def check(rel: str, src: str) -> list[str]:
    """注释逐块受检：相邻注释行聚成一块，模块级文档字符串按文件头同规。"""
    name = os.path.basename(rel)
    ext = os.path.splitext(name)[1]
    python_kind = ext == ".py" or rel.startswith(".githooks/")
    if name in HASH_NAMES:
        skip = heredoc_skip(src) if name == "justfile" else frozenset()
        pairs = hash_comments(src, line_start_only=(name == ".gitignore"), skip=skip)
        kind = "hash"
    elif python_kind:
        skip = frozenset() if ext == ".py" else heredoc_skip(src)
        pairs = python_comments(src, skip)
        kind = "hash"
    elif ext in HASH_SUFFIXES:
        if ext in {".yml", ".yaml"}:
            skip = yaml_scalar_skip(src)
        elif ext == ".sh":
            skip = heredoc_skip(src)
        else:
            skip = frozenset()
        pairs = hash_comments(src, skip=skip)
        kind = "hash"
    else:
        pairs = web_comments(src, ext)
        kind = "web"

    def flush(lineno: int, bodies: list[str]) -> list[str]:
        merged = "\n".join(bodies)
        loc = f"{rel}:{lineno}"
        if "`" in merged:
            return [
                f"{loc}: comment contains a code segment"
                " — move references and examples to the record file"
            ]
        if "§" in merged:
            return [
                f'{loc}: comment contains a "§ section number" style external reference'
                " — move sources to the record file"
            ]
        if not CJK.search(merged):
            return [f"{loc}: comment must be written in Chinese: {merged[:40]}"]
        return body_problems(loc, merged, "comment")

    out = []
    cur = None  # 相邻注释行聚成一块：换行的括号与表述跨行不误报
    for lineno, text in pairs:
        exempt = MARKER.search(text) or text.startswith("#!")
        body = None
        if not exempt:
            body = web_body(text) if kind == "web" else text[1:].strip()
            segs = [seg.strip(SEGMENT_TRIM) for seg in SEGMENT_SPLIT.split(body)]
            segs = [seg for seg in segs if seg]
            if not segs or all(DIRECTIVE_SEG.match(seg) for seg in segs):
                exempt = True  # 纯装饰行与纯指令行不判罚，也隔断聚块
        if exempt:
            if cur is not None:
                out += flush(*cur)
                cur = None
            continue
        if cur is not None and cur[0] + len(cur[1]) == lineno:
            cur[1].append(body)
        else:
            if cur is not None:
                out += flush(*cur)
            cur = (lineno, [body])
    if cur is not None:
        out += flush(*cur)
    if python_kind:
        out += module_docstring_problems(rel, src)
    return out
