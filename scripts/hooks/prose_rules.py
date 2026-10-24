#!/usr/bin/env python3
"""书写纪律公共规则：中文主体的字符集、专有名词注记、括号配对与标记语言
豁免。注释门禁、提交信息门禁与拉取请求内容门禁共用此单一实现，保证多处
判定一致；本模块自身注释同规。

两层口径：
  正文判定    —— 摘出注记括号后查主体与各注记：只允许汉字、中文标点、阿拉
                 伯数字与空白，专名只进紧跟的全角括号且限白名单；
  行与文档判定 —— 先豁免结构性记号（行内反引号、链接、强调、任务列表、日
                 期），再落正文判定；标记语言按行遍历并跳过代码围栏。
"""

import re

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
        ".rs",
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
        "CRITICAL",
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
        "DeepSeek",
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
        "HIGH",
        "HIServices",
        "HIToolbox",
        "HTML",
        "HTTP",
        "HTTPS",
        "IO",
        "ImageIO",
        "Intel",
        "Issue",
        "JS",
        "JSON",
        "JSONL",
        "JavaScript",
        "Julia",
        "KILL",
        "Keychain",
        "Kotlin",
        "L1",
        "L2",
        "L3",
        "L4",
        "LGPL",
        "LLM",
        "LOW",
        "LRU",
        "LaunchServices",
        "Luhn",
        "M1",
        "M2",
        "M3",
        "M4",
        "M5",
        "MPMC",
        "Markdown",
        "Metal",
        "Miri",
        "NVIDIA",
        "NSRunLoop",
        "OpenAI",
        "PEM",
        "PNG",
        "PR",
        "Pages",
        "Pasteboard",
        "Prompt",
        "ProperNoun",
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
        "UB",
        "URL",
        "UTC",
        "UTF-8",
        "Unicode",
        "VM",
        "VoiceOver",
        "WebGL",
        "WebGPU",
        "WebKit",
        "WebView",
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
        "git",
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
        "keychain",
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
        "reqwest",
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
CHINESE_EXTRA = "，。；：！？《》…—·～＝％￥→\u201c\u201d\u2018\u2019"

# 工具指令段整段豁免：解释器声明，以及格式化与静态检查指令及其外文参数。
DIRECTIVE_SEG = re.compile(
    r"^[!—–\-\s]*#!"
    r"|^[\s—–\-]*(?:noqa|fmt|ruff|isort|pylint|type|shellcheck|yamllint"
    r"|actionlint|prettier|markdownlint)\b(?:[:\s][A-Za-z0-9_+=,.\s\-]*)?$"
)
# 表述按中文标点切段；段两端先剥修饰字符，指令段只认整段外文的匹配，中文与
# 指令混写的段仍受检。
SEGMENT_SPLIT = re.compile(r"[，。；：！？、（）《》…—\n]+")
SEGMENT_TRIM = " \t\n─*-·-"

DATE_RE = re.compile(r"\d{4}-\d{2}-\d{2}")


def chinese_zone_problems(loc, zone, label):
    """中文主体的字符级检查：只允许汉字、中文标点、阿拉伯数字与空白。

    数字之间的小数点随数字放行——它是数字写法的一部分，不是句读标点。"""
    out = []
    for seg in SEGMENT_SPLIT.split(zone):
        seg = seg.strip(SEGMENT_TRIM)
        if not seg or DIRECTIVE_SEG.match(seg):
            continue
        bad = []
        for index, ch in enumerate(seg):
            if CJK.match(ch) or ch in CHINESE_EXTRA or "0" <= ch <= "9" or ch in " \t\n\r":
                continue
            if (
                ch == "."
                and index > 0
                and seg[index - 1].isdigit()
                and index + 1 < len(seg)
                and seg[index + 1].isdigit()
            ):
                continue
            if ch not in bad:
                bad.append(ch)
        if bad:
            shown = " ".join(bad[:3])
            out.append(
                f"{loc}: {label} body contains foreign letters or halfwidth punctuation"
                f" ({shown}) — put proper nouns in fullwidth bracket annotations,"
                f" rewrite the rest in Chinese"
            )
    return out


def gloss_problems(loc, gloss, label):
    """注记括号检查：内有外文时只许白名单专名、不得混入中文；纯中文时同主体。"""
    gloss = gloss.strip()
    if not gloss:
        return []
    if LATIN.search(gloss):
        if CJKWISE.search(gloss):
            return [
                f"{loc}: {label} annotation bracket mixes Chinese"
                " — brackets hold proper nouns only; keep Chinese prose in the body"
            ]
        parts = [p for p in re.split(r"[\s,/]+", gloss) if p]
        bad = [p for p in parts if p not in PROPER_NOUNS]
        if bad:
            shown = " ".join(bad[:3])
            return [
                f"{loc}: {label} annotation has words outside the allowlist ({shown})"
                " — registered proper nouns only; rewrite anything else as Chinese in the body"
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
                problems.append(
                    " annotation brackets must not nest — brackets do not nest brackets;"
                    " flatten or reword as parallel clauses"
                )
                residual.append(ch)
        elif ch == "）":
            if depth == 0:
                problems.append(
                    " annotation brackets are unpaired — brackets must wrap annotations"
                    " in matched pairs"
                )
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
        problems.append(
            " annotation brackets are unpaired — brackets must wrap annotations in matched pairs"
        )
    return glosses, "".join(residual), problems


def body_problems(loc, body, label):
    """整条表述的分域检查：先摘注记括号，再查主体与各注记。"""
    out = []
    glosses, residual, bracket_problems = split_glosses(body)
    for message in bracket_problems:
        out.append(f"{loc}: {label}{message}")
    for gloss in glosses:
        out += gloss_problems(loc, gloss, label)
    out += chinese_zone_problems(loc, residual, label)
    return out


def strip_links(text):
    """链接与图片逐层剥除：徽章是套着图片的嵌套链接，先剥内层图片再剥外层。"""
    for pattern in (r"!\[[^\]]*\]\([^)]*\)", r"\[[^\]]*\]\([^)]*\)"):
        while True:
            stripped = re.sub(pattern, " ", text)
            if stripped == text:
                break
            text = stripped
    return text


def prose_line_problems(loc, text):
    """单行表述判定：结构性记号豁免后落正文判定，主体必须中文。"""
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
            f"{loc}: text contains a code segment — move references and samples to the record files"
        ]
    if "§" in text:
        return [
            f'{loc}: text contains a "§ section number" style external reference — cite it in the record files'
        ]
    if not CJK.search(text):
        return [f"{loc}: text must be written in Chinese: {text[:40]}"]
    return body_problems(loc, text, "text")


def markdown_problems(rel, src):
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
        out += markdown_line_problems(rel, lineno, line)
    return out


def markdown_line_problems(rel, lineno, line):
    stripped = line.strip()
    if not stripped:
        return []
    loc = f"{rel}:{lineno}"
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
            out += prose_line_problems(loc, cell)
        return out
    return prose_line_problems(loc, stripped)
