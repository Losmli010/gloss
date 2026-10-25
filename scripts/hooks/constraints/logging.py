"""约束门禁·日志：统一出口与一律英文——除核心库（gloss-core）外不得直接
依赖日志三件套（tracing），日志宏实参里不得出现非英文字节（ASCII）。判定
口径与取舍见记录文件。
"""

import os
import re

from . import manifests


def run(root, loader=None):
    """执行日志出口与日志语言约束，返回通过与失败两个清单。"""
    dump = manifests.dep_dump(root, loader)
    oks = []
    fails = []
    for owner, mrel, _sec, key, start, _text in dump:
        if key in ("tracing", "tracing-appender", "tracing-subscriber") and owner != "gloss-core":
            manifests.note_violation(
                fails,
                "constraint violation (unified log exit): %s directly depends on %s"
                " (%s:%d) — use gloss_core::log only" % (os.path.basename(owner), key, mrel, start),
            )
    log_owners = len({owner for owner, _, _, _, _, _ in dump})
    oks.append("ok unified log exit (log dependency ownership across %d crates)" % log_owners)

    rs_files = 0
    log_violations = 0
    log_macro_re = re.compile(
        r"(info|warn|error|debug|trace|event|info_span|warn_span|error_span"
        r"|debug_span|trace_span)!\(([^)]*)\)"
    )
    for f in manifests.walk_rs(root):
        rs_files += 1
        flat = manifests.read_text(f, loader).replace("\n", " ")
        hits = [m.group(0) for m in log_macro_re.finditer(flat) if re.search(r"[^ -~]", m.group(0))]
        if hits:
            log_violations += 1
            manifests.note_violation(
                fails,
                "constraint violation (log messages English-only): %s has a log macro"
                " argument with non-ASCII — %s" % (manifests.rel(root, f), hits[0][:60]),
            )
    oks.append(
        "ok log messages English-only (%d .rs files scanned, %d violations)"
        % (rs_files, log_violations)
    )
    return oks, fails
