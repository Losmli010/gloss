#!/usr/bin/env python3
"""测试行为清单一致性门禁：登记文档（docs/tests/）与测试源码双向核对，登记文档
按包落位——根总纲加包清单。
  - 测试源码侧：版本库跟踪的本语言（Rust）源码里的测试函数，加上清单声明的
    自管测试目标（自带运行器的自检目标），每一项都必须登记进任一登记文件；
  - 清单侧：各登记文件的每个条目必须能在测试源码里找到同名测试；
  - 唯一性三道检查：登记行数对账源码跨文件实例数、跨文件同名必须带文件绑
    定限定符（限定符取定义文件名主干，汇编模块文件取父目录名）、同条目两
    行不得绑到同一文件；描述带“最新沿革见”指针的沿革行豁免三道检查；
  - 结构三道检查：逐源文件小节标题里的路径必须是真实存在的跟踪源文件；包
    清单的单元测试小节集合与该包单元章有登记的测试源文件按路径双射（包归
    属由登记文件名机械推导）；根总纲的总览节必须与登记文件再生结果一致，
    过期即违规，再生成走总览再生成配方（just test-index）。
条目的描述文字不在校验面（描述与代码冲突时以代码为准，见指令文档的测试节）。
本地提交前门禁与持续集成检查共用此脚本，保证本地与远端判定一致。违规报告
经统一诊断出口打印：每条带规则标识、位置（取实际登记文件）、条目清单与修复
指引；根总纲缺失与测试源码扫描面为空两处前置故障按无法完成判定失败关闭退
出。退出码按全脚本统一约定。

用法：python3 scripts/hooks/check_test_bdd.py <repo-root>，缺省为本脚本的上一级目录。
"""

from __future__ import annotations

import os
import re
import sys
from collections import Counter
from typing import Any

import gate_report
import test_index

BDD_REL = "docs/tests/bdd.md"
UNIT_CHAPTER = test_index.UNIT_CHAPTER
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
RULE_BDD_PATH = "BDD-PATH"
RULE_BDD_MAP = "BDD-MAP"
RULE_BDD_INDEX = "BDD-INDEX"

HISTORY_MARKER = "最新沿革见"
NAME_RE = re.compile(r"[A-Za-z0-9_]+")
QUALIFIED_RE = re.compile(r"^(.*)（([^（）]*)）\s*$")

SYNC_REMEDY = (
    "added/renamed/removed tests: register them in docs/tests/ within the same PR"
    " and refresh the entry's update time; description conflicts with code: code wins"
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
PATH_REMEDY = (
    "the heading path must be a git-tracked source file: retitle it to the file's"
    " current repo-relative path, or drop the section if the file is gone"
)
MAP_REMEDY = (
    "unit-test sections must biject with the package's unit-registered source files"
    " (derived from the crates/<pkg>/src prefix): add a section per unlisted file"
    " carrying unit registrations, drop sections for files without them"
)
INDEX_REMEDY = "the overview is generated: run just test-index to refresh it"

INVENTORY_REMEDY = (
    "the test behavior inventory is a git-tracked file; do not delete or rename"
    " it; the directory is allowed via a gitignore exception"
)
EMPTY_SURFACE_REMEDY = (
    "run the gate from the workspace root; an empty scan surface means the"
    " bidirectional cross-check cannot complete"
)


def tracked_files(root: str, loader: Any = None) -> list[str]:
    """版本库跟踪清单（全文件）：登记文件枚举与路径真实性检查共用一次枚举。"""
    if loader is not None:
        return loader.tracked()
    return gate_report.tracked_files(root)


def tracked_rs_files(root: str, loader: Any = None) -> list[str]:
    """版本库跟踪清单里的本语言（Rust）源码：测试实例扫描面。"""
    return [p for p in tracked_files(root, loader) if p.endswith(".rs")]


def src_test_instances(
    root: str, tracked: list[str] | None = None, loader: Any = None
) -> dict[str, set[str]]:
    """测试属性（#[test]）与异步测试属性（#[tokio::test]）函数名到定义文件
    （仓库相对路径）集合。

    属性行触发待定态（pending）；跳过中间的其它属性行（如忽略属性）取最近的
    函数（fn）名；属性与函数同行也支持。实例按文件计：同名测试落在几个文件
    就是几个实例，供限定行数对账与限定符文件绑定用。"""
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
    """测试目标声明（[[test]]）的目标名，关自检的二进制（harness = false）没有
    测试属性（#[test]）函数。声明本身即登记面，目标源文件缺席不影响双向核对
    （缺文件的构建不成立）。"""
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
    root: str,
    tracked: list[str] | None = None,
    loader: Any = None,
    declared: set[str] | None = None,
) -> dict[str, set[str]]:
    """测试目标声明（[[test]]）的目标名到目标源文件集合：按跟踪清单里测试目录
    下的同名源文件解析，目录形态（tests/<名>/main.rs）按目录名归位，解析不到
    的声明不计实例（限定符文件绑定无从谈起）。声明清单可由调用方传入复用。"""
    rels = tracked if tracked is not None else tracked_rs_files(root, loader)
    by_tests_stem: dict[str, set[str]] = {}
    for rel in rels:
        parts = rel.replace(os.sep, "/").split("/")
        if "tests" in parts[:-1] and parts[-1].endswith(".rs"):
            stem = parts[-1][: -len(".rs")]
            by_tests_stem.setdefault(stem, set()).add(rel)
            if stem == "main" and parts[-2] != "tests":
                by_tests_stem.setdefault(parts[-2], set()).add(rel)
    instances: dict[str, set[str]] = {}
    for name in declared if declared is not None else target_test_names(root, loader):
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
    """清单侧条目名：表格取首列，人工测试取三级小节标题；剥掉快照重名的
    弹层（popup）与设置（settings）限定后缀；表头、分隔行与以分包路径起头的
    文件标题经标识符过滤自然排除。“发版人工步骤”节登记无自动化测试源码的
    发版链路运维步骤，整节豁免双向核对。"""
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

    裸名按尾部全角括号剥限定符；沿革行按描述里的“最新沿革见”指针机器
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
        history_zone = line
        if line.startswith("### "):
            entry = {"kind": "heading", "cell": re.sub(r"^#+\s*", "", line), "line": line}
        elif line.startswith("|") and not re.match(r"^\s*\|\s*(测试名称|---)", line):
            cell = re.sub(r"^\|\s*", "", line)
            cell = re.sub(r"\s*\|.*$", "", cell)
            entry = {"kind": "row", "cell": cell, "line": line}
            # 沿革指针锚定到描述列（第三列）：标记出现在名称或限定符里不构成豁免
            cells = [c.strip() for c in line.strip().strip("|").split("|")]
            if len(cells) >= 3:
                history_zone = cells[2]
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
                "history": HISTORY_MARKER in history_zone,
                "cell": entry["cell"],
                "raw": line,
            }
        )
    return out


def qualifier_binds(qualifier: str, rel: str) -> bool:
    """限定符是否绑定源文件：等于文件名主干，模块文件（mod）取父目录名。"""
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


def heading_paths(bdd_path: str, loader: Any = None) -> set[str]:
    """三级小节标题里的路径形态文本：剥限定符尾巴与反引号后含分隔符的标题集合，
    各章一律收取（人工两类的小节标题是测试名，不含分隔符，天然不进集合）。"""
    paths = set()
    for line in bdd_lines(bdd_path, loader):
        if not line.startswith("### "):
            continue
        rel = test_index.heading_path(line)
        if rel:
            paths.add(rel)
    return paths


def unit_section_paths(bdd_path: str, loader: Any = None) -> set[str]:
    """单元测试章内逐源文件小节的路径集合：章内路径形态的三级标题。"""
    paths = set()
    chapter = ""
    for line in bdd_lines(bdd_path, loader):
        line = line.rstrip("\n")
        if line.startswith("## "):
            chapter = line
            continue
        if chapter != UNIT_CHAPTER or not line.startswith("### "):
            continue
        rel = test_index.heading_path(line)
        if rel:
            paths.add(rel)
    return paths


def unit_registered_files(
    registrations: list[dict[str, Any]], instances: dict[str, set[str]]
) -> set[str]:
    """单元测试章有登记的测试实例涉及的源文件集合：限定行按限定符绑定到定义文件，
    无限定行按裸名取定义文件并集（跨文件同名无限定符时两文件都算，文件绑定检查
    另行判罚）。纯快照章登记的源文件（如渲染测试集中住的源文件）不在其中，不要
    求单元小节。"""
    files: set[str] = set()
    for reg in registrations:
        if reg["section"] != UNIT_CHAPTER:
            continue
        inst = instances.get(reg["bare"])
        if not inst:
            continue
        qualifier = reg["qualifier"]
        if qualifier and NAME_RE.fullmatch(qualifier):
            files |= {rel for rel in inst if qualifier_binds(qualifier, rel)}
        else:
            files |= inst
    return files


def unit_map_findings(
    inventory: list[str],
    per_file: dict[str, dict[str, Any]],
    registered: set[str],
) -> list[gate_report.Finding]:
    """包清单的单元测试小节集合与该包单元章有登记的测试源文件按路径双射。

    包归属由登记文件名机械推导——文件名整段即包名的推到对应包的源码目录，
    入口包推到入口源码目录——根总纲与推导不出的文件跳过；缺小节与多小节两
    向都判，跨包误挂的小节由所属包缺小节一侧拦下。"""
    findings = []
    for rel in inventory:
        prefix = test_index.package_prefix(rel)
        if prefix is None:
            continue
        sections = {p for p in per_file[rel]["unit_sections"] if p.startswith(prefix)}
        files = {p for p in registered if p.startswith(prefix)}
        missing = sorted(files - sections)
        extra = sorted(sections - files)
        if not missing and not extra:
            continue
        evidence = []
        if missing:
            evidence.append("missing section: " + ", ".join(missing))
        if extra:
            evidence.append("section without unit registrations: " + ", ".join(extra))
        findings.append(
            gate_report.Finding(
                rule=RULE_BDD_MAP,
                path=rel,
                line=0,
                message="unit-test sections do not biject with unit-registered source"
                " files of this package",
                evidence="; ".join(evidence),
                remedy=MAP_REMEDY,
            )
        )
    return findings


def overview_findings(
    root: str, inventory: list[str], loader: Any = None
) -> list[gate_report.Finding]:
    """总览新鲜度：根总纲的总览节与登记文件再生成结果逐行比对，过期即违规。"""
    texts = {}
    for rel in inventory:
        texts[rel] = "\n".join(bdd_lines(os.path.join(root, rel), loader))
    expected = "\n".join(test_index.build_section(root, inventory, texts)).strip("\n")
    root_text = texts[BDD_REL]
    start, end = test_index.overview_span(root_text)
    actual = "\n".join(root_text.split("\n")[start:end]).strip("\n")
    if actual == expected:
        return []
    return [
        gate_report.Finding(
            rule=RULE_BDD_INDEX,
            path=BDD_REL,
            line=0,
            message="the generated overview does not match the inventory files",
            remedy=INDEX_REMEDY,
        )
    ]


def duplicate_rows(bdd_path: str, loader: Any = None) -> list[str]:
    """整行精确重复（同名同目标同场景同时间），即改名后旧行未删的真实特征。
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
    """同一二级章节内重复的三级小节标题，或同一小节内逐字重复的短横条目行
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
    """结构校验：非豁免小节里的每个表格必须是良构表格（GFM）——表行连成一个块，
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


def require_inventory(root: str, inventory: list[str]) -> None:
    """根总纲在场守卫：登记目录为空、缺根总纲或跟踪的登记文件不在工作区，即输入
    不可读，无法完成判定，失败关闭退出；包清单缺席走双向核对判缺登记，不算前置故障。"""
    tracked_missing = [rel for rel in inventory if not os.path.isfile(os.path.join(root, rel))]
    if BDD_REL in inventory and not tracked_missing:
        return
    missing = tracked_missing or [BDD_REL]
    for rel in missing:
        gate_report.fail(
            gate_report.Finding(
                rule=gate_report.RULE_INFRA,
                path=rel,
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


def entry_finding(
    rule: str,
    message: str,
    remedy: str,
    entries: list[str],
    path: str = BDD_REL,
) -> gate_report.Finding:
    """单类违规的条目：位置取实际登记文件，条目清单并成一行证据。"""
    return gate_report.Finding(
        rule=rule,
        path=path,
        line=0,
        message=message,
        evidence=", ".join(entries),
        remedy=remedy,
    )


def collect(root: str, loader: Any = None) -> tuple[list[str], list[gate_report.Finding]]:
    """双向核对与结构检查，违规随执行流即时经统一诊断出口打到标准错误，返回通过
    与违规条目两个清单；装入器在场时清单与文件读取走共享缓存，缺省自行读取。"""
    tracked = tracked_files(root, loader)
    inventory = test_index.inventory_from_tracked(tracked)
    require_inventory(root, inventory)

    src_instances = src_test_instances(root, loader=loader)
    declared_targets = target_test_names(root, loader=loader)
    target_instances = target_test_instances(root, loader=loader, declared=declared_targets)
    instances: dict[str, set[str]] = {}
    for source in (src_instances, target_instances):
        for name, rels in source.items():
            instances.setdefault(name, set()).update(rels)
    # 双向核对按名字集合：测试目标声明即登记面；限定符检查按文件实例
    src_all = set(instances) | declared_targets
    require_sources(src_all)

    found: list[gate_report.Finding] = []

    def emit(finding: gate_report.Finding) -> None:
        found.append(finding)
        gate_report.fail(finding)

    per_file: dict[str, dict[str, Any]] = {}
    for rel in inventory:
        path = os.path.join(root, rel)
        per_file[rel] = {
            "names": bdd_entries(path, loader),
            "registrations": bdd_registrations(path, loader),
            "headings": heading_paths(path, loader),
            "unit_sections": unit_section_paths(path, loader),
            "dup_rows": duplicate_rows(path, loader),
            "dup_chapters": duplicate_chapters(path, loader),
            "dup_blocks": duplicate_blocks(path, loader),
            "broken_tables": broken_tables(path, loader),
        }

    bdd_names: set[str] = set()
    registrations: list[dict[str, Any]] = []
    for rel in inventory:
        bdd_names |= per_file[rel]["names"]
        registrations.extend(per_file[rel]["registrations"])

    stale = sorted(bdd_names - src_all)
    if stale:
        emit(
            entry_finding(
                RULE_BDD_STALE,
                "these inventory entries no longer exist in test sources"
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
                "these tests are not registered in docs/tests/",
                SYNC_REMEDY,
                missing,
            )
        )
    for rule, message, remedy, key in (
        (
            RULE_BDD_DUP_ROW,
            "these inventory entries are registered multiple times (entry count exceeds"
            " same-name tests in sources; old rows left after a rename?)",
            DUP_REMEDY,
            "dup_rows",
        ),
        (
            RULE_BDD_DUP_CHAPTER,
            "these ## chapter headings appear more than once in one inventory file"
            " (sign of duplicated blocks)",
            DUP_REMEDY,
            "dup_chapters",
        ),
        (
            RULE_BDD_DUP_BLOCK,
            "these section headings or bullet lines repeat within the same"
            " chapter/section (sign of duplicated blocks)",
            DUP_REMEDY,
            "dup_blocks",
        ),
        (
            RULE_BDD_TABLE,
            "these sections have tables missing a header row (or the body is cut from"
            " the header by a blank line/prose)",
            TABLE_REMEDY,
            "broken_tables",
        ),
    ):
        for rel in inventory:
            entries = per_file[rel][key]
            if entries:
                emit(entry_finding(rule, message, remedy, entries, path=rel))

    stale_paths: dict[str, list[str]] = {}
    for rel in inventory:
        stale_in_file = sorted(p for p in per_file[rel]["headings"] if p not in set(tracked))
        if stale_in_file:
            stale_paths[rel] = stale_in_file
    for rel, entries in sorted(stale_paths.items()):
        emit(
            entry_finding(
                RULE_BDD_PATH,
                "these section heading paths are not git-tracked source files",
                PATH_REMEDY,
                entries,
                path=rel,
            )
        )
    for finding in qualifier_findings(registrations, instances):
        emit(finding)
    registered = unit_registered_files(registrations, instances)
    for finding in unit_map_findings(inventory, per_file, registered):
        emit(finding)
    for finding in overview_findings(root, inventory, loader):
        emit(finding)

    oks = []
    if not found:
        oks.append(
            "%d inventory files match test sources (%d in sources / %d in inventory)"
            % (len(inventory), len(src_all), len(bdd_names))
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
    """双向核对，返回（oks、fails）两清单；失败清单收渲染行（消息、位置与条目证据）
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
    gate_report.banner("test behavior inventory gate", "repo root: %s" % root)
    oks, found = collect(root, loader)
    # 违规已在扫描时即时输出，这里只补汇总与结论
    for line in oks:
        print("  ok: %s" % line)
    if found:
        gate_report.finish(found)
        return gate_report.EXIT_VIOLATION
    return gate_report.EXIT_PASS


if __name__ == "__main__":
    sys.exit(main())
