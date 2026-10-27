"""约束门禁·界面样式域：样式统一的四条边界——语义色构造只在语义色板、
明暗态只在样式出口的解析点读取、字体族构造只在字体基建与排印模块、界面
模块不声明含数值字面量的设计常数（算法阈值与资产几何按登记表显式放行，
放行文件之外的新增即拦）。生产代码行按首个列零的测试模块标记截断，内联
测试不受限；命中行带放行标记可豁免、缺缘由即判罚。违规经统一诊断出口带
规则标识报告，证据行完整不截断。判罚口径、登记表与已知盲区见记录文件。
"""

from __future__ import annotations

import os
import sys
from typing import Any

import gate_report
import rg_engine

from . import manifests

RULE_UI_STYLE_COLOR = "UI-STYLE-COLOR"
RULE_UI_STYLE_DARK = "UI-STYLE-DARK"
RULE_UI_STYLE_FONT = "UI-STYLE-FONT"
RULE_UI_STYLE_CONST = "UI-STYLE-CONST"

COLOR_REMEDY = (
    "move the color into the palette data in ui/style/palette.rs; for a real"
    " exception add style:allow with a reason at the end of the line"
)
DARK_REMEDY = (
    "resolve the appearance through the palette function in ui/style/mod.rs;"
    " for a real exception add style:allow with a reason at the end of the line"
)
FONT_REMEDY = (
    "build fonts through the role constructors in ui/style/typography.rs; for"
    " a real exception add style:allow with a reason at the end of the line"
)
CONST_REMEDY = (
    "move the design constant into ui/style/, or register the file as a"
    " non-design whitelist in this gate and its record; for a real exception"
    " add style:allow with a reason at the end of the line"
)

UI_DIR = "crates/gloss-app/src/ui/"
STYLE_DIR = UI_DIR + "style/"

COLOR_OWNER = STYLE_DIR + "palette.rs"
DARK_OWNER = STYLE_DIR + "mod.rs"
FONT_OWNERS = (
    UI_DIR + "fonts.rs",
    UI_DIR + "fonts/imp.rs",
    STYLE_DIR + "typography.rs",
)
CONST_OWNERS = (
    STYLE_DIR + "mod.rs",
    STYLE_DIR + "palette.rs",
    STYLE_DIR + "tokens.rs",
    STYLE_DIR + "typography.rs",
    STYLE_DIR + "geometry.rs",
    STYLE_DIR + "motion.rs",
)

# 非设计常数登记表：这些文件的数值常数没有设计语义（探测打分、文案资产、
# 语言别名表、图标栅格化几何），不进样式层，在此显式登记；登记外的新增
# 文件出现数值常数即被设计常数检查拦下。
CONST_WHITELIST = (
    UI_DIR + "fonts/imp.rs",
    UI_DIR + "i18n.rs",
    UI_DIR + "code_hl/",
    UI_DIR + "popup/icon.rs",
)

COLOR_EXPRESSION = r"Color32\s*::\s*from_"
DARK_EXPRESSION = r"\bdark_mode\b"
FONT_EXPRESSION = r"FontId\s*::\s*new\b|FontFamily\s*::"
CONST_EXPRESSION = r"(?:const|static)\s+\w+\s*:[^=]*=[^;]*\b\d"

ALLOW_MARKER = "style:allow"

TEST_MODULE_MARKER = "#[cfg(test)]"

COLOR_MESSAGE = (
    "constructs a semantic color inline — color data lives in the palette"
    " (ui/style/palette.rs) only"
)
DARK_MESSAGE = (
    "reads the appearance mode directly — resolve it once through the palette"
    " function in ui/style/mod.rs"
)
FONT_MESSAGE = (
    "constructs a font family inline — font construction lives in fonts.rs"
    " and ui/style/typography.rs only"
)
CONST_MESSAGE = (
    "declares a constant with a numeric literal — design constants live in"
    " ui/style/ only, non-design constants must be registered"
)


def engine_hits(
    root: str, files: list[str], expression: str, label: str
) -> dict[str, dict[int, str]]:
    """单表达式的引擎命中：引擎故障按无法完成判定失败关闭退出。"""
    try:
        return rg_engine.run(root, [expression], files)
    except RuntimeError as exc:
        gate_report.fail(
            gate_report.infra_failure("cannot complete the ui style scan (%s): %s" % (label, exc))
        )
        sys.exit(gate_report.EXIT_INFRA)


def production_cutoff(text: str) -> int:
    """生产代码的行号上限：首个列零的测试模块标记所在行（不含），与行数
    上限检查同一口径；返回值比任何生产行号都大表示没有测试模块。"""
    for no, line in enumerate(text.split("\n"), 1):
        if line.startswith(TEST_MODULE_MARKER):
            return no
    return len(text.split("\n")) + 1


def judge(
    rep: manifests.Reporter,
    rule: str,
    message: str,
    remedy: str,
    path: str,
    hits: dict[int, str],
) -> int:
    """单文件的命中判定：区分带放行标记与不带两路，不带即违例一条（证据
    取最小行号），带而缺缘由按标记判罚；返回违例文件数。"""
    plain = {no: line for no, line in hits.items() if ALLOW_MARKER not in line}
    marked = {no: line for no, line in hits.items() if ALLOW_MARKER in line}
    violations = 0
    if plain:
        violations += 1
        no = min(plain)
        rep.note(
            gate_report.Finding(
                rule=rule,
                path=path,
                line=no,
                message=message,
                evidence=plain[no],
                remedy=remedy,
            )
        )
    for no in sorted(marked):
        if gate_report.allow_reason_missing(ALLOW_MARKER, marked[no]):
            rep.note(gate_report.allow_reason_finding(ALLOW_MARKER, path, no, marked[no]))
    return violations


def run(
    root: str, loader: Any = None, report: manifests.Reporter | None = None
) -> tuple[list[str], list[str]]:
    """执行界面样式域四条检查，返回通过与失败两个清单。"""
    rep = report if report is not None else manifests.Reporter()
    oks = []
    rs_files = [
        manifests.rel(root, f)
        for f in manifests.walk_rs(os.path.join(root, "crates"), os.path.join(root, "src"))
    ]
    ui_files = [f for f in rs_files if f.startswith(UI_DIR)]

    def gated_hits(files: list[str], expression: str, label: str) -> dict[str, dict[int, str]]:
        hits = engine_hits(root, files, expression, label)
        gated: dict[str, dict[int, str]] = {}
        for f, file_hits in hits.items():
            text = manifests.read_text(os.path.join(root, f), loader)
            cutoff = production_cutoff(text)
            inside = {no: line for no, line in file_hits.items() if no < cutoff}
            if inside:
                gated[f] = inside
        return gated

    color_targets = [f for f in rs_files if f != COLOR_OWNER]
    color_violations = sum(
        judge(rep, RULE_UI_STYLE_COLOR, COLOR_MESSAGE, COLOR_REMEDY, f, hits)
        for f, hits in gated_hits(color_targets, COLOR_EXPRESSION, "colors").items()
    )
    oks.append(
        "semantic colors in the palette (%d .rs files scanned, %d inline colors)"
        % (len(color_targets), color_violations)
    )

    dark_targets = [f for f in rs_files if f != DARK_OWNER]
    dark_violations = sum(
        judge(rep, RULE_UI_STYLE_DARK, DARK_MESSAGE, DARK_REMEDY, f, hits)
        for f, hits in gated_hits(dark_targets, DARK_EXPRESSION, "appearance").items()
    )
    oks.append(
        "appearance resolved at the single read point (%d .rs files scanned,"
        " %d direct reads)" % (len(dark_targets), dark_violations)
    )

    font_targets = [f for f in rs_files if f not in FONT_OWNERS]
    font_violations = sum(
        judge(rep, RULE_UI_STYLE_FONT, FONT_MESSAGE, FONT_REMEDY, f, hits)
        for f, hits in gated_hits(font_targets, FONT_EXPRESSION, "fonts").items()
    )
    oks.append(
        "font construction centralized (%d .rs files scanned, %d inline builds)"
        % (len(font_targets), font_violations)
    )

    const_targets = [
        f for f in ui_files if not f.startswith(CONST_OWNERS) and not f.startswith(CONST_WHITELIST)
    ]
    const_violations = sum(
        judge(rep, RULE_UI_STYLE_CONST, CONST_MESSAGE, CONST_REMEDY, f, hits)
        for f, hits in gated_hits(const_targets, CONST_EXPRESSION, "constants").items()
    )
    oks.append(
        "design constants in ui/style (%d ui .rs files scanned, %d strays;"
        " %d files registered as non-design whitelist)"
        % (len(const_targets), const_violations, len(CONST_WHITELIST))
    )
    return oks, rep.lines
