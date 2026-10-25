#!/usr/bin/env python3
"""语言规范·中文区：中文表述怎么写。中文主体只允许汉字、中文标点、阿拉伯
数字与空白；专有名词一律收进紧跟的全角括号作注记，括号内只放白名单里的
专名本身，括号不嵌套、不成单边；工具指令段整段豁免。被注释、提交信息与
拉取请求内容门禁复用；本模块自身注释同规。

"""

import re

CJK = re.compile(r"[\u4e00-\u9fff]")
LATIN = re.compile(r"[A-Za-z]")
# 汉字、中文标点与全角形式：注记括号内出现即算混语，英文区出现即非纯英文
CJKWISE = re.compile(r"[\u4e00-\u9fff\u3000-\u303f\uff00-\uffef\u2018\u2019\u201c\u201d]")

# 注记括号允许的白名单：专有名词限技术、产品、格式、协议、框架、工具与包名，
# 外加通用概念缩写——正文中文已自明、缩写只作首现注记的那类。新增词条在此
# 登记；标识符、路径、旗标、链接与代码段一律不进注释，沉记录文件。
PROPER_NOUNS = frozenset(
    {
        ".rs",
        "AGPL",
        "AI",
        "AMD",
        "API",
        "ASCII",
        "AX",
        "AppKit",
        "Apple",
        "ApplicationServices",
        "BDD",
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
        "CoreFoundation",
        "CoreGraphics",
        "CoreText",
        "DMG",
        "Dart",
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
        "HIServices",
        "HIToolbox",
        "HTML",
        "HTTP",
        "HTTPS",
        "IO",
        "ImageIO",
        "Intel",
        "JS",
        "JSON",
        "JSONL",
        "JavaScript",
        "Julia",
        "Keychain",
        "Kotlin",
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
        "NSRunLoop",
        "OpenAI",
        "PEM",
        "PNG",
        "PR",
        "Pasteboard",
        "Python",
        "RGBA",
        "RSS",
        "RunLoop",
        "Rust",
        "RustSec",
        "SSE",
        "SVG",
        "SipHash",
        "Songti",
        "Sublime",
        "Swift",
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
        "macOS",
        "markdown",
        "mimalloc",
        "memmap2",
        "moka",
        "nm",
        "otool",
        "pollster",
        "pulldown-cmark",
        "pytest",
        "reqwest",
        "ruff",
        "rustls",
        "semver",
        "serde",
        "sha256",
        "syntect",
        "tokio",
        "tombi",
        "tracing",
        "ttc",
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
