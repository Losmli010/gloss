"""约束门禁·日志：统一出口与一律英文——除核心库（gloss-core）外不得直接
依赖日志三件套（tracing），日志宏实参里不得出现非英文字节（ASCII）。违规经
统一诊断出口带规则标识报告，证据为完整宏实参不截断。判定口径与取舍见记录
文件。
"""

from __future__ import annotations

import os
import re
from typing import Any

import gate_report

from . import manifests

RULE_LOG_EXIT = "LOG-EXIT"
RULE_LOG_ENGLISH = "LOG-ENGLISH"

LOG_EXIT_REMEDY = (
    "route logging through gloss_core::log instead of depending on the tracing family directly"
)
LOG_ENGLISH_REMEDY = (
    "keep log macro arguments in English (ASCII); localize in the UI catalog instead"
)

LOG_MACRO_RE = re.compile(
    r"(info|warn|error|debug|trace|event|info_span|warn_span|error_span"
    r"|debug_span|trace_span)!\(([^)]*)\)"
)

NON_ASCII_RE = re.compile(r"[^ -~]")


def run(
    root: str, loader: Any = None, report: manifests.Reporter | None = None
) -> tuple[list[str], list[str]]:
    """执行日志出口与日志语言约束，返回通过与失败两个清单。"""
    rep = report if report is not None else manifests.Reporter()
    dump = manifests.dep_dump(root, loader)
    oks = []
    for owner, mrel, _sec, key, start, text in dump:
        if key in ("tracing", "tracing-appender", "tracing-subscriber") and owner != "gloss-core":
            rep.note(
                gate_report.Finding(
                    rule=RULE_LOG_EXIT,
                    path=mrel,
                    line=start,
                    message="unified log exit: %s directly depends on %s — use"
                    " gloss_core::log only" % (os.path.basename(owner), key),
                    evidence=text,
                    remedy=LOG_EXIT_REMEDY,
                )
            )
    log_owners = len({owner for owner, _, _, _, _, _ in dump})
    oks.append("ok unified log exit (log dependency ownership across %d crates)" % log_owners)

    rs_files = 0
    log_violations = 0
    for f in manifests.walk_rs(root):
        rs_files += 1
        flat = manifests.read_text(f, loader).replace("\n", " ")
        hits = [m.group(0) for m in LOG_MACRO_RE.finditer(flat) if NON_ASCII_RE.search(m.group(0))]
        if hits:
            log_violations += 1
            rep.note(
                gate_report.Finding(
                    rule=RULE_LOG_ENGLISH,
                    path=manifests.rel(root, f),
                    line=0,
                    message="log messages English-only: has a log macro argument with non-ASCII",
                    evidence=hits[0],
                    remedy=LOG_ENGLISH_REMEDY,
                )
            )
    oks.append(
        "ok log messages English-only (%d .rs files scanned, %d violations)"
        % (rs_files, log_violations)
    )
    return oks, rep.lines
