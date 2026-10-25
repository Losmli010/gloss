"""约束门禁·界面边界：界面库（egui）上下文统一装入点与外部函数接口（FFI）
声明集中——浮层与设置窗的上下文只允许经统一装入点建立并装上字体与主题，
跨语言声明只允许出现在平台适配层的专用模块；确需绕过的行尾放行标记并注明
缘由。已知盲区与匹配取舍见记录文件。
"""

from __future__ import annotations

import re
from typing import Any

from . import manifests


def run(root: str, loader: Any = None) -> tuple[list[str], list[str]]:
    """执行上下文装入点与声明集中约束，返回通过与失败两个清单。"""
    oks = []
    fails = []
    context_owner = "crates/gloss-app/src/ui/context.rs"
    ctx_re = re.compile(r"(?:^|[^A-Za-z0-9_])Context::default\(\)|\.set_fonts\(|\.set_theme\(")
    ctx_files = 0
    ctx_violations = 0
    for f in manifests.walk_rs(root):
        ctx_files += 1
        if manifests.rel(root, f) == context_owner:
            continue
        hits = [
            line
            for line in manifests.read_text(f, loader).split("\n")
            if "constraints:allow-context" not in line
            and ctx_re.search(line.replace(" ", "").replace("\t", ""))
        ]
        if not hits:
            continue
        ctx_violations += 1
        manifests.note_violation(
            fails,
            "constraint violation (egui context unified load point): %s bypasses the"
            " unified load point — go through new_context / reapply in ui/context.rs"
            " (%s)" % (manifests.rel(root, f), hits[0][:40]),
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
        hits = [
            line
            for line in manifests.read_text(f, loader).split("\n")
            if 'extern"C"{' in line.replace(" ", "").replace("\t", "")
        ]
        if not hits:
            continue
        ffi_violations += 1
        manifests.note_violation(
            fails,
            "constraint violation (FFI declarations centralized): %s declares a C"
            " interface inline — move it into the ffi module of gloss-platform (%s)"
            % (manifests.rel(root, f), hits[0][:40]),
        )
    oks.append(
        "ok FFI declarations centralized (%d .rs files scanned, %d inline declarations)"
        % (ffi_files, ffi_violations)
    )
    return oks, fails
