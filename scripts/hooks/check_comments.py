#!/usr/bin/env python3
"""注释纪律门禁：跟踪文件的注释主体只写中文——不混任何外文字母与半角标点，
专有名词一律收进紧跟的全角括号作注记；不变量注释整段纯英文。本地注释配方
（提交前门禁的一部分）与持续集成的钩子工作流共用此脚本，保证本地与远端判
定一致；本脚本自身注释同规。

书写判罚的公共实现沉淀为独立的书写规则模块，提交信息与拉取请求内容门禁
同源共用；本脚本只负责各文件形态的注释抽取与文件面策略。

源码（Rust）侧覆盖六条检查：
  只留文件头      —— 文档注释、行内、行尾与块注释一律违规；内部头注释只能
                     出现在文件头（首个代码项之前）。
  文件头必须存在  —— 每个被跟踪的源码文件至少一行头注释。
  文件头是中文    —— 每行必须含汉字；主体只允许汉字、中文标点、阿拉伯数字
                     与空白，不出现外文字母或半角标点；专有名词收进紧跟的全
                     角括号作注记，括号内只放白名单里的专名本身（可以空格、
                     半角逗号或斜杠分隔多个），括号内外都不得混入第二种语
                     言。外部文档引用不进文件头——章节号随文档漂移，文档名
                     是本地指针。
  注记括号成对    —— 全角括号必须成对包裹注记且不嵌套，单边或套用判罚。
  不变量注释纯英文 —— 不变量块及其紧邻的连续注释行紧贴不安全块、整段英文
                     书写（静态检查工具要求不变量紧贴不安全块）。
  行尾豁免标记    —— 各门禁的放行标记照旧放行所在行。

源码（Rust）之外的文件同规覆盖：脚本、配置、工作流与站点资源的注释逐块同
规——主体全中文、专名只进括号注记、标点不混用；路径、命令、旗标、标识符与
链接都算代码引用，一并判罚。相邻注释行聚成一块受检，换行的括号与表述跨行
不误报。工具指令行豁免：解释器声明、格式化与静态检查指令、行尾豁免标记。
井号注释、双斜线注释、块注释与页面标记注释都纳入；脚本文件头统一写成
模块级文档字符串，判罚与注释同规（解释器引导串豁免）。标记语言（Markdown）
文档逐行受检——自述、说明与测试清单同一套豁免：代码围栏、行内反引号与链
接、表格首列与纯标识符小节标题（双向核对的键）以及日期；注释记录目录与
提示词目录不在受检面——前者是注释的住处，后者是模型功能资产（同数据集
夹具）。

词法层区分注释与字符串字面量：字符串内的注释符（如链接）不误报。

用法：以解释器运行本脚本，首参为仓库根，其余参数为可选的文件清单（供单测
传入临时样本）；缺省扫全部被跟踪文件。
"""

import ast
import io
import os
import re
import subprocess
import sys
import tokenize

from prose_rules import (
    CJK,
    CJKWISE,
    DIRECTIVE_SEG,
    MARKER,
    SEGMENT_SPLIT,
    SEGMENT_TRIM,
    body_problems,
    markdown_problems,
)

# 源码之外的注释文件：井号系脚本与配置、钩子目录脚本、任务配方文件与忽略清单。
HASH_SUFFIXES = {".py", ".toml", ".yml", ".yaml", ".sh"}
HASH_NAMES = {"justfile", ".gitignore"}
# 站点资源：脚本用双斜线与斜线星注释，样式用斜线星，页面用标记注释。
WEB_SUFFIXES = {".js", ".css", ".html"}


def is_comment_candidate(rel):
    name = os.path.basename(rel)
    ext = os.path.splitext(name)[1]
    return (
        ext in HASH_SUFFIXES
        or name in HASH_NAMES
        or rel.startswith(".githooks/")
        or ext in WEB_SUFFIXES
        or is_md_doc(rel)
    )


def is_md_doc(rel):
    """标记语言（Markdown）文档逐行受检，核对键豁免；注释记录目录与提示
    词目录除外——前者是注释的住处，后者是模型功能资产。"""
    if ext_of(rel) != ".md":
        return False
    if rel.startswith("docs" + os.sep + "comments" + os.sep):
        return False
    return "prompts" not in rel.split(os.sep)


def ext_of(rel):
    return os.path.splitext(os.path.basename(rel))[1]


def hash_comments(src, line_start_only=False):
    """井号系注释抽取：引号感知；井号须在行首或空白之后；多行字符串整段跳过。"""
    out = []
    in_ml = None
    for lineno, line in enumerate(src.splitlines(), 1):
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


def python_comments(src):
    """注释走标准词法（Python）取准确集合；语法异常时退回井号扫描。"""
    try:
        out = []
        for tok in tokenize.generate_tokens(io.StringIO(src).readline):
            if tok.type == tokenize.COMMENT:
                out.append((tok.start[0], tok.string))
        return out
    except (tokenize.TokenError, IndentationError, SyntaxError):
        return hash_comments(src)


def web_comments(src, suffix):
    """站点资源注释抽取：页面认标记注释，脚本与样式认双斜线与斜线星注释。"""
    out = []
    closer = None
    for lineno, line in enumerate(src.splitlines(), 1):
        j, n = 0, len(line)
        in_str = None
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
    return out


def web_body(text):
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


def module_docstring_problems(rel, src):
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


def nonrust_violations(rel, src):
    name = os.path.basename(rel)
    ext = os.path.splitext(name)[1]
    python_kind = ext == ".py" or rel.startswith(".githooks/")
    if is_md_doc(rel):
        return markdown_problems(rel, src)
    if name in HASH_NAMES:
        pairs = hash_comments(src, line_start_only=(name == ".gitignore"))
        kind = "hash"
    elif python_kind:
        pairs = python_comments(src)
        kind = "hash"
    elif ext in HASH_SUFFIXES:
        pairs = hash_comments(src)
        kind = "hash"
    else:
        pairs = web_comments(src, ext)
        kind = "web"

    def flush(lineno, bodies):
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


def scan_files(root, explicit):
    if explicit:
        return [p if os.path.isabs(p) else os.path.join(root, p) for p in explicit]
    out = subprocess.run(
        ["git", "-C", root, "ls-files"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    return [os.path.join(root, p) for p in out.splitlines() if p]


def run(root, explicit):
    problems = []

    def lex(src):
        """按字符扫描，产出注释事件：序号、起止行、种类、行首空白、是否在文件头前与原文。"""
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

        while i < n:
            c = src[i]
            if c == "\n":
                i += 1
                line_starts_blank = True
                inner_attr_only = True
                continue
            if c in " \t\r":
                i += 1
                continue
            if c == "/" and i + 1 < n and src[i + 1] == "/":
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
                        line_of(j) if j < n else line_of(n - 1),
                        kind,
                        indent_only,
                        before_code,
                        body,
                    )
                )
                seq += 1
                i = j
                continue
            if c == "/" and i + 1 < n and src[i + 1] == "*":
                start = i
                before_code = code_seen
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
                end = min(i, n - 1)
                events.append(
                    (
                        seq,
                        line_of(start),
                        line_of(end),
                        "block",
                        line_starts_blank,
                        before_code,
                        src[start:i],
                    )
                )
                seq += 1
                line_starts_blank = True
                inner_attr_only = True
                continue
            if c == '"':
                inner_attr_only = False
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
                    inner_attr_only = False
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
                m = re.match(r"'(\\.[^']*|[^'\\])'", src[i : i + 8])
                if m:
                    inner_attr_only = False
                    i += m.end()
                    continue
                i += 1  # 生命周期标注不是字面量
                continue
            if c == "#" and inner_attr_only and src[i : i + 3] == "#![":
                # 内部属性声明：字符串感知地跳到配对方括号，不视为代码项
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
            i += 1

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

    for path in scan_files(root, explicit):
        rel = os.path.relpath(path, root)
        if not rel.endswith(".rs"):
            if is_comment_candidate(rel):
                try:
                    with open(path, encoding="utf-8") as fh:
                        plain = fh.read()
                except (OSError, UnicodeDecodeError) as exc:
                    problems.append(f"{rel}: unreadable ({exc})")
                    continue
                problems.extend(nonrust_violations(rel, plain))
            continue
        try:
            with open(path, encoding="utf-8") as fh:
                src = fh.read()
        except (OSError, UnicodeDecodeError) as exc:
            problems.append(f"{rel}: unreadable ({exc})")
            continue
        events = lex(src)
        header_lines = []
        header_found = False
        safety_open = False
        prev_end = None
        for _seq, sline, eline, kind, indent_only, before_code, text in events:
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
                    f"{rel}:{sline}: /// doc comment"
                    " — move the content to the record file, then delete"
                )
                continue
            if kind == "block":
                problems.append(
                    f"{rel}:{sline}: block comment"
                    " — move the content to the record file, then delete"
                )
                continue
            # 此处余下的都是行内注释
            if MARKER.search(text):
                continue
            if not indent_only:
                problems.append(
                    f"{rel}:{sline}: trailing comment — move to the record file;"
                    " allow markers exempt"
                )
                continue
            stripped = text.strip()
            body = stripped.lstrip("/").strip()
            if safety_open or body.startswith("SAFETY:"):
                safety_open = True
                if CJKWISE.search(body):
                    problems.append(
                        f"{rel}:{sline}: SAFETY comment must be pure English"
                        " — rewrite the whole invariant block in English,"
                        " keep symbol references"
                    )
                continue
            problems.append(
                f"{rel}:{sline}: inline comment — move to the record file"
                " (SAFETY and :allow exempt)"
            )
        if not header_found:
            problems.append(f"{rel}: missing file header //! comment")
        problems.extend(header_violations(header_lines, rel))
    return problems


def main():
    root = (
        sys.argv[1]
        if len(sys.argv) > 1
        else os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    )
    explicit = sys.argv[2:]
    print(f"== comment discipline gate (repo root: {root}) ==")
    problems = run(root, explicit)
    if problems:
        for p in problems:
            print(f"  ✗ {p}", file=sys.stderr)
        print(f"✗ comment discipline gate: {len(problems)} violations", file=sys.stderr)
        print(
            "  rules: see this script's header; move target docs/comments/<file>_comment.md",
            file=sys.stderr,
        )
        sys.exit(1)
    print(
        "  ok comment discipline: sources keep only Chinese file headers, comment"
        " bodies fully Chinese, proper nouns only in bracket annotations, invariant"
        " comments pure English"
    )


if __name__ == "__main__":
    main()
