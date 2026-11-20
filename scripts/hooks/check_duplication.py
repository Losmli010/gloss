#!/usr/bin/env python3
"""克隆检测门禁：子进程调检测引擎（jscpd）出报告，判罚口径全部收口本脚本，
引擎只当候选克隆清单的提供者。受检面为版本库跟踪清单里两棵源码树的全部本语
言源文件，按跟踪清单过滤后显式传参给引擎，未跟踪文件不进扫描面；并轨运行时
跟踪清单经共享装入器取用（见 run_text_gates），单跑自行枚举；资产、数据
集与提示词模板不收。

对引擎报出的每条克隆实例按序判定，短路即出：其一，就地放行——克隆对任一侧
起始行带放行标记且缘由在场，放行该条实例（粒度是单条克隆对）；标记在场而缘
由缺席即判放行缘由条目，放行不成立，落回后续判定。其二，基线放行——两侧文
件对在基线登记，且门禁线宽下该文件对的全部克隆行合计不超过登记上限，放行；
合计超限即整对不放行（冻结合计口径）。其三，两关未过即新克隆红牌。基线体检
模式报出克隆行合计归零的陈旧条目，支持一键清理；观察模式按观察线宽出全量克
隆清单与聚合计数，不判罚；任务包模式按克隆对逐个拼装判定辅助包（三问内核与
仓库覆盖层、克隆对上下文与确定性预填建议、验证清单、输出契约），可传克隆对
标识或路径片段筛选。

违规报告经统一诊断出口打印，退出码按全脚本统一约定：零为通过、一为发现违规、
二为用法错误、三为无法完成判定（失败关闭）。判罚失败的输出末尾带判定辅助入
口指针，失败即给出下一步动作。

用法：python3 scripts/hooks/check_duplication.py [--report | --triage | doctor [--fix]] [repo-root]，缺省为判罚模式；仓库根缺省取本脚本位置向上回溯。任务包模式的筛选值以等号接在其旗标后，取克隆对标识或路径片段。
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from typing import Any, Callable

import tomllib

_HOOKS = os.path.dirname(os.path.abspath(__file__))
_DEV = os.path.abspath(os.path.join(_HOOKS, "..", "dev"))
for _entry in (_HOOKS, _DEV):
    if _entry not in sys.path:
        sys.path.insert(0, _entry)

import gate_report  # noqa: E402
import tool_version  # noqa: E402

REPO_ROOT = os.path.abspath(os.path.join(_HOOKS, "..", ".."))
BASELINE_PATH = os.path.join(_HOOKS, "duplication_baseline.toml")
PROMPT_PATH = os.path.join(_HOOKS, "duplication_triage.md")

ENGINE_BIN = "jscpd"
REPORT_NAME = "jscpd-report.json"
TOOL_SECTION = "jscpd"
GUARDED_TREES = ("crates/", "src/")
SOURCE_SUFFIX = ".rs"

GATE_MIN_TOKENS = 150
REPORT_MIN_TOKENS = 60
FRAGMENT_CAP = 40
TEST_MODULE_MARKERS = ("#[test]", "#[cfg(test)]", "#[tokio::test]", "mod tests")

RULE_DUP_NEW = "DUP-NEW"
RULE_DUP_ALLOW_REASON = "DUP-ALLOW-REASON"
RULE_DUP_STALE = "DUP-STALE"

ALLOW_MARKER = "dup:allow"

DUP_NEW_REMEDY = (
    "existing clone: register the file pair in duplication_baseline.toml with a"
    " frozen line total and a reason, or add dup:allow with a reason on a clone"
    " start line; genuinely new duplication: refactor it away — never widen the"
    " baseline to absorb it"
)

NODE_A_POINTER = "triage: run `just dup-triage` and hand its output to your coding agent"

BASELINE_FIELDS = ("first", "second", "max_lines", "kind", "reason", "registered")
BASELINE_KINDS = ("todo", "retain")

USAGE = "usage: check_duplication.py [--report | --triage[=clone-id] | doctor [--fix]] [repo-root]"


class GateInfra(RuntimeError):
    """无法完成判定的异常：引擎、基线或受检面故障，按失败关闭处置。"""


@dataclass(frozen=True)
class CloneSide:
    """克隆实例的单侧：仓库相对路径与一行制闭区间行号。"""

    path: str
    start: int
    end: int


@dataclass(frozen=True)
class ClonePair:
    """一条克隆实例：两侧与克隆行数；配对键按两侧路径字典序归一。"""

    first: CloneSide
    second: CloneSide

    @property
    def lines(self) -> int:
        return self.first.end - self.first.start + 1

    @property
    def key(self) -> tuple[str, str]:
        return tuple(sorted((self.first.path, self.second.path)))

    @property
    def ident(self) -> str:
        return "%s:%d..%s:%d" % (
            self.first.path,
            self.first.start,
            self.second.path,
            self.second.start,
        )

    def anchor(self) -> CloneSide:
        """报告锚点侧：路径落在配对键首位的一侧，保证输出与配对键同序确定。"""
        return self.first if self.first.path == self.key[0] else self.second

    def other(self) -> CloneSide:
        return self.second if self.first.path == self.key[0] else self.first


@dataclass(frozen=True)
class BaselineEntry:
    """基线条目：文件对与冻结的克隆行合计上限，带处置类别、缘由与登记日期。"""

    first: str
    second: str
    max_lines: int
    kind: str
    reason: str
    registered: str

    @property
    def key(self) -> tuple[str, str]:
        return tuple(sorted((self.first, self.second)))


def surface_files(tracked: list[str]) -> list[str]:
    """受检面过滤：两棵守卫树下的本语言源文件，按跟踪清单原序保持确定。"""
    return [rel for rel in tracked if rel.startswith(GUARDED_TREES) and rel.endswith(SOURCE_SUFFIX)]


def normalize_report(report: dict[str, Any], root: str) -> list[ClonePair]:
    """引擎报告到克隆实例清单：绝对路径剥根前缀归一为仓库相对路径，位置字段
    缺失或路径越界即报告损坏，失败关闭。"""
    if "duplicates" not in report or "statistics" not in report:
        raise GateInfra("engine report misses duplicates or statistics")
    prefix = root.rstrip(os.sep) + os.sep
    out = []
    for item in report["duplicates"]:
        sides = []
        for field in ("firstFile", "secondFile"):
            blob = item.get(field)
            if not isinstance(blob, dict):
                raise GateInfra("engine report clone entry misses %s" % field)
            name = blob.get("name")
            start = blob.get("start")
            end = blob.get("end")
            if not isinstance(name, str) or not isinstance(start, int) or not isinstance(end, int):
                raise GateInfra("engine report clone entry has malformed %s" % field)
            if not name.startswith(prefix):
                raise GateInfra("engine reported a path outside the repo root: %s" % name)
            if start < 1 or end < start:
                raise GateInfra("engine reported an invalid line range: %s" % name)
            sides.append(CloneSide(os.path.relpath(name, root), start, end))
        out.append(ClonePair(sides[0], sides[1]))
    return out


def pair_totals(clones: list[ClonePair]) -> dict[tuple[str, str], int]:
    """文件对到克隆行合计的映射：合计口径在判定与体检两处共用，单点收口。"""
    totals: dict[tuple[str, str], int] = {}
    for clone in clones:
        totals[clone.key] = totals.get(clone.key, 0) + clone.lines
    return totals


def load_baseline(path: str) -> list[BaselineEntry]:
    """基线装入与形态校验：字段缺席、类别越界、上限非正整数、同文件对重复登
    记都按基线损坏处置，失败关闭。"""
    try:
        with open(path, "rb") as fh:
            data = tomllib.load(fh)
    except OSError as exc:
        raise GateInfra("baseline unreadable: %s" % exc) from None
    except tomllib.TOMLDecodeError as exc:
        raise GateInfra("baseline is not valid TOML: %s" % exc) from None
    entries = data.get("entry")
    if entries is None:
        return []
    if not isinstance(entries, list):
        raise GateInfra("baseline entry table is not an array")
    out = []
    seen: set[tuple[str, str]] = set()
    for raw in entries:
        if not isinstance(raw, dict):
            raise GateInfra("baseline entry is not a table")
        unknown = sorted(set(raw) - set(BASELINE_FIELDS))
        if unknown:
            raise GateInfra("baseline entry carries unknown fields: %s" % ", ".join(unknown))
        missing = [f for f in BASELINE_FIELDS if f not in raw]
        if missing:
            raise GateInfra("baseline entry misses fields: %s" % ", ".join(missing))
        first, second, kind, reason, registered = (
            raw[f] for f in BASELINE_FIELDS if f != "max_lines"
        )
        max_lines = raw["max_lines"]
        for label, value in (
            ("first", first),
            ("second", second),
            ("kind", kind),
            ("reason", reason),
            ("registered", registered),
        ):
            if not isinstance(value, str) or not value.strip():
                raise GateInfra("baseline entry field %s is not a non-empty string" % label)
        if kind not in BASELINE_KINDS:
            raise GateInfra(
                "baseline entry kind %r is not one of: %s" % (kind, ", ".join(BASELINE_KINDS))
            )
        if isinstance(max_lines, bool) or not isinstance(max_lines, int) or max_lines <= 0:
            raise GateInfra("baseline entry max_lines is not a positive integer")
        entry = BaselineEntry(first, second, max_lines, kind, reason, registered)
        if entry.key in seen:
            raise GateInfra("baseline registers the same file pair twice: %s <-> %s" % entry.key)
        seen.add(entry.key)
        out.append(entry)
    return out


def baseline_index(entries: list[BaselineEntry]) -> dict[tuple[str, str], BaselineEntry]:
    """基线索引：文件对配对键到条目。"""
    return {entry.key: entry for entry in entries}


def load_lines(root: str, paths: set[str]) -> dict[str, list[str]]:
    """克隆两侧源文件行文装入：文件缺失即无法判定，失败关闭。"""
    out = {}
    for rel in sorted(paths):
        try:
            with open(os.path.join(root, rel), encoding="utf-8", errors="replace") as fh:
                out[rel] = fh.read().split("\n")
        except OSError as exc:
            raise GateInfra("clone side unreadable: %s" % exc) from None
    return out


def marker_sides(
    clone: ClonePair, lines_by_file: dict[str, list[str]]
) -> tuple[bool, list[CloneSide]]:
    """就地放行标记的两面判定：返回（缘由在场的放行是否成立，缺缘由的侧清单）。
    任一侧起始行带标记即进入判定，缘由缺席的侧逐个上报，放行语义按口径成立
    与否返回。"""
    granted = False
    bare: list[CloneSide] = []
    for side in (clone.first, clone.second):
        lines = lines_by_file[side.path]
        if side.start > len(lines):
            raise GateInfra("engine line range exceeds the file: %s" % side.path)
        text = lines[side.start - 1]
        if ALLOW_MARKER not in text:
            continue
        if gate_report.allow_reason_missing(ALLOW_MARKER, text):
            bare.append(side)
        else:
            granted = True
    return granted, bare


def allow_reason_finding(side: CloneSide, text: str) -> gate_report.Finding:
    """放行标记缺缘由的条目：校验复用统一出口的同款判定，标识换克隆门禁专用名。"""
    return gate_report.Finding(
        rule=RULE_DUP_ALLOW_REASON,
        path=side.path,
        line=side.start,
        message="%s marker without a reason" % ALLOW_MARKER,
        evidence=text.strip(),
        remedy=gate_report.ALLOW_REASON_REMEDY,
    )


def dup_new_finding(
    clone: ClonePair, entry: BaselineEntry | None, total: int
) -> gate_report.Finding:
    """新克隆红牌：基线在场而合计超限时，证据点名冻结合计与实际合计，其余情况
    点名两侧区间。"""
    anchor = clone.anchor()
    other = clone.other()
    if entry is not None:
        evidence = "pair total %d lines exceeds the frozen max_lines %d" % (total, entry.max_lines)
    else:
        evidence = "%s:%d-%d <-> %s:%d-%d" % (
            other.path,
            other.start,
            other.end,
            anchor.path,
            anchor.start,
            anchor.end,
        )
    return gate_report.Finding(
        rule=RULE_DUP_NEW,
        path=anchor.path,
        line=anchor.start,
        message="unregistered clone (%d duplicated lines)" % clone.lines,
        evidence=evidence,
        remedy=DUP_NEW_REMEDY,
    )


def judge_clones(
    clones: list[ClonePair],
    entries: list[BaselineEntry],
    lines_by_file: dict[str, list[str]],
) -> list[gate_report.Finding]:
    """判罚主口径：就地放行（单条实例粒度）→ 基线放行（文件对合计粒度）→ 红
    牌，按序短路；缺缘由条目不夺走后续判定的资格，与放行标记语义并轨。"""
    index = baseline_index(entries)
    totals = pair_totals(clones)
    findings: list[gate_report.Finding] = []
    for clone in sorted(clones, key=lambda c: (c.key, c.first.start, c.second.start)):
        granted, bare = marker_sides(clone, lines_by_file)
        if not granted:
            entry = index.get(clone.key)
            total = totals[clone.key]
            if entry is None or total > entry.max_lines:
                findings.append(dup_new_finding(clone, entry, total))
        for side in bare:
            findings.append(allow_reason_finding(side, lines_by_file[side.path][side.start - 1]))
    return sorted(findings, key=lambda f: (f.path, f.line, f.rule))


def stale_entries(
    entries: list[BaselineEntry], totals: dict[tuple[str, str], int]
) -> list[BaselineEntry]:
    """陈旧基线条目：文件对在门禁线宽下克隆行合计归零（含文件对整对消失）。
    只报告不判罚，清理走一键清理。"""
    return [entry for entry in entries if totals.get(entry.key, 0) == 0]


def stale_findings(stale: list[BaselineEntry]) -> list[gate_report.Finding]:
    """陈旧条目的报告形态：位置取条目首侧路径，证据点名文件对与登记上限。"""
    return [
        gate_report.Finding(
            rule=RULE_DUP_STALE,
            path=entry.first,
            line=0,
            message="baseline entry has no clones left at the gate width",
            evidence="pair: %s <-> %s; entry max_lines %d"
            % (entry.first, entry.second, entry.max_lines),
            remedy="rerun with --fix to prune stale baseline entries",
        )
        for entry in stale
    ]


def prune_baseline(path: str, stale: list[BaselineEntry]) -> int:
    """一键清理：陈旧条目剔除后按确定形态重写基线，前置说明（文件头）保留，
    返回剔除数。剩余条目按原序输出，字段序固定，值按基本字符串转义。"""
    stale_keys = {entry.key for entry in stale}
    try:
        entries = load_baseline(path)
    except GateInfra:
        entries = []
    kept = [e for e in entries if e.key not in stale_keys]
    with open(path, encoding="utf-8") as fh:
        head = fh.read().split("[[entry]]")[0]
    blocks = []
    for e in kept:
        blocks.append(
            "[[entry]]\n"
            'first = "%s"\n'
            'second = "%s"\n'
            "max_lines = %d\n"
            'kind = "%s"\n'
            'reason = "%s"\n'
            'registered = "%s"\n' % (e.first, e.second, e.max_lines, e.kind, e.reason, e.registered)
        )
    with open(path, "w", encoding="utf-8") as fh:
        fh.write(head + "".join(blocks))
    return len(stale_keys)


def cross_platform(clone: ClonePair) -> bool:
    """确定性预填：克隆对跨平台目录对（一侧系统（macOS）树、一侧系统（Windows）树）。"""

    def has(path: str, part: str) -> bool:
        return part in path.split("/")

    return (has(clone.first.path, "macos") and has(clone.second.path, "windows")) or (
        has(clone.first.path, "windows") and has(clone.second.path, "macos")
    )


def test_side(clone: ClonePair, lines_by_file: dict[str, list[str]]) -> bool:
    """确定性预填：任一侧落在测试模块——测试目录内，或片段带测试模块记号。"""
    for side in (clone.first, clone.second):
        parts = side.path.split("/")
        if "tests" in parts:
            return True
        lines = lines_by_file[side.path][side.start - 1 : side.end]
        if any(marker in line for line in lines for marker in TEST_MODULE_MARKERS):
            return True
    return False


def prefill_suggestions(
    clone: ClonePair, entries: list[BaselineEntry], lines_by_file: dict[str, list[str]]
) -> list[str]:
    """确定性预填建议：只给方向不定结论，语义判定仍归三问内核与代理。"""
    out = []
    if cross_platform(clone):
        out.append(
            "克隆对跨平台目录对（macos 与 windows）——按覆盖层默认建议 retain"
            "（平台适配面统一会泄漏平台概念）"
        )
    if test_side(clone, lines_by_file):
        out.append("克隆对任一侧落在测试模块——消除方向建议收进包内共享测试助手")
    if clone.key in baseline_index(entries):
        out.append("该文件对在基线已有同簇登记——第三次出现时按三次规则倾向 eliminate")
    if not out:
        out.append("无确定性命中，按三问内核判定（信息不足一律 retain）")
    return out


def fragment(lines: list[str], side: CloneSide) -> str:
    """侧片段：闭区间行文，超上限截断并标注省略行数。"""
    chunk = lines[side.start - 1 : side.end]
    if len(chunk) > FRAGMENT_CAP:
        omitted = len(chunk) - FRAGMENT_CAP
        return "\n".join(chunk[:FRAGMENT_CAP]) + "\n... (%d more lines)" % omitted
    return "\n".join(chunk)


def involved_packages(clone: ClonePair) -> list[str]:
    """涉事工作区分包：路径前两段推名，根包源树归根包名，供验证清单点名。"""
    names = []
    for side in (clone.first, clone.second):
        parts = side.path.split("/")
        name = parts[1] if parts[0] == "crates" else "gloss"
        if name not in names:
            names.append(name)
    return names


def triage_package(
    clone: ClonePair,
    prompt: str,
    lines_by_file: dict[str, list[str]],
    entries: list[BaselineEntry],
) -> str:
    """任务包拼装：四件套——提示词全文、克隆对上下文加预填建议、验证清单、
    输出契约（契约随内核全文携带，清单末尾再点名一遍）。"""
    packages = "、".join("`cargo test -p %s`" % p for p in involved_packages(clone))
    suggestions = "\n".join(
        "  - %s" % s for s in prefill_suggestions(clone, entries, lines_by_file)
    )
    head = "=== 克隆判定任务包 %s ===\n" % clone.ident
    context = (
        "\n## 2. 克隆对上下文\n\n"
        "- 克隆标识：`%s`（`just dup-triage %s` 可单独重出本包）\n"
        "- first：`%s:%d-%d`（%d 行）\n\n"
        "````\n%s\n````\n\n"
        "- second：`%s:%d-%d`（%d 行）\n\n"
        "````\n%s\n````\n\n"
        "- 预填建议（确定性启发，非结论）：\n%s\n"
        % (
            clone.ident,
            clone.ident,
            clone.first.path,
            clone.first.start,
            clone.first.end,
            clone.lines,
            fragment(lines_by_file[clone.first.path], clone.first),
            clone.second.path,
            clone.second.start,
            clone.second.end,
            clone.lines,
            fragment(lines_by_file[clone.second.path], clone.second),
            suggestions,
        )
    )
    checklist = (
        "\n## 3. 验证清单（按处置类型执行）\n\n"
        "- eliminate ⇒ `just precommit` 且 %s 且 `just duplicates`（重构须同步注释记录文件）\n"
        "- retain / exempt ⇒ `just fmt-toml` 通过，条目或就地注记带缘由\n" % packages
    )
    contract = (
        "\n## 4. 输出契约（三选一，全部带缘由）\n\n"
        "- eliminate：<建议抽壳位置，一句话>\n"
        "- retain：<缘由一句话，供基线登记>\n"
        "- exempt：<缘由一句话，供就地放行标记>\n"
    )
    return (
        head
        + "\n## 1. 判定程序：三问通用内核与仓库覆盖层\n\n"
        + prompt
        + context
        + checklist
        + contract
    )


def select_clones(clones: list[ClonePair], needle: str) -> list[ClonePair]:
    """任务包筛选：克隆标识全等或路径片段命中任一侧，空筛选取全部。"""
    if not needle:
        return clones
    out = []
    for clone in clones:
        if needle == clone.ident or needle in clone.first.path or needle in clone.second.path:
            out.append(clone)
    return sorted(out, key=lambda c: (c.key, c.first.start, c.second.start))


def run_engine(root: str, files: list[str], min_tokens: int) -> dict[str, Any]:
    """引擎调用：显式文件清单加绝对路径报告，报告落临时目录后读回；引擎缺席
    与异常退出都按无法判定处置，安装指引带清单钉版。"""
    pin = tool_version.pinned_version(TOOL_SECTION) or "latest"
    with tempfile.TemporaryDirectory(prefix="gloss-dup-") as out_dir:
        cmd = [
            ENGINE_BIN,
            "--absolute",
            "--min-tokens",
            str(min_tokens),
            "--reporters",
            "json",
            "--output",
            out_dir,
            "--silent",
        ] + files
        try:
            proc = subprocess.run(cmd, cwd=root, capture_output=True)
        except FileNotFoundError:
            raise GateInfra(
                "jscpd not found: the clone gate depends on it —"
                " install: npm install -g jscpd@%s (pinned in the repo tool list)" % pin
            ) from None
        if proc.returncode != 0:
            detail = proc.stderr.decode("utf-8", "replace").strip()
            raise GateInfra("jscpd failed (exit %d): %s" % (proc.returncode, detail))
        report_path = os.path.join(out_dir, REPORT_NAME)
        try:
            with open(report_path, encoding="utf-8") as fh:
                return json.load(fh)
        except (OSError, json.JSONDecodeError) as exc:
            raise GateInfra("jscpd report unreadable: %s" % exc) from None


def scan(
    root: str,
    min_tokens: int,
    engine: Callable[[str, list[str], int], dict[str, Any]],
    loader: Any = None,
) -> tuple[list[ClonePair], dict[str, Any]]:
    """单次扫描：跟踪清单过滤、显式传参、报告归一；受检面为空按失败关闭。
    并轨运行时跟踪清单经内容装入器共享枚举，单跑自行枚举。返回克隆实例清
    单与引擎原始报告（观察聚合读统计面）。"""
    if loader is not None:
        tracked = loader.tracked()
    else:
        tracked = gate_report.tracked_files(root)
    rel_files = surface_files(tracked)
    if not rel_files:
        raise GateInfra("scan surface is empty: no tracked sources under the guarded trees")
    report = engine(root, [os.path.join(root, rel) for rel in rel_files], min_tokens)
    return normalize_report(report, root), report


def report_text(clones: list[ClonePair], report: dict[str, Any]) -> str:
    """观察报告：全量克隆清单加聚合计数，按文件对与行数排序，确定性输出。"""
    total = report.get("statistics", {}).get("total", {})
    lines = [
        "== duplication observation report ==",
        "totals: %d clones, %s duplicated lines over %s lines in %s sources (%s%%)"
        % (
            total.get("clones", len(clones)),
            total.get("duplicatedLines", "?"),
            total.get("lines", "?"),
            total.get("sources", "?"),
            round(total.get("percentage", 0.0), 2),
        ),
        "",
        "file pairs by duplicated lines:",
    ]
    totals = pair_totals(clones)
    for key in sorted(totals, key=lambda k: (-totals[k], k)):
        count = sum(1 for c in clones if c.key == key)
        lines.append("  %4d lines / %2d clones  %s <-> %s" % (totals[key], count, key[0], key[1]))
    lines.append("")
    lines.append("clones (first..second start lines):")
    for clone in sorted(clones, key=lambda c: (c.key, c.first.start, c.second.start)):
        lines.append(
            "  %4d lines  %s:%d <-> %s:%d"
            % (
                clone.lines,
                clone.first.path,
                clone.first.start,
                clone.second.path,
                clone.second.start,
            )
        )
    return "\n".join(lines)


def parse_args(argv: list[str]) -> tuple[str, str, str, bool]:
    """参数解析：返回（模式，筛选值，仓库根，清理旗标）。模式互斥：判罚缺省，
    观察、任务包与体检各占一面；体检模式下的清理旗标只在体检面生效；其余旗标
    与多余位置参数按用法错误处置。"""
    mode = "punish"
    needle = ""
    fix = False
    positional: list[str] = []
    for arg in argv:
        if arg == "--report":
            _require_punish(mode)
            mode = "report"
        elif arg == "--triage":
            _require_punish(mode)
            mode = "triage"
        elif arg.startswith("--triage="):
            _require_punish(mode)
            mode = "triage"
            needle = arg.split("=", 1)[1]
        elif arg == "doctor":
            _require_punish(mode)
            mode = "doctor"
        elif arg == "--fix":
            fix = True
        elif arg in ("-h", "--help"):
            print(USAGE)
            raise SystemExit(0)
        elif arg.startswith("-") and arg != "-":
            raise ValueError("unknown flag: %s" % arg)
        else:
            positional.append(arg)
    if len(positional) > 1:
        raise ValueError("too many positional arguments")
    root = positional[0] if positional else REPO_ROOT
    return mode, needle, root, fix


def _require_punish(mode: str) -> None:
    """模式切换守卫：离开判罚缺省面后的再切换按用法错误处置。"""
    if mode != "punish":
        raise ValueError("modes are mutually exclusive (already %s)" % mode)


def usage_failure(detail: str) -> int:
    print(USAGE, file=sys.stderr)
    print("  %s" % detail, file=sys.stderr)
    return gate_report.EXIT_USAGE


def run_punish(root: str, engine: Any, loader: Any = None) -> int:
    """判罚模式：门禁线宽扫描，按判定口径出条目，失败末尾带判定辅助指针。"""
    gate_report.banner("clone gate", "repo root: %s (min tokens: %d)" % (root, GATE_MIN_TOKENS))
    clones, _raw = scan(root, GATE_MIN_TOKENS, engine, loader)
    entries = load_baseline(BASELINE_PATH)
    sides = {side.path for clone in clones for side in (clone.first, clone.second)}
    lines_by_file = load_lines(root, sides)
    findings = judge_clones(clones, entries, lines_by_file)
    for finding in findings:
        gate_report.fail(finding)
    if findings:
        gate_report.finish(findings, NODE_A_POINTER)
        return gate_report.EXIT_VIOLATION
    print("ok: clone gate passed (%d clones scanned, surface clean)" % len(clones))
    return gate_report.EXIT_PASS


def run_doctor(root: str, fix: bool, engine: Any, loader: Any = None) -> int:
    """基线体检：门禁线宽下合计归零的条目按陈旧报告；带清理旗标时剔除重写。"""
    gate_report.banner(
        "clone baseline doctor", "repo root: %s (min tokens: %d)" % (root, GATE_MIN_TOKENS)
    )
    clones, _raw = scan(root, GATE_MIN_TOKENS, engine, loader)
    entries = load_baseline(BASELINE_PATH)
    stale = stale_entries(entries, pair_totals(clones))
    findings = stale_findings(stale)
    for finding in findings:
        gate_report.fail(finding)
    if fix and findings:
        removed = prune_baseline(BASELINE_PATH, stale)
        print("pruned %d stale baseline %s" % (removed, "entry" if removed == 1 else "entries"))
        return gate_report.EXIT_PASS
    if findings:
        gate_report.finish(findings)
        return gate_report.EXIT_VIOLATION
    print("ok: baseline doctor passed (%d entries, none stale)" % len(entries))
    return gate_report.EXIT_PASS


def run_report(root: str, engine: Any, loader: Any = None) -> int:
    """观察模式：观察线宽全量输出，不判罚，恒零退出。"""
    clones, raw = scan(root, REPORT_MIN_TOKENS, engine, loader)
    print(report_text(clones, raw))
    print("ok: observation only, not a gate (%d clones)" % len(clones))
    return gate_report.EXIT_PASS


def run_triage(root: str, needle: str, engine: Any, loader: Any = None) -> int:
    """任务包模式：门禁线宽扫描后按克隆对逐个拼装，提示词工件缺席即失败关闭。"""
    try:
        with open(PROMPT_PATH, encoding="utf-8") as fh:
            prompt = fh.read()
    except OSError as exc:
        raise GateInfra("triage prompt missing: %s" % exc) from None
    clones, _raw = scan(root, GATE_MIN_TOKENS, engine, loader)
    picked = select_clones(clones, needle)
    if needle and not picked:
        print("no clone matches %r" % needle)
        return gate_report.EXIT_PASS
    entries = load_baseline(BASELINE_PATH)
    sides = {side.path for clone in picked for side in (clone.first, clone.second)}
    lines_by_file = load_lines(root, sides)
    for clone in picked:
        print(triage_package(clone, prompt, lines_by_file, entries))
    print("== %d task package(s) ==" % len(picked))
    return gate_report.EXIT_PASS


def main(engine: Any = None, loader: Any = None) -> int:
    runner = engine if engine is not None else run_engine
    try:
        mode, needle, root, fix = parse_args(sys.argv[1:])
    except SystemExit:
        return 0
    except ValueError as exc:
        return usage_failure(str(exc))
    if not os.path.isdir(root):
        return usage_failure("repo-root is not a directory: %s" % root)
    handlers = {
        "punish": lambda: run_punish(root, runner, loader),
        "doctor": lambda: run_doctor(root, fix, runner, loader),
        "report": lambda: run_report(root, runner, loader),
        "triage": lambda: run_triage(root, needle, runner, loader),
    }
    try:
        return handlers[mode]()
    except GateInfra as exc:
        gate_report.fail(gate_report.infra_failure("cannot complete the clone gate: %s" % exc))
        return gate_report.EXIT_INFRA


if __name__ == "__main__":
    sys.exit(main())
