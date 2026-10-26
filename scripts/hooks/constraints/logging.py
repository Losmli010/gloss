"""约束门禁·日志：统一出口与一律英文——除核心库（gloss-core）外不得直接
依赖日志三件套（tracing），日志宏实参里不得出现非英文字节（ASCII）。宏实
参可跨行，内容匹配委托共享检索引擎（ripgrep）的多行模式，表达式与逐文件
折单行的既有口径对齐：实参类排除换行与回车——通用换行翻译会把回车换行折
成换行，回车本身不构成非英文字节命中，制表符仍在判罚面。已知盲区：非统一
码（UTF-8）编码的字节序列在引擎的统一码匹配下整文件逃出本检查（旧实现的
容错解码可判），仓库源码约定统一码编码，实害有限。违规经统一诊断出口带规
则标识报告，证据为命中跨住的完整行不截断。判定口径与取舍见记录文件。
"""

from __future__ import annotations

import os
import sys
from typing import Any

import gate_report
import rg_engine

from . import manifests

RULE_LOG_EXIT = "LOG-EXIT"
RULE_LOG_ENGLISH = "LOG-ENGLISH"

LOG_EXIT_REMEDY = (
    "route logging through gloss_core::log instead of depending on the tracing family directly"
)
LOG_ENGLISH_REMEDY = (
    "keep log macro arguments in English (ASCII); localize in the UI catalog instead"
)

# 宏名先长后短排布，跨行模式里实参类排除换行与回车：通用换行翻译会把回车
# 换行折成换行，回车本身不构成非英文字节命中，制表符仍在判罚面
LOG_MACRO_EXPRESSION = (
    r"(?:info_span|warn_span|error_span|debug_span|trace_span"
    r"|info|warn|error|debug|trace|event)!\([^)]*[^ -~\n\r][^)]*\)"
)


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
                    evidence=manifests.flat_text(text),
                    remedy=LOG_EXIT_REMEDY,
                )
            )
    log_owners = len({owner for owner, _, _, _, _, _ in dump})
    oks.append("unified log exit (log dependency ownership across %d crates)" % log_owners)

    rs_files = [manifests.rel(root, f) for f in manifests.walk_rs(root)]
    try:
        hits = rg_engine.run(root, [LOG_MACRO_EXPRESSION], rs_files, multiline=True)
    except RuntimeError as exc:
        gate_report.fail(
            gate_report.infra_failure("cannot complete the log language scan: %s" % exc)
        )
        sys.exit(gate_report.EXIT_INFRA)
    log_violations = 0
    for f in rs_files:
        file_hits = hits.get(f)
        if not file_hits:
            continue
        log_violations += 1
        no = min(file_hits)
        rep.note(
            gate_report.Finding(
                rule=RULE_LOG_ENGLISH,
                path=f,
                line=no,
                message="log messages English-only: has a log macro argument with non-ASCII",
                evidence=file_hits[no].replace("\n", " "),
                remedy=LOG_ENGLISH_REMEDY,
            )
        )
    oks.append(
        "log messages English-only (%d .rs files scanned, %d violations)"
        % (len(rs_files), log_violations)
    )
    return oks, rep.lines
