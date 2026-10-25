"""约束门禁·架构：依赖方向与评测资产隔离——各包只能依赖允许的边，核心库
（gloss-core）不得引入平台与渲染栈；评测包（gloss-eval）不被任何清单依赖，
评测资产不被生产代码内嵌；包名必须在项目文档登记过，挡住边表与文档的单边
改动。允许边表与红线的取舍见记录文件。
"""

from __future__ import annotations

import os
import re
from typing import Any

from . import manifests

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


def core_forbidden(key: str) -> bool:
    return key in CORE_FORBIDDEN_EXACT or key.startswith(CORE_FORBIDDEN_PREFIX)


def run(root: str, loader: Any = None) -> tuple[list[str], list[str]]:
    """执行依赖方向与评测资产隔离约束，返回通过与失败两个清单。"""
    manifest_list = manifests.manifests(root)
    dump = manifests.dep_dump(root, loader)
    oks = []
    fails = []
    edges = 0
    for owner, mrel, _sec, key, start, text in dump:
        if manifests.is_path_dep(text):
            edges += 1
            allowed = ALLOWED_OWN_DEPS.get(owner, "")
            if key not in allowed.split():
                manifests.note_violation(
                    fails,
                    "constraint violation (dependency direction): %s must not depend on"
                    " %s (%s:%d); allowed edges: %s"
                    % (os.path.basename(owner), key, mrel, start, allowed or "none"),
                )
        elif owner == "gloss-core" and core_forbidden(key):
            manifests.note_violation(
                fails,
                "constraint violation (dependency direction): gloss-core has a"
                " platform/render stack dependency %s (%s:%d) — red-line, forbidden"
                % (key, mrel, start),
            )
    agents_md = os.path.join(root, "AGENTS.md")
    if os.path.isfile(agents_md):
        agents_text = manifests.read_text(agents_md, loader)
        for m in manifest_list:
            name = manifests.pkg_name(m, loader)
            if name and name not in agents_text:
                manifests.note_violation(
                    fails,
                    "constraint violation (dependency direction): crate %s missing from"
                    " the dependency direction notes in %s"
                    % (name, manifests.rel(root, agents_md)),
                )
    oks.append(
        "ok dependency direction (%d in-repo dependency edges + crate names registered)" % edges
    )

    eval_dep_violations = 0
    for owner, mrel, _sec, key, start, _text in dump:
        if key == "gloss-eval" and owner != "gloss-eval":
            manifests.note_violation(
                fails,
                "constraint violation (eval asset isolation): %s depends on gloss-eval"
                " (%s:%d) — the eval toolchain must stay out of the production"
                " dependency graph" % (owner, mrel, start),
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
        hits = [
            line
            for line in manifests.read_text(f, loader).split("\n")
            if re.search(r"include_str!\(.*gloss-eval|include_bytes!\(.*gloss-eval", line)
        ]
        if not hits:
            continue
        eval_asset_violations += 1
        manifests.note_violation(
            fails,
            "constraint violation (eval asset isolation): %s embeds a gloss-eval asset"
            " into production code (%s)" % (manifests.rel(root, f), hits[0][:60]),
        )
    if eval_dep_violations == 0 and eval_asset_violations == 0:
        oks.append("ok eval asset isolation (0 dependency refs + 0 include refs)")
    return oks, fails
