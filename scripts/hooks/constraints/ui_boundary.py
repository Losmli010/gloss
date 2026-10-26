"""约束门禁·界面边界：界面库（egui）上下文统一装入点与外部函数接口（FFI）
声明集中——浮层与设置窗的上下文只允许经统一装入点建立并装上字体与主题，
跨语言声明只允许出现在平台适配层的专用模块；确需绕过的行尾放行标记并注明
缘由、缺失即判罚。内容匹配一律委托共享检索引擎（ripgrep），表达式按空白
容忍形态书写、与逐行的去空白口径对齐。违规经统一诊断出口带规则标识报告，
证据行完整不截断。已知盲区与匹配取舍见记录文件。
"""

from __future__ import annotations

import sys
from typing import Any

import gate_report
import rg_engine

from . import manifests

RULE_EGUI_CTX = "EGUI-CTX"
RULE_FFI_CENTRAL = "FFI-CENTRAL"

EGUI_CTX_REMEDY = (
    "go through new_context / reapply in ui/context.rs; for a real exception add"
    " constraints:allow-context with a reason at the end of the line"
)
FFI_CENTRAL_REMEDY = "move the extern block into the ffi module of gloss-platform"

# 逐行判罚先剥空白再匹配；引擎表达式以空白容忍形态书写，两者口径对齐
CTX_EXPRESSION = (
    r"(?:^|[^A-Za-z0-9_])Context\s*::\s*default\s*\(\s*\)|\.set_fonts\s*\(|\.set_theme\s*\("
)

FFI_EXPRESSION = r'extern\s*"C"\s*\{'

ALLOW_CONTEXT_MARKER = "constraints:allow-context"

CONTEXT_OWNER = "crates/gloss-app/src/ui/context.rs"
FFI_OWNER_DIR = "crates/gloss-platform/src/ffi/"


def engine_hits(root: str, files: list[str], expression: str) -> dict[str, dict[int, str]]:
    """单表达式的引擎命中：引擎故障按无法完成判定失败关闭退出。"""
    try:
        return rg_engine.run(root, [expression], files)
    except RuntimeError as exc:
        gate_report.fail(
            gate_report.infra_failure("cannot complete the egui boundary scan: %s" % exc)
        )
        sys.exit(gate_report.EXIT_INFRA)


def run(
    root: str, loader: Any = None, report: manifests.Reporter | None = None
) -> tuple[list[str], list[str]]:
    """执行上下文装入点与声明集中约束，返回通过与失败两个清单。"""
    rep = report if report is not None else manifests.Reporter()
    oks = []
    rs_files = [manifests.rel(root, f) for f in manifests.walk_rs(root)]

    ctx_targets = [f for f in rs_files if f != CONTEXT_OWNER]
    ctx_hits = engine_hits(root, ctx_targets, CTX_EXPRESSION)
    ctx_violations = 0
    for f in ctx_targets:
        file_hits = ctx_hits.get(f, {})
        plain = {no: line for no, line in file_hits.items() if ALLOW_CONTEXT_MARKER not in line}
        marked = {no: line for no, line in file_hits.items() if ALLOW_CONTEXT_MARKER in line}
        if plain:
            ctx_violations += 1
            no = min(plain)
            rep.note(
                gate_report.Finding(
                    rule=RULE_EGUI_CTX,
                    path=f,
                    line=no,
                    message="bypasses the egui context unified load point — go through"
                    " new_context / reapply in ui/context.rs",
                    evidence=plain[no],
                    remedy=EGUI_CTX_REMEDY,
                )
            )
        for no in sorted(marked):
            if gate_report.allow_reason_missing(ALLOW_CONTEXT_MARKER, marked[no]):
                rep.note(gate_report.allow_reason_finding(ALLOW_CONTEXT_MARKER, f, no, marked[no]))
    oks.append(
        "egui context unified load point (%d .rs files scanned, %d bypasses)"
        % (len(rs_files), ctx_violations)
    )

    ffi_targets = [f for f in rs_files if not f.startswith(FFI_OWNER_DIR)]
    ffi_hits = engine_hits(root, ffi_targets, FFI_EXPRESSION)
    ffi_violations = 0
    for f in ffi_targets:
        file_hits = ffi_hits.get(f)
        if not file_hits:
            continue
        ffi_violations += 1
        no = min(file_hits)
        rep.note(
            gate_report.Finding(
                rule=RULE_FFI_CENTRAL,
                path=f,
                line=no,
                message="declares a C interface inline — FFI declarations centralized in"
                " the ffi module of gloss-platform only",
                evidence=file_hits[no],
                remedy=FFI_CENTRAL_REMEDY,
            )
        )
    oks.append(
        "FFI declarations centralized (%d .rs files scanned, %d inline declarations)"
        % (len(rs_files), ffi_violations)
    )
    return oks, rep.lines
