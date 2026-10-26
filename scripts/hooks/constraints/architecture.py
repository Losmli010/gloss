"""约束门禁·架构：依赖方向与评测资产隔离——各包只能依赖允许的边，核心库
（gloss-core）不得引入平台与渲染栈；评测包（gloss-eval）不被任何清单依赖，
评测资产不被生产代码内嵌；包名必须在项目文档登记过，挡住边表与文档的单边
改动。违规经统一诊断出口带规则标识报告，证据行完整不截断。允许边表与红线
的取舍见记录文件。
"""

from __future__ import annotations

import os
import re
from typing import Any

import gate_report

from . import manifests

RULE_DEP_DIRECTION = "DEP-DIRECTION"
RULE_DEP_CORE_REDLINE = "DEP-CORE-REDLINE"
RULE_DEP_UNREGISTERED = "DEP-UNREGISTERED"
RULE_EVAL_DEP = "EVAL-DEP"
RULE_EVAL_ASSET = "EVAL-ASSET"

DEP_DIRECTION_REMEDY = (
    "move the dependency to an allowed layer edge; changing the allowed table must"
    " update the project doc and this module together"
)
CORE_REDLINE_REMEDY = (
    "gloss-core stays free of platform and render stack dependencies — move the"
    " dependency up the layering"
)
DEP_UNREGISTERED_REMEDY = (
    "register the crate name in the dependency direction notes of the project doc"
)
EVAL_DEP_REMEDY = "move the dependency on the eval toolchain to dev-dependencies, or drop it"
EVAL_ASSET_REMEDY = "keep eval assets referenced only from inside the eval crate"

ALLOWED_OWN_DEPS = {
    "gloss": "gloss-core gloss-platform gloss-app",
    "gloss-app": "gloss-core gloss-platform",
    "gloss-platform": "gloss-core",
    "gloss-core": "",
    "gloss-eval": "gloss-core",
}

CORE_FORBIDDEN_EXACT = {
    "winit",
    "wgpu",
    "egui",
    "epaint",
    "objc2",
    "windows",
    "pollster",
    "rdev",
    "global-hotkey",
    "arboard",
    "font-kit",
    "directories",
    "device_query",
    "enigo",
}
CORE_FORBIDDEN_PREFIX = (
    "winit-",
    "wgpu-",
    "egui-",
    "epaint-",
    "objc2-",
    "windows-",
    "core-graphics",
    "core-text",
    "core-foundation",
    "security-framework",
    "raw-window-handle",
)

EVAL_ASSET_RE = re.compile(r"include_str!\(.*gloss-eval|include_bytes!\(.*gloss-eval")


def core_forbidden(key: str) -> bool:
    return key in CORE_FORBIDDEN_EXACT or key.startswith(CORE_FORBIDDEN_PREFIX)


def run(
    root: str, loader: Any = None, report: manifests.Reporter | None = None
) -> tuple[list[str], list[str]]:
    """执行依赖方向与评测资产隔离约束，返回通过与失败两个清单。"""
    rep = report if report is not None else manifests.Reporter()
    manifest_list = manifests.manifests(root, loader)
    dump = manifests.dep_dump(root, loader)
    oks = []
    edges = 0
    for owner, mrel, _sec, key, start, text in dump:
        if manifests.is_path_dep(text):
            edges += 1
            allowed = ALLOWED_OWN_DEPS.get(owner, "")
            if key not in allowed.split():
                rep.note(
                    gate_report.Finding(
                        rule=RULE_DEP_DIRECTION,
                        path=mrel,
                        line=start,
                        message="dependency direction: %s must not depend on %s; allowed"
                        " edges: %s" % (os.path.basename(owner), key, allowed or "none"),
                        evidence=manifests.flat_text(text),
                        remedy=DEP_DIRECTION_REMEDY,
                    )
                )
        elif owner == "gloss-core" and core_forbidden(key):
            rep.note(
                gate_report.Finding(
                    rule=RULE_DEP_CORE_REDLINE,
                    path=mrel,
                    line=start,
                    message="dependency direction: gloss-core has a platform/render stack"
                    " dependency %s — red-line, forbidden" % key,
                    evidence=manifests.flat_text(text),
                    remedy=CORE_REDLINE_REMEDY,
                )
            )
    agents_md = os.path.join(root, "AGENTS.md")
    if os.path.isfile(agents_md):
        agents_text = manifests.read_text(agents_md, loader)
        for m in manifest_list:
            name = manifests.pkg_name(m, loader)
            if name and name not in agents_text:
                rep.note(
                    gate_report.Finding(
                        rule=RULE_DEP_UNREGISTERED,
                        path=manifests.rel(root, m),
                        line=0,
                        message="crate %s missing from the dependency direction notes in %s"
                        % (name, manifests.rel(root, agents_md)),
                        remedy=DEP_UNREGISTERED_REMEDY,
                    )
                )
    oks.append(
        "ok dependency direction (%d in-repo dependency edges + crate names registered)" % edges
    )

    eval_dep_violations = 0
    for owner, mrel, _sec, key, start, text in dump:
        if key == "gloss-eval" and owner != "gloss-eval":
            rep.note(
                gate_report.Finding(
                    rule=RULE_EVAL_DEP,
                    path=mrel,
                    line=start,
                    message="eval asset isolation: %s depends on gloss-eval — the eval"
                    " toolchain must stay out of the production dependency graph" % owner,
                    evidence=manifests.flat_text(text),
                    remedy=EVAL_DEP_REMEDY,
                )
            )
            eval_dep_violations += 1
    eval_asset_violations = 0
    for f in manifests.walk_rs(
        os.path.join(root, "crates"),
        os.path.join(root, "src"),
        os.path.join(root, "tests"),
        os.path.join(root, "benches"),
    ):
        if manifests.rel(root, f).startswith("crates/gloss-eval/"):
            continue
        hit_line = 0
        hit_text = ""
        for lineno, line in enumerate(manifests.read_text(f, loader).split("\n"), 1):
            if EVAL_ASSET_RE.search(line):
                hit_line = lineno
                hit_text = line
                break
        if not hit_line:
            continue
        eval_asset_violations += 1
        rep.note(
            gate_report.Finding(
                rule=RULE_EVAL_ASSET,
                path=manifests.rel(root, f),
                line=hit_line,
                message="eval asset isolation: this file embeds a gloss-eval asset into"
                " production code",
                evidence=hit_text,
                remedy=EVAL_ASSET_REMEDY,
            )
        )
    if eval_dep_violations == 0 and eval_asset_violations == 0:
        oks.append("ok eval asset isolation (0 dependency refs + 0 include refs)")
    return oks, rep.lines
