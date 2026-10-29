#!/usr/bin/env python3
"""指令文档引用一致性门禁：校验指令文档里提到的仓库事实确实存在。
本地提交前门禁与持续集成检查共用此脚本，保证本地与远端判定一致。

校验对象为仓库的指令文档，五类引用：
  一、全文任何任务配方的提及必须在配方清单里定义；
  二、行内反引号里的仓库路径（含分隔符）必须存在（文件或目录）；
  三、行内反引号里的裸文件名（配方清单或带常见扩展名）必须在仓库根存在；
  四、全文任何测试目标的提及必须存在对应的测试源码（根包或任一分包）；
  五、仓库各处对质量条目的指名引用必须真的在指令文档的“质量条目总表”段里。

扫描逐行进行，代码块内同样算数；路径只认行内反引号包裹的仓库相对路径。
占位符、仓库外路径、注释语法与链接不校验。路径、受检文件与测试目标都以
版本库跟踪清单为准：工作区里未跟踪的临时目录与本地记录不参与判定，本地
与持续集成才看同一批文件。违规报告经统一诊断出口打印：每条带规则标识、
位置、违规行原文与修复指引；受检的指令文档文件本身缺失按无法完成判定失
败关闭退出。退出码按全脚本统一约定。

用法：python3 scripts/hooks/check_agents_doc.py <instruction-doc>，缺省为仓库根的指令文档。
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
from typing import Any

import gate_report
import rg_engine

RULE_DOC_RECIPE = "DOC-RECIPE"
RULE_DOC_PATH = "DOC-PATH"
RULE_DOC_FILE = "DOC-FILE"
RULE_DOC_TEST = "DOC-TEST"
RULE_DOC_RULE_REF = "DOC-RULE-REF"
RULE_DOC_INPUT = "DOC-INPUT"

DOC_INPUT_REMEDY = (
    "the client only accepts AGENTS.md (no AGENT.md-style aliases); do not rename"
    " or move it — restore the file, then rerun the gate"
)
CHANGED_REMEDY = (
    "command or path really changed: fix the doc; do not let it describe a world"
    " that no longer exists"
)
PLACEHOLDER_REMEDY = (
    "example or placeholder only: reword it, or wrap it in * < > ~ to skip validation"
)
RENAMED_REMEDY = (
    "entry renamed: update every citation site (cite by name in the AGENTS.md named-entry form)"
)

RECIPE_RE = re.compile(r"(?:^|[^A-Za-z0-9_-])just ([a-z][a-z0-9-]*)")
BACKTICK_RE = re.compile(r"`([^`]+)`")
CARGO_TEST_RE = re.compile(r"--test ([A-Za-z0-9_-]+)")
RULE_NAME_RE = re.compile(r"\*\*([^*]+)\*\*")
# 引用行先经共享检索引擎（ripgrep）筛出，条目名再在命中行上摘取
REF_LINE_EXPRESSION = r"AGENTS\.md.*质量条目“"
REF_NAME_RE = re.compile(r"质量条目“([^”]+)”")
BARE_SUFFIXES = (".md", ".toml", ".py", ".rs", ".yml", ".json")

# 本脚本与其单测按构造就含质量条目引用模式，跳过自命中
SELF_EXEMPT = {"check_agents_doc.py", "check_agents_doc_test.py"}


def scan_lines(doc_path: str, loader: Any = None) -> list[str]:
    if loader is not None:
        return loader.read_text(doc_path).split("\n")
    with open(doc_path, encoding="utf-8") as fh:
        return fh.read().split("\n")


def rule_names(lines: list[str]) -> list[str]:
    """“质量条目总表”段里以 **名字** 起头的条目名。"""
    names = []
    in_section = False
    for line in lines:
        if line.startswith("### 质量条目总表"):
            in_section = True
            continue
        if in_section and line.startswith("## "):
            break
        if in_section:
            names.extend(RULE_NAME_RE.findall(line))
    return names


def scan_targets(root: str, target: str, tracked: list[str]) -> bool:
    """测试目标是否有对应源码：有跟踪清单时按清单，否则按工作区。"""
    if tracked:
        leaf = ["tests", "%s.rs" % target]
        return any(
            p == "tests/%s.rs" % target or (p.startswith("crates/") and p.split("/")[2:4] == leaf)
            for p in tracked
        )
    candidates = [os.path.join(root, "tests", "%s.rs" % target)]
    crates = os.path.join(root, "crates")
    if os.path.isdir(crates):
        for crate in sorted(os.listdir(crates)):
            candidates.append(os.path.join(crates, crate, "tests", "%s.rs" % target))
    return any(os.path.isfile(c) for c in candidates)


def tracked_files(root: str) -> list[str]:
    """版本库跟踪清单；不是仓库根时返回空表，调用方退回工作区遍历。

    空字节分隔取原始字节再容错解码：非空分隔形态对非英文字节（ASCII）文
    件名做引号八进制转义，拿到的是打引号的假路径（引用核对误报、受检文件
    静默漏检）。"""
    try:
        top = subprocess.run(
            ["git", "-C", root, "rev-parse", "--show-toplevel"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()
        if os.path.realpath(top) != os.path.realpath(root):
            return []
        out = subprocess.run(
            ["git", "-C", root, "ls-files", "-z"],
            capture_output=True,
            check=True,
        ).stdout
        return [p.decode("utf-8", "replace") for p in out.split(b"\0") if p]
    except (OSError, subprocess.CalledProcessError):
        return []


def repo_path_exists(token: str, tracked: list[str]) -> bool:
    """路径是否在跟踪清单里：目录按前缀、文件按全名。"""
    if token.endswith("/"):
        return any(p.startswith(token) for p in tracked)
    return token in tracked or any(p.startswith(token + "/") for p in tracked)


def ref_file(p: str) -> bool:
    """是否参与质量条目引用核对：源码、清单、脚本与工作流，另加任务配方。"""
    name = os.path.basename(p)
    return name.endswith((".rs", ".toml", ".py", ".yml")) or name == "justfile"


def scan_quality_refs(
    root: str, names: list[str], tracked: list[str], loader: Any = None
) -> tuple[list[gate_report.Finding], int, int]:
    """仓库各处（*.rs / *.toml / *.py / *.yml / justfile）对质量条目的指名引用核对。

    有跟踪清单时只扫清单里的文件：工作区里未跟踪的临时目录与本地记录不能
    左右判定，本地与持续集成才看同一批文件；行筛选委托共享检索引擎，命中
    行上摘取条目名核对。返回 (violations, ref_files, ref_count)。"""
    violations = []
    ref_files = 0
    ref_count = 0
    if tracked:
        targets = [p for p in tracked if ref_file(p)]
    else:
        targets = []
        for dirpath, dirnames, filenames in os.walk(root):
            dirnames[:] = sorted(d for d in dirnames if d not in ("target", ".git"))
            for name in sorted(filenames):
                if ref_file(name):
                    targets.append(os.path.relpath(os.path.join(dirpath, name), root))
    targets = [rel for rel in targets if os.path.basename(rel) not in SELF_EXEMPT]
    ref_files = len(targets)
    try:
        hits = rg_engine.run(root, [REF_LINE_EXPRESSION], targets)
    except RuntimeError as exc:
        gate_report.fail(
            gate_report.infra_failure("cannot complete the quality-entry citation scan: %s" % exc)
        )
        sys.exit(gate_report.EXIT_INFRA)
    for rel in targets:
        file_hits = hits.get(rel, {})
        for line_no in sorted(file_hits):
            line = file_hits[line_no]
            for cited in REF_NAME_RE.findall(line):
                ref_count += 1
                if cited not in names:
                    violations.append(
                        gate_report.Finding(
                            rule=RULE_DOC_RULE_REF,
                            path=rel,
                            line=line_no,
                            message='cites quality entry "%s" — no entry with this name'
                            " in the AGENTS.md quality entry table" % cited,
                            evidence=line.strip(),
                            remedy=RENAMED_REMEDY,
                        )
                    )
    return violations, ref_files, ref_count


def require_doc(doc_path: str) -> None:
    """受检文档在场守卫：文件缺失即无法完成判定，失败关闭退出。"""
    if os.path.isfile(doc_path):
        return
    gate_report.fail(
        gate_report.Finding(
            rule=RULE_DOC_INPUT,
            path=doc_path,
            line=0,
            message="instruction file not found",
            remedy=DOC_INPUT_REMEDY,
        )
    )
    sys.exit(gate_report.EXIT_INFRA)


def collect(doc_path: str, loader: Any = None) -> tuple[list[gate_report.Finding], int, int, int]:
    """按五类引用核对 doc_path，违规随执行流即时经统一诊断出口打到标准错误，
    返回 (violations, checked, ref_files, ref_count)；装入器在场时清单枚举与
    文件读取走共享缓存，缺省自行读取。"""
    require_doc(doc_path)
    root = os.path.dirname(os.path.abspath(doc_path))
    doc_name = os.path.basename(doc_path)
    tracked = loader.tracked() if loader is not None else tracked_files(root)
    lines = scan_lines(doc_path, loader)
    justfile_lines = None
    justfile_path = os.path.join(root, "justfile")
    if os.path.isfile(justfile_path):
        if loader is not None:
            justfile_lines = loader.read_text(justfile_path).split("\n")
        else:
            with open(justfile_path, encoding="utf-8") as fh:
                justfile_lines = fh.read().split("\n")

    found: list[gate_report.Finding] = []
    checked = 0

    def emit(finding: gate_report.Finding) -> None:
        found.append(finding)
        gate_report.fail(finding)

    for line_no, line in enumerate(lines, 1):
        # ---- 任务配方 ----
        for recipe in RECIPE_RE.findall(line):
            checked += 1
            defined = False
            for jl in justfile_lines or []:
                if re.match(r"^%s(\s[^:]+)?:(\s|$)" % recipe, jl):
                    defined = True
                    break
            if not defined:
                emit(
                    gate_report.Finding(
                        rule=RULE_DOC_RECIPE,
                        path=doc_name,
                        line=line_no,
                        message='recipe "just %s" not found in justfile' % recipe,
                        evidence=line.strip(),
                        remedy=CHANGED_REMEDY,
                    )
                )

        # ---- 行内反引号里的仓库路径与裸文件名 ----
        for token in BACKTICK_RE.findall(line):
            if "*" in token or "<" in token or ">" in token or "//" in token:
                continue
            if token.startswith("~"):
                continue
            if "/" in token:
                checked += 1
                exists = (
                    repo_path_exists(token, tracked)
                    if tracked
                    else os.path.exists(os.path.join(root, token))
                )
                if not exists:
                    emit(
                        gate_report.Finding(
                            rule=RULE_DOC_PATH,
                            path=doc_name,
                            line=line_no,
                            message='path "%s" does not exist in the repo' % token,
                            evidence=line.strip(),
                            remedy=PLACEHOLDER_REMEDY,
                        )
                    )
            elif token == "justfile" or token.endswith(BARE_SUFFIXES):
                checked += 1
                exists = (
                    repo_path_exists(token, tracked)
                    if tracked
                    else os.path.exists(os.path.join(root, token))
                )
                if not exists:
                    emit(
                        gate_report.Finding(
                            rule=RULE_DOC_FILE,
                            path=doc_name,
                            line=line_no,
                            message='file "%s" does not exist at the repo root' % token,
                            evidence=line.strip(),
                            remedy=PLACEHOLDER_REMEDY,
                        )
                    )

        # ---- 测试目标 ----
        for target in CARGO_TEST_RE.findall(line):
            checked += 1
            if not scan_targets(root, target, tracked):
                emit(
                    gate_report.Finding(
                        rule=RULE_DOC_TEST,
                        path=doc_name,
                        line=line_no,
                        message="cargo test --test %s: no tests/%s.rs found" % (target, target),
                        evidence=line.strip(),
                        remedy=CHANGED_REMEDY,
                    )
                )

    # ---- 仓库各处对质量条目的指名引用必须存在 ----
    ref_violations, ref_files, ref_count = scan_quality_refs(
        root, rule_names(lines), tracked, loader
    )
    for finding in ref_violations:
        emit(finding)
    return found, checked, ref_files, ref_count


def render(finding: gate_report.Finding) -> str:
    """违规渲染行：消息加位置，作为运行函数返回清单里的条目。"""
    loc = gate_report.location(finding)
    return "%s (%s)" % (finding.message, loc) if loc else finding.message


def run(doc_path: str, loader: Any = None) -> tuple[list[str], int, int, int]:
    """按五类引用核对 doc_path，返回 (violations, checked, ref_files, ref_count)；
    违规清单收渲染行（消息加位置）供调用方取用，装入器在场时清单枚举与文件
    读取走共享缓存，缺省自行读取。"""
    found, checked, ref_files, ref_count = collect(doc_path, loader)
    return [render(finding) for finding in found], checked, ref_files, ref_count


def usage_failure(detail: str) -> int:
    print("usage: check_agents_doc.py [instruction-doc]", file=sys.stderr)
    print("  %s" % detail, file=sys.stderr)
    return gate_report.EXIT_USAGE


def main(loader: Any = None) -> int:
    if len(sys.argv) > 2:
        return usage_failure("too many arguments: expected at most one instruction-doc path")
    if len(sys.argv) >= 2:
        arg = os.path.abspath(sys.argv[1])
    else:
        arg = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "AGENTS.md")
    found, checked, ref_files, ref_count = collect(arg, loader)
    doc_name = os.path.basename(arg)
    gate_report.banner(
        "agents-doc reference gate",
        "repo root: %s" % os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..")),
    )
    if found:
        gate_report.finish(found)
        return gate_report.EXIT_VIOLATION
    print(
        "ok: %s references consistent (%d checked: just recipes / repo paths / test targets;"
        " plus %d named quality-entry citations across %d files)"
        % (doc_name, checked, ref_count, ref_files)
    )
    return gate_report.EXIT_PASS


if __name__ == "__main__":
    sys.exit(main())
