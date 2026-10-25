"""约束门禁·界面边界：界面库（egui）上下文统一装入点与外部函数接口（FFI）
声明集中——浮层与设置窗的上下文只允许经统一装入点建立并装上字体与主题，
跨语言声明只允许出现在平台适配层的专用模块；确需绕过的行尾放行标记并注明
缘由。违规经统一诊断出口带规则标识报告，证据行完整不截断。已知盲区与匹配
取舍见记录文件。
"""

from __future__ import annotations

import re
from typing import Any

import gate_report

from . import manifests

RULE_EGUI_CTX = "EGUI-CTX"
RULE_FFI_CENTRAL = "FFI-CENTRAL"

EGUI_CTX_REMEDY = (
    "go through new_context / reapply in ui/context.rs; for a real exception add"
    " constraints:allow-context with a reason at the end of the line"
)
FFI_CENTRAL_REMEDY = "move the extern block into the ffi module of gloss-platform"

CTX_RE = re.compile(r"(?:^|[^A-Za-z0-9_])Context::default\(\)|\.set_fonts\(|\.set_theme\(")

FFI_RE = re.compile(re.escape('extern"C"{'))


def first_hit(
    path: str, loader: Any, skip_marker: str | None, pattern: re.Pattern[str]
) -> tuple[int, str]:
    """取首个违规行：行号加完整行文；放行标记行按各自检查的口径跳过。"""
    for lineno, line in enumerate(manifests.read_text(path, loader).split("\n"), 1):
        if skip_marker is not None and skip_marker in line:
            continue
        if pattern.search(line.replace(" ", "").replace("\t", "")):
            return lineno, line
    return 0, ""


def run(
    root: str, loader: Any = None, report: manifests.Reporter | None = None
) -> tuple[list[str], list[str]]:
    """执行上下文装入点与声明集中约束，返回通过与失败两个清单。"""
    rep = report if report is not None else manifests.Reporter()
    oks = []
    context_owner = "crates/gloss-app/src/ui/context.rs"
    ctx_files = 0
    ctx_violations = 0
    for f in manifests.walk_rs(root):
        ctx_files += 1
        if manifests.rel(root, f) == context_owner:
            continue
        hit_line, hit_text = first_hit(f, loader, "constraints:allow-context", CTX_RE)
        if not hit_line:
            continue
        ctx_violations += 1
        rep.note(
            gate_report.Finding(
                rule=RULE_EGUI_CTX,
                path=manifests.rel(root, f),
                line=hit_line,
                message="bypasses the egui context unified load point — go through"
                " new_context / reapply in ui/context.rs",
                evidence=hit_text,
                remedy=EGUI_CTX_REMEDY,
            )
        )
    oks.append(
        "ok egui context unified load point (%d .rs files scanned, %d bypasses)"
        % (ctx_files, ctx_violations)
    )

    ffi_owner_dir = "crates/gloss-platform/src/ffi/"
    ffi_files = 0
    ffi_violations = 0
    for f in manifests.walk_rs(root):
        ffi_files += 1
        if manifests.rel(root, f).startswith(ffi_owner_dir):
            continue
        hit_line, hit_text = first_hit(f, loader, None, FFI_RE)
        if not hit_line:
            continue
        ffi_violations += 1
        rep.note(
            gate_report.Finding(
                rule=RULE_FFI_CENTRAL,
                path=manifests.rel(root, f),
                line=hit_line,
                message="declares a C interface inline — FFI declarations centralized in"
                " the ffi module of gloss-platform only",
                evidence=hit_text,
                remedy=FFI_CENTRAL_REMEDY,
            )
        )
    oks.append(
        "ok FFI declarations centralized (%d .rs files scanned, %d inline declarations)"
        % (ffi_files, ffi_violations)
    )
    return oks, rep.lines
