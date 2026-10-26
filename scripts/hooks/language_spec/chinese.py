#!/usr/bin/env python3
"""语言规范·中文区：中文表述怎么写。中文主体只允许汉字、中文标点、阿拉伯
数字与空白；专有名词一律收进紧跟的全角括号作注记，括号内只放白名单里的
专名本身，括号不嵌套、不成单边；工具指令段与可直接运行的命令段整段豁免。
判罚以带规则标识的违规条目返回，位置由调用方按路径与行号传入，标识登记
于统一登记表。被注释、提交信息与拉取请求内容门禁复用；本模块自身注释同规。

"""

from __future__ import annotations

import os
import re
import sys

_HOOKS = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if _HOOKS not in sys.path:
    sys.path.insert(0, _HOOKS)

import gate_report  # noqa: E402

RULE_PROSE_FOREIGN = "PROSE-FOREIGN"
RULE_PROSE_GLOSS_MIXED = "PROSE-GLOSS-MIXED"
RULE_PROSE_GLOSS_UNKNOWN = "PROSE-GLOSS-UNKNOWN"
RULE_PROSE_GLOSS_NESTED = "PROSE-GLOSS-NESTED"
RULE_PROSE_GLOSS_UNPAIRED = "PROSE-GLOSS-UNPAIRED"

FOREIGN_REMEDY = "put proper nouns in fullwidth bracket annotations and write the rest in Chinese"
GLOSS_MIXED_REMEDY = "brackets hold proper nouns only; move Chinese prose back into the body"
GLOSS_UNKNOWN_REMEDY = "register the term in the allowlist or rewrite it in Chinese in the body"
GLOSS_NESTED_REMEDY = "flatten nested brackets or reword as parallel clauses"
GLOSS_UNPAIRED_REMEDY = "complete or remove the unpaired bracket"

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
# 可直接运行的命令段整段豁免：解释器或仓库工具名开头、全外文参数的段——
# 脚本用法行写可复制的命令本体，混入中文即失去可复制性，故整段放行；参数
# 区须含命令形态的结构性记号（双横旗标、路径分隔、占位与选项括号、选项冒
# 号），句点之类的散文标点不算，以工具词开头的英文散文句不豁免。
COMMAND_SEG = re.compile(
    r"^(?:python3(?:\.\d+)?|python|cargo|just|git|rg|bash|sh|make|curl)"
    r"(?:[ \t]+[-A-Za-z0-9_+=,./:@%<>[\]*?|]+)+$"
)
COMMAND_ARG_HINT = re.compile(r"--|/|<|\[|:")


def is_command_seg(seg: str) -> bool:
    """段是否为可直接运行的命令：命令形态全配，且参数区带结构性字符。"""
    return COMMAND_SEG.match(seg) is not None and COMMAND_ARG_HINT.search(seg) is not None


# 表述按中文标点切段；段两端先剥修饰字符，指令段只认整段外文的匹配，中文与
# 指令混写的段仍受检。
SEGMENT_SPLIT = re.compile(r"[，。；：！？、（）《》…—\n]+")
SEGMENT_TRIM = " \t\n─*-·-"


def chinese_zone_problems(path: str, line: int, zone: str, label: str) -> list[gate_report.Finding]:
    """中文主体的字符级检查：只允许汉字、中文标点、阿拉伯数字与空白。

    数字之间的小数点随数字放行——它是数字写法的一部分，不是句读标点。"""
    out = []
    for seg in SEGMENT_SPLIT.split(zone):
        seg = seg.strip(SEGMENT_TRIM)
        if not seg or DIRECTIVE_SEG.match(seg) or is_command_seg(seg):
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
                gate_report.Finding(
                    rule=RULE_PROSE_FOREIGN,
                    path=path,
                    line=line,
                    message="%s body contains foreign letters or halfwidth punctuation"
                    " (%s) — put proper nouns in fullwidth bracket annotations,"
                    " rewrite the rest in Chinese" % (label, shown),
                    evidence=seg,
                    remedy=FOREIGN_REMEDY,
                )
            )
    return out


def gloss_problems(path: str, line: int, gloss: str, label: str) -> list[gate_report.Finding]:
    """注记括号检查：内有外文时只许白名单专名、不得混入中文；纯中文时同主体。"""
    gloss = gloss.strip()
    if not gloss:
        return []
    if LATIN.search(gloss):
        if CJKWISE.search(gloss):
            return [
                gate_report.Finding(
                    rule=RULE_PROSE_GLOSS_MIXED,
                    path=path,
                    line=line,
                    message="%s annotation bracket mixes Chinese"
                    " — brackets hold proper nouns only; keep Chinese prose in the body" % label,
                    evidence=gloss,
                    remedy=GLOSS_MIXED_REMEDY,
                )
            ]
        parts = [p for p in re.split(r"[\s,/]+", gloss) if p]
        bad = [p for p in parts if p not in PROPER_NOUNS]
        if bad:
            shown = " ".join(bad[:3])
            return [
                gate_report.Finding(
                    rule=RULE_PROSE_GLOSS_UNKNOWN,
                    path=path,
                    line=line,
                    message="%s annotation has words outside the allowlist (%s)"
                    " — registered proper nouns only; rewrite anything else as Chinese"
                    " in the body" % (label, shown),
                    evidence=gloss,
                    remedy=GLOSS_UNKNOWN_REMEDY,
                )
            ]
        return []
    return chinese_zone_problems(path, line, gloss, label)


def split_glosses(body: str) -> tuple[list[str], str, list[tuple[str, str]]]:
    """按栈配对全角括号，切成注记与主体：返回（注记清单，主体残文，括号问题）；
    括号问题收规则标识与消息尾段，消息头由调用方按标签补齐。"""
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
                    (
                        RULE_PROSE_GLOSS_NESTED,
                        " annotation brackets must not nest — brackets do not nest brackets;"
                        " flatten or reword as parallel clauses",
                    )
                )
                residual.append(ch)
        elif ch == "）":
            if depth == 0:
                problems.append(
                    (
                        RULE_PROSE_GLOSS_UNPAIRED,
                        " annotation brackets are unpaired — brackets must wrap annotations"
                        " in matched pairs",
                    )
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
            (
                RULE_PROSE_GLOSS_UNPAIRED,
                " annotation brackets are unpaired — brackets must wrap annotations"
                " in matched pairs",
            )
        )
    return glosses, "".join(residual), problems


def body_problems(path: str, line: int, body: str, label: str) -> list[gate_report.Finding]:
    """整条表述的分域检查：先摘注记括号，再查主体与各注记。"""
    out = []
    glosses, residual, bracket_problems = split_glosses(body)
    evidence = body.replace("\n", " ")
    for rule, tail in bracket_problems:
        remedy = GLOSS_NESTED_REMEDY if rule == RULE_PROSE_GLOSS_NESTED else GLOSS_UNPAIRED_REMEDY
        out.append(
            gate_report.Finding(
                rule=rule,
                path=path,
                line=line,
                message="%s%s" % (label, tail),
                evidence=evidence,
                remedy=remedy,
            )
        )
    for gloss in glosses:
        out += gloss_problems(path, line, gloss, label)
    out += chinese_zone_problems(path, line, residual, label)
    return out
