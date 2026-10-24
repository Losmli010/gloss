#!/usr/bin/env python3
# 注释纪律门禁：跟踪文件的注释主体只写中文——不混任何外文字母与半角标点，
# 专有名词一律收进紧跟的全角括号作注记；不变量注释整段纯英文。本地注释配方
# （提交前门禁的一部分）与持续集成的钩子工作流共用此脚本，保证本地与远端判
# 定一致；本脚本自身注释同规。
#
# 源码（Rust）侧覆盖六条检查：
#   只留文件头      —— 文档注释、行内、行尾与块注释一律违规；内部头注释只能
#                      出现在文件头（首个代码项之前）。
#   文件头必须存在  —— 每个被跟踪的源码文件至少一行头注释。
#   文件头是中文    —— 每行必须含汉字；主体只允许汉字、中文标点、阿拉伯数字
#                      与空白，不出现外文字母或半角标点；专有名词收进紧跟的全
#                      角括号作注记，括号内只放白名单里的专名本身（可以空格、
#                      半角逗号或斜杠分隔多个），括号内外都不得混入第二种语
#                      言。外部文档引用不进文件头——章节号随文档漂移，文档名
#                      是本地指针。
#   注记括号成对    —— 全角括号必须成对包裹注记且不嵌套，单边或套用判罚。
#   不变量注释纯英文 —— 不变量块及其紧邻的连续注释行紧贴不安全块、整段英文
#                      书写（静态检查工具要求不变量紧贴不安全块）。
#   行尾豁免标记    —— 各门禁的放行标记照旧放行所在行。
#
# 源码（Rust）之外的文件同规覆盖：脚本、配置、工作流与站点资源的注释逐块同
# 规——主体全中文、专名只进括号注记、标点不混用；路径、命令、旗标、标识符与
# 链接都算代码引用，一并判罚。相邻注释行聚成一块受检，换行的括号与表述跨行
# 不误报。工具指令行豁免：解释器声明、格式化与静态检查指令、行尾豁免标记。
# 井号注释、双斜线注释、块注释与页面标记注释都纳入；脚本的模块级文档字符串
# 按文件头同规（解释器引导串豁免），文件头不得借字符串形态逃出纪律。
#
# 词法层区分注释与字符串字面量：字符串内的注释符（如链接）不误报。
#
# 用法：以解释器运行本脚本，首参为仓库根，其余参数为可选的文件清单（供单测
# 传入临时样本）；缺省扫全部被跟踪文件。

import ast
import io
import os
import re
import subprocess
import sys
import tokenize

CJK = re.compile(r"[\u4e00-\u9fff]")
LATIN = re.compile(r"[A-Za-z]")
# 汉字、中文标点与全角形式：注记括号内出现即算混语，不变量注释出现即非纯英文
CJKWISE = re.compile(r"[\u4e00-\u9fff\u3000-\u303f\uff00-\uffef\u2018\u2019\u201c\u201d]")
MARKER = re.compile(r"\b(?:secrets|lines|i18n|constraints|deps):allow\b")

# 注记括号允许的专有名词白名单：技术、产品、格式、协议、框架、工具与包名。
# 新增专名在此登记；标识符、路径、旗标、链接与代码段一律不进注释，沉记录文件。
PROPER_NOUNS = frozenset(
    {
        "4xx",
        "AGPL",
        "AI",
        "AMD",
        "API",
        "ASCII",
        "AX",
        "Actions",
        "AppKit",
        "Apple",
        "ApplicationServices",
        "BDD",
        "BEGIN",
        "Base64",
        "C",
        "CD",
        "CEF",
        "CF",
        "CFRunLoop",
        "CI",
        "CJK",
        "CLI",
        "CPU",
        "CVC",
        "Carbon",
        "Cargo",
        "Chrome",
        "Chromium",
        "Clippy",
        "Cmd",
        "Cmd+C",
        "Commits",
        "Conventional",
        "CoreFoundation",
        "CoreGraphics",
        "CoreText",
        "Ctrl+C",
        "DMG",
        "Dart",
        "Data",
        "Dependabot",
        "Dock",
        "Electron",
        "Elixir",
        "FFI",
        "FT",
        "GPL",
        "GPU",
        "GUI",
        "Gecko",
        "GitHub",
        "Gloss",
        "HEAD",
        "HIServices",
        "HIToolbox",
        "HTML",
        "HTTP",
        "HTTPS",
        "IO",
        "Intel",
        "JS",
        "JSON",
        "JSONL",
        "JavaScript",
        "Julia",
        "KILL",
        "Kotlin",
        "L1",
        "L2",
        "L3",
        "L4",
        "LGPL",
        "LLM",
        "LRU",
        "LaunchServices",
        "Luhn",
        "MPMC",
        "Markdown",
        "Metal",
        "Miri",
        "NVIDIA",
        "OpenAI",
        "PEM",
        "PNG",
        "PR",
        "Pages",
        "Pasteboard",
        "Prompt",
        "Python",
        "RGBA",
        "RSS",
        "Release",
        "Releases",
        "RunLoop",
        "Rust",
        "RustSec",
        "SAFETY",
        "SC",
        "SIGKILL",
        "SIGSTOP",
        "SSE",
        "STOP",
        "SVG",
        "SipHash",
        "Songti",
        "Sublime",
        "Swift",
        "TERM",
        "TIFF",
        "TLS",
        "TOML",
        "TTFT",
        "TTL",
        "TypeScript",
        "URL",
        "UTC",
        "UTF-8",
        "Unicode",
        "VM",
        "VoiceOver",
        "WebGL",
        "WebKit",
        "WindowServer",
        "Windows",
        "WorkBuddy",
        "ZCode",
        "aarch64",
        "abort",
        "ad-hoc",
        "base64",
        "bash",
        "bincode",
        "cargo",
        "cargo-bundle",
        "cargo-deny",
        "cargo-llvm-cov",
        "cargo-watch",
        "clippy",
        "core-foundation",
        "crate",
        "crates.io",
        "criterion",
        "crossbeam",
        "dtrace",
        "egui",
        "egui-wgpu",
        "egui-winit",
        "egui_commonmark",
        "egui_extras",
        "egui_kittest",
        "epaint",
        "eval",
        "flavor",
        "git-cliff",
        "gloss-app",
        "gloss-core",
        "gloss-eval",
        "gloss-mono",
        "gloss-platform",
        "image",
        "inferno",
        "justfile",
        "kittest",
        "libc",
        "libmalloc",
        "live",
        "macOS",
        "markdown",
        "mimalloc",
        "memmap2",
        "moka",
        "nm",
        "otool",
        "p50",
        "p95",
        "panic",
        "pollster",
        "pulldown-cmark",
        "pytest",
        "ruff",
        "rustls",
        "semver",
        "serde",
        "sha256",
        "span",
        "squash",
        "stderr",
        "stdin",
        "stdout",
        "syntect",
        "tap",
        "tokio",
        "tombi",
        "tracing",
        "ttc",
        "ui-monospace",
        "unsafe",
        "valgrind",
        "vmmap",
        "wgpu",
        "winit",
        "x86_64",
        "yaml-rust",
        "yaml-rust2",
        "zh-Hans-CN",
    }
)

# 中文主体允许的非汉字字符：中文标点、注音序号与少数语言中立符号；阿拉伯数
# 字与空白另行放行，其余一律判罚
CHINESE_EXTRA = "，。；：！？、「」『』《》…—·～＝％￥→①②③④⑤⑥⑦⑧⑨⑩⑪⑫⑬⑭⑮"

# 源码之外的注释文件：井号系脚本与配置、钩子目录脚本、任务配方文件与忽略清单。
HASH_SUFFIXES = {".py", ".toml", ".yml", ".yaml", ".sh"}
HASH_NAMES = {"justfile", ".gitignore"}
# 站点资源：脚本用双斜线与斜线星注释，样式用斜线星，页面用标记注释。
WEB_SUFFIXES = {".js", ".css", ".html"}

# 工具指令段整段豁免：解释器声明，以及格式化与静态检查指令及其外文参数。
DIRECTIVE_SEG = re.compile(
    r"^[!—–\-\s]*#!"
    r"|^[\s—–\-]*(?:noqa|fmt|ruff|isort|pylint|type|shellcheck|yamllint"
    r"|actionlint|prettier|markdownlint)\b(?:[:\s][A-Za-z0-9_+=,.\s\-]*)?$"
)
# 表述按中文标点切段；段两端先剥修饰字符，指令段只认整段外文的匹配，中文与
# 指令混写的段仍受检。
SEGMENT_SPLIT = re.compile(r"[，。；：！？、（）「」『』《》…—\n]+")
SEGMENT_TRIM = " \t\n─*-·-"


def is_comment_candidate(rel):
    name = os.path.basename(rel)
    ext = os.path.splitext(name)[1]
    return (
        ext in HASH_SUFFIXES
        or name in HASH_NAMES
        or rel.startswith(".githooks/")
        or ext in WEB_SUFFIXES
    )


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


def chinese_zone_problems(loc, zone, label):
    """中文主体的字符级检查：只允许汉字、中文标点、阿拉伯数字与空白。"""
    out = []
    for seg in SEGMENT_SPLIT.split(zone):
        seg = seg.strip(SEGMENT_TRIM)
        if not seg or DIRECTIVE_SEG.match(seg):
            continue
        bad = []
        for ch in seg:
            if CJK.match(ch) or ch in CHINESE_EXTRA or "0" <= ch <= "9" or ch in " \t\n\r":
                continue
            if ch not in bad:
                bad.append(ch)
        if bad:
            shown = " ".join(bad[:3])
            out.append(
                f"{loc}: {label}主体出现外文字符或半角标点（{shown}）"
                f"——专有名词收进全角括号注记，其余改写为中文"
            )
    return out


def gloss_problems(loc, gloss, label):
    """注记括号检查：内有外文时只许白名单专名、不得混入中文；纯中文时同主体。"""
    gloss = gloss.strip()
    if not gloss:
        return []
    if LATIN.search(gloss):
        if CJKWISE.search(gloss):
            return [f"{loc}: {label}注记括号混入中文——括号内只放专有名词本身，中文表述留在主体"]
        parts = [p for p in re.split(r"[\s,/]+", gloss) if p]
        bad = [p for p in parts if p not in PROPER_NOUNS]
        if bad:
            shown = " ".join(bad[:3])
            return [
                f"{loc}: {label}注记出现白名单之外的词（{shown}）"
                f"——只放登记过的专有名词；能改写为中文的写进主体"
            ]
        return []
    return chinese_zone_problems(loc, gloss, label)


def split_glosses(body):
    """按栈配对全角括号，切成注记与主体：返回（注记清单，主体残文，括号问题）。"""
    glosses = []
    residual = []
    depth = 0
    buf = []
    problems = []
    for ch in body:
        if ch == "（":
            depth += 1
            if depth == 1:
                residual.append(" ")
                buf = []
            else:
                problems.append("注记括号嵌套——括号不套括号，改写为并列或摊平表述")
                residual.append(ch)
        elif ch == "）":
            if depth == 0:
                problems.append("注记括号不配对——括号须成对包裹注记")
                residual.append(" ")
            else:
                if depth == 1:
                    glosses.append("".join(buf))
                else:
                    residual.append(ch)
                depth -= 1
        else:
            if depth == 0 or depth > 1:
                residual.append(ch)
            else:
                buf.append(ch)
    if depth > 0:
        problems.append("注记括号不配对——括号须成对包裹注记")
    return glosses, "".join(residual), problems


def body_problems(loc, body, label):
    """整条注释的分域检查：先摘注记括号，再查主体与各注记。"""
    out = []
    glosses, residual, bracket_problems = split_glosses(body)
    for message in bracket_problems:
        out.append(f"{loc}: {label}{message}")
    for gloss in glosses:
        out += gloss_problems(loc, gloss, label)
    out += chinese_zone_problems(loc, residual, label)
    return out


def module_docstring_problems(rel, src):
    """模块级文档字符串按文件头同规：内容走注释同款判罚，解释器引导串豁免。

    文档字符串是字面量不是注释，词法抽取收不到它；这里按语法树补一刀，
    防止文件头换一种写法就逃出纪律。语法异常的退回路径本就只管井号注释，
    此处一并跳过。
    """
    try:
        tree = ast.parse(src)
    except SyntaxError:
        return []
    if not tree.body:
        return []
    first = tree.body[0]
    if not (
        isinstance(first, ast.Expr)
        and isinstance(first.value, ast.Constant)
        and isinstance(first.value.value, str)
    ):
        return []
    text = first.value.value
    if re.search(r"exec\s+python", text):
        return []  # 解释器引导串：跨壳脚本的井号叹号替身，同解释器声明豁免
    loc = f"{rel}:{first.lineno}"
    if "`" in text:
        return [f"{loc}: 文件头出现代码段——引用与示例沉记录文件"]
    if "§" in text:
        return [f"{loc}: 文件头出现「§ 章节号」式外部引用——出处沉记录文件"]
    if not CJK.search(text):
        return [f"{loc}: 文件头须写成中文：{text[:40]}"]
    return body_problems(loc, text, "文件头")


def nonrust_violations(rel, src):
    name = os.path.basename(rel)
    ext = os.path.splitext(name)[1]
    python_kind = ext == ".py" or rel.startswith(".githooks/")
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
            return [f"{loc}: 注释出现代码段——引用与示例沉记录文件"]
        if "§" in merged:
            return [f"{loc}: 注释出现「§ 章节号」式外部引用——出处沉记录文件"]
        if not CJK.search(merged):
            return [f"{loc}: 注释须写成中文：{merged[:40]}"]
        return body_problems(loc, merged, "注释")

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
                out.append(f"{rel}: 文件头含无汉字的行：{body[:60]}")
            if "`" in body:
                out.append(f"{rel}: 文件头出现反引号代码段——代码引用移出文件头，沉记录文件")
            if "§" in body:
                out.append(
                    f"{rel}: 文件头出现「§ 章节号」式外部引用——章节号随文档漂移，出处进记录文件"
                )
            merged.append(body.replace("`", " ").replace("§", " "))
        if merged:
            out += body_problems(rel, "\n".join(merged), "文件头")
        return out

    for path in scan_files(root, explicit):
        rel = os.path.relpath(path, root)
        if not rel.endswith(".rs"):
            if is_comment_candidate(rel):
                try:
                    with open(path, encoding="utf-8") as fh:
                        plain = fh.read()
                except (OSError, UnicodeDecodeError) as exc:
                    problems.append(f"{rel}: 无法读取（{exc}）")
                    continue
                problems.extend(nonrust_violations(rel, plain))
            continue
        try:
            with open(path, encoding="utf-8") as fh:
                src = fh.read()
        except (OSError, UnicodeDecodeError) as exc:
            problems.append(f"{rel}: 无法读取（{exc}）")
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
                    problems.append(f"{rel}:{sline}: //! 出现在文件头之后，只能作文件头")
                else:
                    header_found = True
                    header_lines.append(text)
                continue
            if kind == "doc":
                problems.append(f"{rel}:{sline}: /// 文档注释 —— 内容迁记录文件后删除")
                continue
            if kind == "block":
                problems.append(f"{rel}:{sline}: 块注释 —— 内容迁记录文件后删除")
                continue
            # 此处余下的都是行内注释
            if MARKER.search(text):
                continue
            if not indent_only:
                problems.append(f"{rel}:{sline}: 行尾注释 —— 迁记录文件，豁免标记除外")
                continue
            stripped = text.strip()
            body = stripped.lstrip("/").strip()
            if safety_open or body.startswith("SAFETY:"):
                safety_open = True
                if CJKWISE.search(body):
                    problems.append(
                        f"{rel}:{sline}: SAFETY 注释须为纯英文——不变量整段改写为英文，符号引用保留"
                    )
                continue
            problems.append(f"{rel}:{sline}: 行内注释 —— 迁记录文件（SAFETY 与 :allow 除外）")
        if not header_found:
            problems.append(f"{rel}: 缺文件头 //! 注释")
        problems.extend(header_violations(header_lines, rel))
    return problems


def main():
    root = (
        sys.argv[1]
        if len(sys.argv) > 1
        else os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    )
    explicit = sys.argv[2:]
    print(f"== 注释纪律门禁（仓库根：{root}）==")
    problems = run(root, explicit)
    if problems:
        for p in problems:
            print(f"  ✗ {p}", file=sys.stderr)
        print(f"✗ 注释纪律门禁：{len(problems)} 处违规", file=sys.stderr)
        print(
            "  规则见本脚本头注；迁移目标 docs/comments/<文件名>_comment.md",
            file=sys.stderr,
        )
        sys.exit(1)
    print("  ok 注释纪律：源码只留中文文件头，注释主体全中文，专名只进括号注记，不变量注释纯英文")


if __name__ == "__main__":
    main()
