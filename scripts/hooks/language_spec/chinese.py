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
# 外加通用概念缩写与测试分层标签——正文中文已自明、注记只作对照锚点。新增
# 词条在此登记；标识符、路径、旗标、链接与代码段一律不进注释，沉记录文件。
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
        "YAML",
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
        "ripgrep",
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
