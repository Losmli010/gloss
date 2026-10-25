"""约束门禁·清单卫生：版本单点维护、依赖只开需要的特性、依赖无死条目——
子包的版本与语言版本继承工作区，字面量只在根清单的工作区段；第三方依赖声明
必须关闭默认特性，且必须在源码面有使用证据；条目行尾放行标记豁免并须注明
缘由。判定口径与取舍见记录文件。
"""

import os
import re

from . import manifests


def run(root, loader=None):
    """执行版本单点、依赖特性与死条目约束，返回通过与失败两个清单。"""
    dump = manifests.dep_dump(root, loader)
    oks = []
    fails = []

    root_manifest = os.path.join(root, "Cargo.toml")
    ws_body = (
        manifests.section_body(root_manifest, "[workspace.package]", loader)
        if os.path.isfile(root_manifest)
        else []
    )
    for field in ("version", "edition"):
        if not any(re.match(r"^%s\s*=\s*\"" % field, line) for line in ws_body):
            manifests.note_violation(
                fails,
                "constraint violation (single version source): %s [workspace.package]"
                " has no %s literal (the single source lives here)"
                % (manifests.rel(root, root_manifest), field),
            )
    for m in manifests.manifests(root):
        pkg = manifests.section_body(m, "[package]", loader)
        for field in ("version", "edition"):
            if not any(re.match(r"^%s\.workspace\s*=\s*true" % field, line) for line in pkg):
                manifests.note_violation(
                    fails,
                    "constraint violation (single version source): %s [package] misses"
                    " %s.workspace = true (do not hardcode the literal)"
                    % (manifests.rel(root, m), field),
                )
    oks.append("ok single version source (root [workspace.package] + per-crate *.workspace = true)")

    deps_checked = 0
    for _owner, mrel, _sec, key, start, text in dump:
        if manifests.is_path_dep(text) and "version" not in text:
            continue
        if "workspace = true" in text:
            continue
        deps_checked += 1
        if "default-features = false" not in text:
            manifests.note_violation(
                fails,
                "constraint violation (default features off): %s:%d %s lacks"
                " default-features = false" % (mrel, start, key),
            )
    oks.append(
        "ok default features off (%d third-party dependency declarations checked)" % deps_checked
    )

    dead_count = 0
    dead_scanned = 0
    dead_allowed = 0
    for owner, mrel, _sec, key, start, text in dump:
        if manifests.is_path_dep(text) or "workspace = true" in text:
            continue
        dead_scanned += 1
        if "deps:allow" in text:
            dead_allowed += 1
            continue
        crate_dir = root if mrel == "Cargo.toml" else os.path.join(root, os.path.dirname(mrel))
        import_name = key.replace("-", "_")
        evidence = re.compile(
            r"(?:^|[^A-Za-z0-9_])%s(?:::|!)|^\s*(?:pub\s+)?use\s+%s\s*(?:;|as)"
            % (import_name, import_name)
        )
        found = False
        for base in ("src", "tests", "benches"):
            base_dir = os.path.join(crate_dir, base)
            if not os.path.isdir(base_dir):
                continue
            for dirpath, _, filenames in os.walk(base_dir):
                for name in filenames:
                    path = os.path.join(dirpath, name)
                    for line in manifests.read_text(path, loader).split("\n"):
                        if evidence.search(line):
                            found = True
                            break
                    if found:
                        break
                if found:
                    break
            if found:
                break
        if found:
            continue
        dead_count += 1
        manifests.note_violation(
            fails,
            "constraint violation (no dead dependencies): %s declares %s (%s:%d) with no"
            " usage evidence in src/tests/benches — use it, drop it, or add a deps:allow"
            " marker with a reason at the end of the entry line"
            % (os.path.basename(owner), key, mrel, start),
        )
    if dead_count == 0:
        oks.append(
            "ok no dead dependencies (%d third-party declarations scanned, %d deps:allow)"
            % (dead_scanned, dead_allowed)
        )
    return oks, fails
