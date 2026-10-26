"""约束门禁·源码卫生：桩副本一致、文件行数上限与残留任务标记——跨包复用
的同名测试桩逐字一致，单个生产源文件不超过五百行（文件头部放行标记注明
缘由后豁免，缺失即判罚），大写任务标记词全字匹配、命中即失败；项目文档与
本模块及其单测的夹具豁免任务标记自命中。源码扫描面为空按失败关闭退出；非
版本库环境响亮跳过任务标记段。违规经统一诊断出口带规则标识报告，证据行完
整不截断。判定口径与取舍见记录文件。
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
from typing import Any

import gate_report

from . import manifests

RULE_STUB_DRIFT = "STUB-DRIFT"
RULE_LINE_CAP = "LINE-CAP"
RULE_TASK_MARKER = "TASK-MARKER"

STUB_DRIFT_REMEDY = "sync the same-name stubs across crates when changing injected semantics"
LINE_CAP_REMEDY = (
    "split by responsibility; if a buffer is truly needed, add lines:allow with a"
    " reason in the first 5 lines"
)
TASK_MARKER_REMEDY = "delete the marker or track the work in the plan document"

LINE_CAP = 500

SELF_FILES = (
    "AGENTS.md",
    "scripts/hooks/constraints/source_hygiene.py",
    "scripts/hooks/constraints/source_hygiene_test.py",
)

MARKER_RE = re.compile(r"\b(?:TODO|FIXME|HACK|TBD)\b", re.ASCII)

EMPTY_SOURCE_MESSAGE = "the source scan surface is empty: no .rs files found under the source trees"

EMPTY_SOURCE_REMEDY = (
    "run the gate from the workspace root; an empty source surface means the source"
    " checks cannot complete"
)

SKIP_LINE = "  ⚠ task-marker scan skipped, not a git work tree: %s"


def work_tree_status(root: str) -> str:
    """探测根目录的版本库状态：在工作树内返回空串，否则返回原因；版本库可
    执行文件缺失属门禁自身故障，失败关闭退出。"""
    try:
        proc = subprocess.run(
            ["git", "-C", root, "rev-parse", "--is-inside-work-tree"], capture_output=True
        )
    except FileNotFoundError:
        gate_report.fail(
            gate_report.infra_failure("git executable not found: cannot enumerate tracked files")
        )
        sys.exit(gate_report.EXIT_INFRA)
    if proc.returncode == 0:
        return ""
    detail = " ".join(proc.stderr.decode("utf-8", "replace").split())
    return detail or "git rev-parse failed with exit code %d" % proc.returncode


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


def run(
    root: str, loader: Any = None, report: manifests.Reporter | None = None
) -> tuple[list[str], list[str]]:
    """执行桩副本一致、行数上限与任务标记约束，返回通过与失败两个清单。"""
    rep = report if report is not None else manifests.Reporter()
    oks = []

    core_stubs = os.path.join(root, "crates", "gloss-core", "tests", "stubs")
    if os.path.isfile(os.path.join(core_stubs, "engine.rs")):
        drifts = 0

        def assert_same(desc: str, text_a: str, text_b: str) -> None:
            nonlocal drifts
            if text_a != text_b:
                rep.note(
                    gate_report.Finding(
                        rule=RULE_STUB_DRIFT,
                        path="",
                        line=0,
                        message="stub copy drift: %s — same-name stubs must be identical" % desc,
                        remedy=STUB_DRIFT_REMEDY,
                    )
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
    src_count = 0
    src_bases = [os.path.join(root, "src")]
    crates_dir = os.path.join(root, "crates")
    if os.path.isdir(crates_dir):
        for name in sorted(os.listdir(crates_dir)):
            src_bases.append(os.path.join(crates_dir, name, "src"))
    for f in manifests.walk_rs(*src_bases):
        src_count += 1
        lines = manifests.read_text(f, loader).split("\n")
        production = 0
        for line in lines:
            if line.startswith("#[cfg(test)]"):
                break
            production += 1
        if production <= LINE_CAP:
            continue
        allowed = [(no, line) for no, line in enumerate(lines[:5], 1) if "lines:allow" in line]
        if allowed:
            big_allowed += 1
            for no, line in allowed:
                if gate_report.allow_reason_missing("lines:allow", line):
                    rep.note(
                        gate_report.allow_reason_finding(
                            "lines:allow", manifests.rel(root, f), no, line
                        )
                    )
            continue
        big_files += 1
        rep.note(
            gate_report.Finding(
                rule=RULE_LINE_CAP,
                path=manifests.rel(root, f),
                line=0,
                message="line cap: %d production lines, over the %d-line cap"
                % (production, LINE_CAP),
                remedy=LINE_CAP_REMEDY,
            )
        )
    if src_count == 0:
        gate_report.fail(
            gate_report.Finding(
                rule=gate_report.RULE_EMPTY_SURFACE,
                path="",
                line=0,
                message=EMPTY_SOURCE_MESSAGE,
                remedy=EMPTY_SOURCE_REMEDY,
            )
        )
        sys.exit(gate_report.EXIT_INFRA)
    if big_files == 0:
        oks.append(
            "ok line cap (production code <=%d lines; %d lines:allow exemptions)"
            % (LINE_CAP, big_allowed)
        )

    skip_reason = work_tree_status(root)
    if not skip_reason:
        if loader is not None:
            tracked = loader.tracked()
        else:
            out = gate_report.run_git(["ls-files", "-z"], cwd=root).stdout
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
            hit_line = 0
            hit_text = ""
            for no, line in enumerate(data.decode("utf-8", "replace").split("\n"), 1):
                if MARKER_RE.search(line):
                    hit_line = no
                    hit_text = line
                    break
            if hit_line:
                rep.note(
                    gate_report.Finding(
                        rule=RULE_TASK_MARKER,
                        path=f,
                        line=hit_line,
                        message="leftover task marker: whole-word match in a git-tracked file",
                        evidence=hit_text,
                        remedy=TASK_MARKER_REMEDY,
                    )
                )
        oks.append("ok no leftover task markers (whole-word scan of git-tracked files)")
    else:
        print(SKIP_LINE % skip_reason, file=sys.stderr)

    return oks, rep.lines
