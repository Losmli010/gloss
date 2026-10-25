"""约束门禁·源码卫生：桩副本一致、文件行数上限与残留任务标记——跨包复用
的同名测试桩逐字一致，单个生产源文件不超过五百行（文件头部放行标记注明
缘由后豁免），大写任务标记词全字匹配、命中即失败；项目文档与本模块及其
单测的夹具豁免任务标记自命中。判定口径与取舍见记录文件。
"""

from __future__ import annotations

import os
import re
import subprocess
from typing import Any

from . import manifests

LINE_CAP = 500

SELF_FILES = (
    "AGENTS.md",
    "scripts/hooks/constraints/source_hygiene.py",
    "scripts/hooks/constraints/source_hygiene_test.py",
)


def stub_section(path: str, pat: str, loader: Any = None) -> str:
    """端口桩按节比对：节以“列 0 的 ///”切分（桩的条目文档都在列 0，
    结构体字段文档有缩进，不会误切）。文件缺失按空节处理（对齐 cmp 空对空）。"""
    if not os.path.isfile(path):
        return ""
    out = []
    found = False
    for line in manifests.read_text(path, loader).split("\n"):
        if found:
            if line.startswith("///"):
                break
            out.append(line)
        elif line.startswith("/// %s" % pat):
            found = True
            out.append(line)
    while out and not out[-1].strip():
        out.pop()
    return "\n".join(out)


def run(root: str, loader: Any = None) -> tuple[list[str], list[str]]:
    """执行桩副本一致、行数上限与任务标记约束，返回通过与失败两个清单。"""
    oks = []
    fails = []

    core_stubs = os.path.join(root, "crates", "gloss-core", "tests", "stubs")
    if os.path.isfile(os.path.join(core_stubs, "engine.rs")):
        drifts = 0

        def assert_same(desc: str, text_a: str, text_b: str) -> None:
            nonlocal drifts
            if text_a != text_b:
                manifests.note_violation(
                    fails,
                    "stub copy drift: %s — same-name stubs must be identical;"
                    " sync across crates when changing injected semantics" % desc,
                )
                drifts += 1

        def text_of(path: str) -> str:
            return manifests.read_text(path, loader) if os.path.isfile(path) else ""

        assert_same(
            "engine.rs (core <-> app)",
            text_of(os.path.join(core_stubs, "engine.rs")),
            text_of(os.path.join(root, "crates", "gloss-app", "tests", "stubs", "engine.rs")),
        )
        for crate in ("gloss-app", "gloss-platform"):
            assert_same(
                "memory config store stub (core <-> %s)" % crate,
                stub_section(os.path.join(core_stubs, "ports.rs"), "内存版配置存储桩", loader),
                stub_section(
                    os.path.join(root, "crates", crate, "tests", "stubs", "ports.rs"),
                    "内存版配置存储桩",
                    loader,
                ),
            )
        if drifts == 0:
            oks.append(
                "ok stub copies identical (engine.rs full text + shared port stubs per section)"
            )
    else:
        oks.append("ok stub copies identical (no tests/stubs/, skipped)")

    big_files = 0
    big_allowed = 0
    src_bases = [os.path.join(root, "src")]
    crates_dir = os.path.join(root, "crates")
    if os.path.isdir(crates_dir):
        for name in sorted(os.listdir(crates_dir)):
            src_bases.append(os.path.join(crates_dir, name, "src"))
    for f in manifests.walk_rs(*src_bases):
        lines = manifests.read_text(f, loader).split("\n")
        production = 0
        for line in lines:
            if line.startswith("#[cfg(test)]"):
                break
            production += 1
        if production <= LINE_CAP:
            continue
        if any("lines:allow" in line for line in lines[:5]):
            big_allowed += 1
            continue
        big_files += 1
        manifests.note_violation(
            fails,
            "constraint violation (line cap): %s has %d production lines, over the %d-line"
            " cap — split by responsibility; if a buffer is truly needed, add lines:allow"
            " with a reason in the first 5 lines" % (manifests.rel(root, f), production, LINE_CAP),
        )
    if big_files == 0:
        oks.append(
            "ok line cap (production code <=%d lines; %d lines:allow exemptions)"
            % (LINE_CAP, big_allowed)
        )

    if (
        subprocess.run(
            ["git", "-C", root, "rev-parse", "--is-inside-work-tree"], capture_output=True
        ).returncode
        == 0
    ):
        marker_re = re.compile(r"\b(?:TODO|FIXME|HACK|TBD)\b", re.ASCII)
        if loader is not None:
            tracked = loader.tracked()
        else:
            out = subprocess.run(
                ["git", "-C", root, "ls-files", "-z"], capture_output=True, check=True
            ).stdout
            tracked = [p.decode("utf-8", "replace") for p in out.split(b"\0") if p]
        for f in tracked:
            if f in SELF_FILES:
                continue
            path = os.path.join(root, f)
            try:
                if loader is not None:
                    data = loader.read_bytes(path)
                else:
                    with open(path, "rb") as fh:
                        data = fh.read()
            except OSError:
                continue
            if b"\0" in data:
                continue
            hits = [
                "%d:%s" % (no, line)
                for no, line in enumerate(data.decode("utf-8", "replace").split("\n"), 1)
                if marker_re.search(line)
            ]
            if hits:
                manifests.note_violation(fails, "leftover task marker: %s — %s" % (f, hits[0][:60]))
        oks.append("ok no leftover task markers (whole-word scan of git-tracked files)")

    return oks, fails
