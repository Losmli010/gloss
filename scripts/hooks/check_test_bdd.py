#!/usr/bin/env python3
"""测试行为清单一致性门禁：登记文档与测试源码双向核对。
  - 测试源码侧：版本库跟踪的本语言（Rust）源码里的测试函数，加上清单声明的
    自管测试目标（自带运行器的自检目标），每一项都必须登记进清单；
  - 清单侧：清单的每个条目必须能在测试源码里找到同名测试；
  - 唯一性三道检查：登记行数对账源码跨文件实例数、跨文件同名必须带文件绑
    定限定符（限定符取定义文件名主干，汇编模块文件取父目录名）、同条目两
    行不得绑到同一文件；描述带“最新沿革见”指针的沿革行豁免三道检查。
条目的描述文字不在校验面（描述与代码冲突时以代码为准，见指令文档的测试节）。
本地提交前门禁与持续集成检查共用此脚本，保证本地与远端判定一致。违规报告
经统一诊断出口打印：每条带规则标识、位置、条目清单与修复指引；清单文件缺
失与测试源码扫描面为空两处前置故障按无法完成判定失败关闭退出。退出码按全
脚本统一约定。

用法：python3 scripts/hooks/check_test_bdd.py <repo-root>，缺省为本脚本的上一级目录。
"""

from __future__ import annotations

import os
import re
import sys
from collections import Counter
from typing import Any

import gate_report

BDD_REL = os.path.join("docs", "tests", "bdd.md")
FN_RE = re.compile(r"fn\s+([A-Za-z0-9_]+)")
TEST_ATTR_RE = re.compile(r"^\s*#\[(?:tokio::)?test\]")
MANUAL_CHAPTER = "## 发版人工步骤"

RULE_BDD_STALE = "BDD-STALE"
RULE_BDD_MISSING = "BDD-MISSING"
RULE_BDD_DUP_ROW = "BDD-DUP-ROW"
RULE_BDD_DUP_CHAPTER = "BDD-DUP-CHAPTER"
RULE_BDD_DUP_BLOCK = "BDD-DUP-BLOCK"
RULE_BDD_TABLE = "BDD-TABLE"
RULE_BDD_DUP_QUALIFIER = "BDD-DUP-QUALIFIER"
RULE_BDD_INSTANCE = "BDD-INSTANCE"
RULE_BDD_BINDING = "BDD-BINDING"

HISTORY_MARKER = "最新沿革见"
NAME_RE = re.compile(r"[A-Za-z0-9_]+")
QUALIFIED_RE = re.compile(r"^(.*)（([^（）]*)）\s*$")

SYNC_REMEDY = (
    "added/renamed/removed tests: register them in bdd.md within the same PR and"
    " refresh the entry's update time; description conflicts with code: code wins"
)
DUP_REMEDY = (
    "keep one registration per test: delete the duplicated row, heading or block;"
    " old rows left after a rename go too"
)
QUALIFIER_REMEDY = (
    "append a full-width parenthesized qualifier holding the defining file's name"
    " (parent dir for mod files) to the entry name"
)
TABLE_REMEDY = "repair the table: keep header, separator and body rows contiguous"

INVENTORY_REMEDY = (
    "the test behavior inventory is a git-tracked file; do not delete or rename"
    " it; the directory is allowed via a gitignore exception"
)
EMPTY_SURFACE_REMEDY = (
    "run the gate from the workspace root; an empty scan surface means the"
    " bidirectional cross-check cannot complete"
)


def tracked_rs_files(root: str, loader: Any = None) -> list[str]:
    if loader is not None:
        return [p for p in loader.tracked() if p.endswith(".rs")]
    out = gate_report.run_git(["ls-files", "-z", "--", "*.rs"], cwd=root).stdout
    return [p.decode("utf-8", "replace") for p in out.split(b"\0") if p]


def src_test_instances(
    root: str, tracked: list[str] | None = None, loader: Any = None
) -> dict[str, set[str]]:
    """#[test] / #[tokio::test] 函数名到定义文件（仓库相对路径）集合。

    属性行触发 pending；跳过中间的其它属性行（如 #[ignore = \"...\"]）取最近的
    fn 名；属性与 fn 同行也支持。实例按文件计：同名测试落在几个文件就是几个
    实例，供限定行数对账与限定符文件绑定用。"""
    instances: dict[str, set[str]] = {}
    rels = tracked if tracked is not None else tracked_rs_files(root, loader)
    for rel in rels:
        pending = False
        path = os.path.join(root, rel)
        if loader is not None:
            lines = loader.read_text(path, errors="replace").split("\n")
        else:
            with open(path, encoding="utf-8", errors="replace") as fh:
                lines = fh.read().split("\n")
        for line in lines:
            line = line.rstrip("\n")
            if TEST_ATTR_RE.match(line):
                m = FN_RE.search(line)
                if m:
                    instances.setdefault(m.group(1), set()).add(rel)
                else:
                    pending = True
                continue
            if pending:
                if re.match(r"^\s*#", line):
                    continue
                m = FN_RE.search(line)
                if m:
                    instances.setdefault(m.group(1), set()).add(rel)
                    pending = False
    return instances


def manifest_files(root: str) -> list[str]:
    out = [os.path.join(root, "Cargo.toml")]
    crates = os.path.join(root, "crates")
    if os.path.isdir(crates):
        for name in sorted(os.listdir(crates)):
            m = os.path.join(crates, name, "Cargo.toml")
            if os.path.isfile(m):
                out.append(m)
    return out


def target_test_names(root: str, loader: Any = None) -> set[str]:
    """[[test]] 声明的测试目标名（harness = false 自检二进制没有 #[test] 函数）。
    声明本身即登记面，目标源文件缺席不影响双向核对（缺文件的构建不成立）。"""
    names = set()
    for manifest in manifest_files(root):
        if not os.path.isfile(manifest):
            continue
        if loader is not None:
            lines = loader.read_text(manifest, errors="replace").split("\n")
        else:
            with open(manifest, encoding="utf-8", errors="replace") as fh:
                lines = fh.read().split("\n")
        for i, line in enumerate(lines):
            if line.strip() != "[[test]]":
                continue
            for window in lines[i + 1 : i + 4]:
                m = re.match(r'\s*name\s*=\s*"([^"]+)"', window)
                if m:
                    names.add(m.group(1))
                    break
    return names


def target_test_instances(
    root: str, tracked: list[str] | None = None, loader: Any = None
) -> dict[str, set[str]]:
    """[[test]] 测试目标名到目标源文件集合：按「tests/ 目录下同名 .rs」在跟踪
    清单里解析，解析不到的声明不计实例（限定符文件绑定无从谈起）。"""
    rels = tracked if tracked is not None else tracked_rs_files(root, loader)
    by_tests_stem: dict[str, set[str]] = {}
    for rel in rels:
        parts = rel.replace(os.sep, "/").split("/")
        if "tests" in parts[:-1] and parts[-1].endswith(".rs"):
            stem = parts[-1][: -len(".rs")]
            by_tests_stem.setdefault(stem, set()).add(rel)
    instances: dict[str, set[str]] = {}
    for name in target_test_names(root, loader):
        files = by_tests_stem.get(name)
        if files:
            instances[name] = set(files)
    return instances


def bdd_lines(bdd_path: str, loader: Any = None) -> list[str]:
    """行为清单按行读取：装入器在场时走共享缓存，行集合与逐行读取一致。"""
    if loader is not None:
        return loader.read_text(bdd_path).split("\n")
    with open(bdd_path, encoding="utf-8") as fh:
        return fh.read().split("\n")


def bdd_entries(bdd_path: str, loader: Any = None) -> set[str]:
    """清单侧条目名：表格取首列，人工测试取 ### 小节标题；剥掉快照重名的
    （popup）/（settings）限定后缀；表头、分隔行与 crates/... 文件标题经标识符
    过滤自然排除。「## 发版人工步骤」节登记无自动化测试源码的发版链路运维步骤，
    整节豁免双向核对。"""
    names = []
    exempt = False
    for line in bdd_lines(bdd_path, loader):
        line = line.rstrip("\n")
        if line.startswith("## "):
            exempt = line == MANUAL_CHAPTER
            continue
        if exempt:
            continue
        if line.startswith("### "):
            names.append(re.sub(r"^#+\s*", "", line))
        elif line.startswith("|"):
            cell = re.sub(r"^\|\s*", "", line)
            cell = re.sub(r"\s*\|.*$", "", cell)
            names.append(cell)
    cleaned = set()
    for name in names:
        name = re.sub(r"（[^）]*）\s*$", "", name)
        if re.fullmatch(r"[A-Za-z0-9_]+", name):
            cleaned.add(name)
    return cleaned


def bdd_registrations(bdd_path: str, loader: Any = None) -> list[dict[str, Any]]:
    """清单登记行的结构化提取：每条带裸名、限定符、小节、行号与沿革标记。

    裸名按「尾部全角括号」剥限定符；沿革行按描述里的「最新沿革见」指针机器
    识别，豁免限定名唯一、实例数对账与文件绑定三道检查（同名不同描述的
    历史重录是有意保留）。人工测试豁免节不计。"""
    out: list[dict[str, Any]] = []
    exempt = False
    section = ""
    for line in bdd_lines(bdd_path, loader):
        line = line.rstrip("\n")
        if line.startswith("## "):
            exempt = line == MANUAL_CHAPTER
            section = "" if exempt else line
            continue
        if exempt:
            continue
        entry: dict[str, Any] | None = None
        if line.startswith("### "):
            entry = {"kind": "heading", "cell": re.sub(r"^#+\s*", "", line), "line": line}
        elif line.startswith("|") and not re.match(r"^\s*\|\s*(测试名称|---)", line):
            cell = re.sub(r"^\|\s*", "", line)
            cell = re.sub(r"\s*\|.*$", "", cell)
            entry = {"kind": "row", "cell": cell, "line": line}
        if entry is None:
            continue
        bare, qualifier = entry["cell"], ""
        m = QUALIFIED_RE.match(entry["cell"])
        if m:
            bare, qualifier = m.group(1), m.group(2)
        out.append(
            {
                "kind": entry["kind"],
                "section": section,
                "bare": bare,
                "qualifier": qualifier,
                "history": HISTORY_MARKER in line,
                "cell": entry["cell"],
                "raw": line,
            }
        )
    return out


def qualifier_binds(qualifier: str, rel: str) -> bool:
    """限定符是否绑定源文件：等于文件名主干，mod 文件取父目录名。"""
    stem = os.path.splitext(os.path.basename(rel))[0]
    if stem == "mod":
        return os.path.basename(os.path.dirname(rel)) == qualifier
    return stem == qualifier


def qualifier_findings(
    registrations: list[dict[str, Any]], instances: dict[str, set[str]]
) -> list[gate_report.Finding]:
    """限定名唯一、实例数对账与限定符文件绑定三道检查。

    按裸名分组（仅标识符裸名，沿革行豁免）：登记行数不等于源码跨文件实例
    数判实例数对账；跨文件同名的限定行缺限定符、限定符非纯文件名、绑定不
    到任何定义文件、定义文件无限定符覆盖判文件绑定；两行绑到同一文件判限
    定名唯一（同文件被登记两遍）。源码无实例的裸名由双向核对判陈旧，此处
    跳过避免双报。"""
    grouped: dict[str, list[dict[str, Any]]] = {}
    for reg in registrations:
        if reg["history"] or not NAME_RE.fullmatch(reg["bare"]):
            continue
        grouped.setdefault(reg["bare"], []).append(reg)

    instance_rows: list[str] = []
    binding_rows: list[str] = []
    dup_rows: list[str] = []
    for name in sorted(grouped):
        regs = grouped[name]
        files = instances.get(name)
        if not files:
            continue
        if len(regs) != len(files):
            instance_rows.append(
                "%s (%d inventory rows vs %d source instances)" % (name, len(regs), len(files))
            )
        owner: dict[str, str] = {}
        covered: set[str] = set()
        for reg in regs:
            qualifier = reg["qualifier"]
            if not qualifier:
                if len(files) > 1:
                    binding_rows.append(
                        "%s: cross-file same-name test needs a file-bound qualifier (%s)"
                        % (name, reg["cell"])
                    )
                continue
            if not NAME_RE.fullmatch(qualifier):
                binding_rows.append(
                    "%s: qualifier %r is not a plain file-name token" % (name, qualifier)
                )
                continue
            matches = sorted(rel for rel in files if qualifier_binds(qualifier, rel))
            if not matches:
                binding_rows.append(
                    "%s: qualifier %r matches none of: %s"
                    % (name, qualifier, ", ".join(sorted(files)))
                )
                continue
            covered.update(matches)
            for rel in matches:
                if rel in owner:
                    dup_rows.append(
                        "%s: %s is bound by two rows (%s / %s)"
                        % (name, os.path.basename(rel), owner[rel], reg["cell"])
                    )
                owner[rel] = reg["cell"]
        if len(files) > 1:
            for rel in sorted(files - covered):
                binding_rows.append("%s: no qualifier bound to %s" % (name, rel))
    findings = []
    if instance_rows:
        findings.append(
            entry_finding(
                RULE_BDD_INSTANCE,
                "inventory row counts do not match cross-file source instance counts"
                " (history rows excluded)",
                "qualify each instance with its defining file's name, or drop stale rows",
                instance_rows,
            )
        )
    if binding_rows:
        findings.append(
            entry_finding(
                RULE_BDD_BINDING,
                "qualifiers must bind to defining files (file stem, parent dir for mod files)",
                QUALIFIER_REMEDY,
                binding_rows,
            )
        )
    if dup_rows:
        findings.append(
            entry_finding(
                RULE_BDD_DUP_QUALIFIER,
                "two rows of the same entry bind the same defining file",
                "distinct instances need distinct file-bound qualifiers; old rows go",
                dup_rows,
            )
        )
    return findings


def duplicate_rows(bdd_path: str, loader: Any = None) -> list[str]:
    """整行精确重复（同名同目标同场景同时间）＝改名后旧行未删的真实特征。
    同名不同描述的沿革行（历史重录记录）是有意保留，不拦。"""
    rows = []
    exempt = False
    for line in bdd_lines(bdd_path, loader):
        line = line.rstrip("\n")
        if line.startswith("## "):
            exempt = line == MANUAL_CHAPTER
            continue
        if not line.startswith("|") or exempt:
            continue
        if re.match(r"^\s*\|\s*测试名称", line):
            continue
        if re.match(r"^\s*\|\s*---", line):
            continue
        rows.append(line)
    return sorted(k for k, c in Counter(rows).items() if c > 1)


def duplicate_chapters(bdd_path: str, loader: Any = None) -> list[str]:
    chapters = []
    for line in bdd_lines(bdd_path, loader):
        if line.startswith("## "):
            chapters.append(line.rstrip("\n"))
    return sorted(k for k, c in Counter(chapters).items() if c > 1)


def duplicate_blocks(bdd_path: str, loader: Any = None) -> list[str]:
    """同一 ## 章节内重复的 ### 小节标题，或同一小节内逐字重复的「- 」条目行
    ——人工测试条目整段重复拼接的痕迹（表格行的重复由整行精确重复检查覆盖）。"""
    dups = set()
    chapter = ""
    section = ""
    exempt = False
    h3 = set()
    bullets = set()
    for raw in bdd_lines(bdd_path, loader):
        line = raw.rstrip("\n")
        if line.startswith("## "):
            chapter = line
            exempt = line == MANUAL_CHAPTER
            section = "" if exempt else line
            h3 = set()
            bullets = set()
            continue
        if line.startswith("### "):
            if exempt or chapter == "":
                continue
            if line in h3:
                dups.add(line)
            h3.add(line)
            section = line
            bullets = set()
            continue
        if line.startswith("- "):
            if exempt or section == "":
                continue
            if line in bullets:
                dups.add(line)
            bullets.add(line)
    return sorted(dups)


def broken_tables(bdd_path: str, loader: Any = None) -> list[str]:
    """结构校验：非豁免小节里的每个表格必须是良构 GFM 表——表行连成一个块，
    且块的第二行是分隔行。空行或正文混在表头与表体之间会把表体从表头上
    切断（渲染退化为纯文本），一律按缺表头列出。"""
    broken = {}
    section = ""
    exempt = False
    state = {"in_run": False, "run_len": 0, "run_ok": False}

    def end_run() -> None:
        if state["in_run"] and not state["run_ok"] and not exempt and section != "":
            broken[section] = True
        state["in_run"] = False

    for raw in bdd_lines(bdd_path, loader):
        line = raw.rstrip("\n")
        if line.startswith("## "):
            end_run()
            exempt = line == MANUAL_CHAPTER
            section = "" if exempt else line
            continue
        if line.startswith("### "):
            if exempt:
                continue
            end_run()
            section = line
            continue
        if line.startswith("|"):
            if exempt or section == "":
                continue
            if not state["in_run"]:
                state["in_run"] = True
                state["run_len"] = 0
                state["run_ok"] = False
            state["run_len"] += 1
            if state["run_len"] == 2 and re.match(r"^\s*\|\s*---", line):
                state["run_ok"] = True
            continue
        end_run()
    end_run()
    return sorted(broken)


def require_inventory(bdd_path: str) -> None:
    """清单文件在场守卫：缺失即输入不可读，无法完成判定，失败关闭退出。"""
    if os.path.isfile(bdd_path):
        return
    gate_report.fail(
        gate_report.Finding(
            rule=gate_report.RULE_INFRA,
            path=BDD_REL,
            line=0,
            message="the test behavior inventory is missing",
            remedy=INVENTORY_REMEDY,
        )
    )
    sys.exit(gate_report.EXIT_INFRA)


def require_sources(src_all: set[str]) -> None:
    """测试源码扫描面守卫：两侧都提取不到即扫描面意外为空，失败关闭退出。"""
    if src_all:
        return
    gate_report.fail(
        gate_report.Finding(
            rule=gate_report.RULE_EMPTY_SURFACE,
            path="",
            line=0,
            message="no tests extracted from test sources"
            " (git-tracked .rs and [[test]] targets are both empty)",
            remedy=EMPTY_SURFACE_REMEDY,
        )
    )
    sys.exit(gate_report.EXIT_INFRA)


def entry_finding(rule: str, message: str, remedy: str, entries: list[str]) -> gate_report.Finding:
    """单类违规的条目：清单文件为位置，条目清单并成一行证据。"""
    return gate_report.Finding(
        rule=rule,
        path=BDD_REL,
        line=0,
        message=message,
        evidence=", ".join(entries),
        remedy=remedy,
    )


def collect(root: str, loader: Any = None) -> tuple[list[str], list[gate_report.Finding]]:
    """双向核对，违规随执行流即时经统一诊断出口打到标准错误，返回通过与违规
    条目两个清单；装入器在场时清单与文件读取走共享缓存，缺省自行读取。"""
    bdd_path = os.path.join(root, BDD_REL)
    require_inventory(bdd_path)

    src_instances = src_test_instances(root, loader=loader)
    target_instances = target_test_instances(root, loader=loader)
    instances: dict[str, set[str]] = {}
    for source in (src_instances, target_instances):
        for name, rels in source.items():
            instances.setdefault(name, set()).update(rels)
    # 双向核对按名字集合：测试目标声明即登记面；限定符检查按文件实例
    src_all = set(instances) | target_test_names(root, loader=loader)
    require_sources(src_all)
    bdd_names = bdd_entries(bdd_path, loader)
    registrations = bdd_registrations(bdd_path, loader)

    found: list[gate_report.Finding] = []

    def emit(finding: gate_report.Finding) -> None:
        found.append(finding)
        gate_report.fail(finding)

    stale = sorted(bdd_names - src_all)
    if stale:
        emit(
            entry_finding(
                RULE_BDD_STALE,
                "these bdd.md entries no longer exist in test sources"
                " (renamed or removed without syncing)",
                SYNC_REMEDY,
                stale,
            )
        )
    missing = sorted(src_all - bdd_names)
    if missing:
        emit(
            entry_finding(
                RULE_BDD_MISSING,
                "these tests are not registered in bdd.md",
                SYNC_REMEDY,
                missing,
            )
        )
    dups = duplicate_rows(bdd_path, loader)
    if dups:
        emit(
            entry_finding(
                RULE_BDD_DUP_ROW,
                "these bdd.md entries are registered multiple times (entry count exceeds"
                " same-name tests in sources; old rows left after a rename?)",
                DUP_REMEDY,
                dups,
            )
        )
    dups = duplicate_chapters(bdd_path, loader)
    if dups:
        emit(
            entry_finding(
                RULE_BDD_DUP_CHAPTER,
                "these ## chapter headings appear more than once (sign of duplicated blocks)",
                DUP_REMEDY,
                dups,
            )
        )
    dups = duplicate_blocks(bdd_path, loader)
    if dups:
        emit(
            entry_finding(
                RULE_BDD_DUP_BLOCK,
                "these section headings or bullet lines repeat within the same"
                " chapter/section (sign of duplicated blocks)",
                DUP_REMEDY,
                dups,
            )
        )
    broken = broken_tables(bdd_path, loader)
    if broken:
        emit(
            entry_finding(
                RULE_BDD_TABLE,
                "these sections have tables missing a header row (or the body is cut from"
                " the header by a blank line/prose)",
                TABLE_REMEDY,
                broken,
            )
        )
    for finding in qualifier_findings(registrations, instances):
        emit(finding)

    oks = []
    if not found:
        oks.append(
            "bdd.md matches test sources (%d in sources / %d in inventory)"
            % (len(src_all), len(bdd_names))
        )
    return oks, found


def render(finding: gate_report.Finding) -> str:
    """违规渲染行：消息加位置，条目证据附在尾部，作为运行函数返回清单里的条目。"""
    loc = gate_report.location(finding)
    base = "%s (%s)" % (finding.message, loc) if loc else finding.message
    if finding.evidence:
        base = "%s — %s" % (base, finding.evidence)
    return base


def run(root: str, loader: Any = None) -> tuple[list[str], list[str]]:
    """双向核对，返回 (oks, fails)；失败清单收渲染行（消息、位置与条目证据）
    供调用方取用，装入器在场时清单与文件读取走共享缓存，缺省自行读取。"""
    oks, found = collect(root, loader)
    return oks, [render(finding) for finding in found]


def usage_failure(detail: str) -> int:
    print("usage: check_test_bdd.py [repo-root]", file=sys.stderr)
    print("  %s" % detail, file=sys.stderr)
    return gate_report.EXIT_USAGE


def main(loader: Any = None) -> int:
    if len(sys.argv) > 2:
        return usage_failure("too many arguments: expected at most one repo-root")
    root = (
        os.path.abspath(sys.argv[1])
        if len(sys.argv) > 1
        else os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    )
    if len(sys.argv) > 1 and not os.path.isdir(root):
        return usage_failure("repo-root is not a directory: %s" % root)
    oks, found = collect(root, loader)
    # 违规已在扫描时即时输出，这里只补汇总与结论
    for line in oks:
        print("  ok: %s" % line)
    if found:
        print("", file=sys.stderr)
        gate_report.summary(found)
        return gate_report.EXIT_VIOLATION
    return gate_report.EXIT_PASS


if __name__ == "__main__":
    sys.exit(main())
