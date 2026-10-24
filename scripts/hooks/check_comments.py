#!/usr/bin/env python3
# 注释纪律门禁：跟踪文件的注释只写纯中文表述——不混英语语句，也不写代码引用。
# Rust 源码只保留文件头注释，其余注释一律迁往本地记录文件；非 Rust 文件的注释
# 逐行同规。本地注释配方（提交前门禁的一部分）与持续集成的钩子工作流共用此
# 脚本，保证本地与远端判定一致。
#
# Rust 侧覆盖五条检查：
#   只留文件头      —— 文档注释、行内、行尾与块注释一律违规；内部头注释只能
#                      出现在文件头（首个代码项之前）。
#   文件头必须存在  —— 每个被跟踪的 Rust 源文件至少一行头注释。
#   文件头是中文    —— 每行必须含汉字；禁止反引号代码段与标识符——类型、函数、
#                      字段、路径、旗标在代码里自明，细节沉记录文件；英文只允许
#                      裸写的专有名词（技术、产品、格式、框架与包名，白名单）。
#                      外部文档引用不进文件头——章节号随文档漂移，文档名是本地指针。
#   安全豁免        —— 不变量注释块及其紧邻的连续注释行不判罚（Clippy 要求
#                      不变量紧贴不安全块）。
#   行尾豁免标记    —— 各门禁的放行标记照旧放行所在行。
#
# 非 Rust 侧覆盖：脚本、配置、工作流与站点资源的注释逐行同规——必须含汉字、
# 不含反引号代码段、不含白名单之外的英文词；路径、命令、旗标、标识符与链接
# 都算代码引用，一并判罚。工具指令行豁免：解释器声明、格式化与静态检查指令、
# 行尾豁免标记。井号注释、双斜线注释、块注释与页面标记注释都纳入。
#
# 词法层区分注释与字符串字面量：字符串内的注释符（如链接）不误报。
#
# 用法：以解释器运行本脚本，首参为仓库根，其余参数为可选的文件清单（供单测
# 传入临时样本）；缺省扫全部被跟踪文件。

import io
import os
import re
import subprocess
import sys
import tokenize

CJK = re.compile(r"[\u4e00-\u9fff]")
WORD = re.compile(r"[A-Za-z0-9_.+-]*[A-Za-z][A-Za-z0-9_.+-]*")
MARKER = re.compile(r"\b(?:secrets|lines|i18n|constraints|deps):allow\b")

# 注释允许的英文专有名词白名单：技术、产品、格式、协议、框架、工具与包名。
# 新增专有名词在此登记；标识符、路径、旗标、链接与代码段一律不进注释。
PROPER_NOUNS = frozenset(
    {
        "4xx",
        "AGPL",
        "AI",
        "AMD",
        "API",
        "ASCII",
        "Apple",
        "AppKit",
        "ApplicationServices",
        "Actions",
        "AX",
        "Base64",
        "BDD",
        "C",
        "CD",
        "CEF",
        "CF",
        "CFRunLoop",
        "CI",
        "CJK",
        "CLI",
        "CPU",
        "Cmd",
        "Cmd+C",
        "Ctrl+C",
        "CVC",
        "Carbon",
        "Chrome",
        "Chromium",
        "Commits",
        "Conventional",
        "CoreFoundation",
        "CoreGraphics",
        "CoreText",
        "Dart",
        "Dependabot",
        "DMG",
        "Dock",
        "Data",
        "Electron",
        "Elixir",
        "FFI",
        "FT",
        "Gecko",
        "Gloss",
        "GitHub",
        "GPL",
        "GPU",
        "GUI",
        "HEAD",
        "HIServices",
        "HIToolbox",
        "HTTP",
        "HTTPS",
        "HTML",
        "IO",
        "Intel",
        "JS",
        "JSON",
        "JavaScript",
        "Julia",
        "Kotlin",
        "KILL",
        "L1",
        "L2",
        "L3",
        "L4",
        "LLM",
        "LRU",
        "LaunchServices",
        "LGPL",
        "Luhn",
        "Markdown",
        "Metal",
        "Miri",
        "MPMC",
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
        "Releases",
        "RunLoop",
        "Rust",
        "RustSec",
        "libc",
        "SIGKILL",
        "SIGSTOP",
        "SAFETY",
        "SC",
        "SSE",
        "STOP",
        "SVG",
        "SipHash",
        "Songti",
        "Sublime",
        "Swift",
        "TLS",
        "TOML",
        "TypeScript",
        "TTL",
        "TTFT",
        "TIFF",
        "TERM",
        "UTC",
        "UTF-8",
        "URL",
        "VM",
        "VoiceOver",
        "WebKit",
        "WebGL",
        "WindowServer",
        "Windows",
        "WorkBuddy",
        "ZCode",
        "aarch64",
        "abort",
        "ad-hoc",
        "bash",
        "base64",
        "BEGIN",
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
        "Cargo",
        "Clippy",
        "Release",
        "git-cliff",
        "crossbeam",
        "criterion",
        "dtrace",
        "egui",
        "egui-wgpu",
        "egui-winit",
        "egui_kittest",
        "egui_commonmark",
        "egui_extras",
        "epaint",
        "eval",
        "flavor",
        "gloss-app",
        "gloss-core",
        "gloss-eval",
        "gloss-mono",
        "gloss-platform",
        "image",
        "inferno",
        "justfile",
        "kittest",
        "libmalloc",
        "live",
        "mimalloc",
        "moka",
        "macOS",
        "markdown",
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
        "stdin",
        "stdout",
        "stderr",
        "syntect",
        "tap",
        "tombi",
        "tokio",
        "tracing",
        "ttc",
        "ui-monospace",
        "Unicode",
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


# 非 Rust 注释文件：井号系脚本与配置、钩子目录脚本、justfile 与忽略清单。
HASH_SUFFIXES = {".py", ".toml", ".yml", ".yaml", ".sh"}
HASH_NAMES = {"justfile", ".gitignore"}
# 站点资源：脚本用双斜线与斜线星注释，样式用斜线星，页面用标记注释。
WEB_SUFFIXES = {".js", ".css", ".html"}

# 工具指令段整段豁免：解释器声明，以及格式化与静态检查指令及其 ASCII 参数。
DIRECTIVE_SEG = re.compile(
    r"^[!—–\-\s]*#!"
    r"|^[\s—–\-]*(?:noqa|fmt|ruff|isort|pylint|type|shellcheck|yamllint"
    r"|actionlint|prettier|markdownlint)\b(?:[:\s][A-Za-z0-9_+=,.\s\-]*)?$"
)
# 表述按中文标点切段，指令段只认整段 ASCII 的匹配，中文与指令混写的段仍受检。
SEGMENT_SPLIT = re.compile(r"[，。；：！？、（）「」『』《》…—]+")
# 修饰性段内装饰字符，判罚前剥去。
SEGMENT_TRIM = " \t─*-·"


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
    """Python 注释走标准词法取准确集合；语法异常时退回井号扫描。"""
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


def nonrust_violations(rel, src):
    name = os.path.basename(rel)
    ext = os.path.splitext(name)[1]
    if name in HASH_NAMES:
        pairs = hash_comments(src, line_start_only=(name == ".gitignore"))
        kind = "hash"
    elif ext == ".py" or rel.startswith(".githooks/"):
        pairs = python_comments(src)
        kind = "hash"
    elif ext in HASH_SUFFIXES:
        pairs = hash_comments(src)
        kind = "hash"
    else:
        pairs = web_comments(src, ext)
        kind = "web"
    out = []
    for lineno, text in pairs:
        if MARKER.search(text):
            continue
        if text.startswith("#!"):
            continue
        body = web_body(text) if kind == "web" else text[1:].strip()
        segs = [
            seg.strip(SEGMENT_TRIM)
            for seg in SEGMENT_SPLIT.split(body)
            if seg.strip(SEGMENT_TRIM) and not DIRECTIVE_SEG.match(seg.strip(SEGMENT_TRIM))
        ]
        if not segs:
            continue
        if "`" in body:
            out.append(f"{rel}:{lineno}: 注释出现代码段——引用与示例沉记录文件")
            continue
        if "§" in body:
            out.append(f"{rel}:{lineno}: 注释出现「§ 章节号」式外部引用——出处沉记录文件")
            continue
        if not CJK.search(body):
            out.append(f"{rel}:{lineno}: 注释须写成中文：{body[:40]}")
            continue
        for seg in segs:
            bad = [w for w in WORD.findall(seg) if w not in PROPER_NOUNS]
            if bad:
                out.append(
                    f"{rel}:{lineno}: 注释出现代码引用或白名单之外的英文词：{' '.join(bad[:3])}"
                    f"（改写为中文；专有名词登记白名单；引用沉记录文件）"
                )
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
        """按字符扫描，产出注释事件：
        (seq, start_line, end_line, kind, indent_only, text)
        kind ∈ inner/doc/line/block；indent_only 指该行注释前只有空白。"""
        newlines = [m.start() for m in re.finditer("\n", src)]

        def line_of(i):
            import bisect

            return bisect.bisect_right(newlines, i) + 1

        events = []
        i, n = 0, len(src)
        seq = 0
        code_seen = False  # 是否已越过文件头（见过代码项）
        line_starts_blank = True  # 当前行目前只有空白
        inner_attr_only = True  # 当前行目前只有内部属性 #![...]（不算代码项）

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
                # 内部属性 #![...]：字符串感知地跳到配对方括号，不视为代码项
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
        for text in lines:
            body = text[3:].strip() if text.startswith("//!") else text.strip()
            if not body:
                continue
            if not CJK.search(body):
                out.append(f"{rel}: 文件头含无汉字的行：{body[:60]}")
            if "§" in body:
                out.append(
                    f"{rel}: 文件头出现「§ 章节号」式外部引用——章节号随文档漂移，出处进记录文件"
                )
            if "`" in body:
                out.append(f"{rel}: 文件头出现反引号代码段——专有名词裸写，标识符与代码移出文件头")
                continue
            bad = [w for w in WORD.findall(body) if w not in PROPER_NOUNS]
            if bad:
                out.append(
                    f"{rel}: 文件头出现专有名词之外的英文词：{' '.join(bad[:3])}"
                    f"（改写为中文；确属专有名词则登记 PROPER_NOUNS 白名单）"
                )
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
                problems.append(f"{rel}:{sline}: /// 文档注释 —— 内容迁 docs/comments/ 后删除")
                continue
            if kind == "block":
                problems.append(f"{rel}:{sline}: 块注释 —— 内容迁 docs/comments/ 后删除")
                continue
            # 此处余下的都是行内注释
            if MARKER.search(text):
                continue
            if not indent_only:
                problems.append(f"{rel}:{sline}: 行尾注释 —— 迁 docs/comments/，豁免标记除外")
                continue
            stripped = text.strip()
            body = stripped.lstrip("/").strip()
            if safety_open or body.startswith("SAFETY:"):
                safety_open = True
                continue
            problems.append(
                f"{rel}:{sline}: 行内注释 —— 迁 docs/comments/（SAFETY 与 :allow 除外）"
            )
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
    print("  ok 注释纪律：Rust 源码只留中文文件头，非 Rust 注释全部为纯中文表述")


if __name__ == "__main__":
    main()
