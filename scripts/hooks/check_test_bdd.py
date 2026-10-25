#!/usr/bin/env python3
"""测试行为清单一致性门禁：登记文档与测试源码双向核对。
  - 测试源码侧：版本库跟踪的本语言（Rust）源码里的测试函数，加上清单声明的
    自管测试目标（自带运行器的自检目标），每一项都必须登记进清单；
  - 清单侧：清单的每个条目必须能在测试源码里找到同名测试。
条目的描述文字不在校验面（描述与代码冲突时以代码为准，见指令文档的测试节）。
本地提交前门禁与持续集成检查共用此脚本，保证本地与远端判定一致。

用法：以解释器运行本脚本，首参为仓库根，缺省为本脚本的上一级目录。
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
from collections import Counter
from typing import Any

BDD_REL = os.path.join("docs", "tests", "bdd.md")
FN_RE = re.compile(r"fn\s+([A-Za-z0-9_]+)")
TEST_ATTR_RE = re.compile(r"^\s*#\[(?:tokio::)?test\]")
MANUAL_CHAPTER = "## 发版人工步骤"


def tracked_rs_files(root: str, loader: Any = None) -> list[str]:
    if loader is not None:
        return [p for p in loader.tracked() if p.endswith(".rs")]
    out = subprocess.run(
        ["git", "-C", root, "ls-files", "-z", "--", "*.rs"],
        capture_output=True,
        check=True,
    ).stdout
    return [p.decode("utf-8", "replace") for p in out.split(b"\0") if p]


def src_test_names(root: str, loader: Any = None) -> set[str]:
    """git 跟踪 .rs 里的 #[test] / #[tokio::test] 函数名。

    属性行触发 pending；跳过中间的其它属性行（如 #[ignore = \"...\"]）取最近的
    fn 名；属性与 fn 同行也支持。"""
    names = set()
    for rel in tracked_rs_files(root, loader):
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
                    names.add(m.group(1))
                else:
                    pending = True
                continue
            if pending:
                if re.match(r"^\s*#", line):
                    continue
                m = FN_RE.search(line)
                if m:
                    names.add(m.group(1))
                    pending = False
    return names


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
    """[[test]] 声明的测试目标名（harness = false 自检二进制没有 #[test] 函数）。"""
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


def run(root: str, loader: Any = None) -> tuple[list[str], list[str]]:
    """双向核对，返回 (oks, fails)；装入器在场时清单与文件读取走共享缓存，
    缺省自行读取。"""
    bdd_path = os.path.join(root, BDD_REL)
    if not os.path.isfile(bdd_path):
        raise SystemExit(
            "error: %s not found\n"
            "  the test behavior inventory is a git-tracked file; do not delete or"
            " rename it; the directory is allowed via a gitignore exception." % BDD_REL
        )

    src_names = src_test_names(root, loader)
    target_names = target_test_names(root, loader)
    bdd_names = bdd_entries(bdd_path, loader)

    if not src_names and not target_names:
        raise SystemExit(
            "error: no tests extracted from test sources"
            " (git-tracked .rs and [[test]] targets are both empty)."
        )

    # 违规随执行流即时输出，对齐壳脚本（bash）版的标准错误交错，列表供调用方取用
    fails = []

    def fail(msg: str) -> None:
        fails.append(msg)
        print("  ✗ %s" % msg, file=sys.stderr)

    # 两侧各自已去重，但测试函数名与测试目标名可能同名（如浮层自检），
    # 合并后须再去重，否则差集把多出的那份误报成“未登记”。
    src_all = src_names | target_names
    stale = sorted(bdd_names - src_all)
    missing = sorted(src_all - bdd_names)
    if stale:
        fail(
            "these bdd.md entries no longer exist in test sources"
            " (renamed or removed without syncing):\n%s" % "\n".join("    %s" % s for s in stale)
        )
    if missing:
        fail(
            "these tests are not registered in bdd.md:\n%s"
            % "\n".join("    %s" % s for s in missing)
        )
    dups = duplicate_rows(bdd_path, loader)
    if dups:
        fail(
            "these bdd.md entries are registered multiple times (entry count exceeds"
            " same-name tests in sources; old rows left after a rename?):\n%s"
            % "\n".join("    %s" % s for s in dups)
        )
    dups = duplicate_chapters(bdd_path, loader)
    if dups:
        fail(
            "these ## chapter headings appear more than once"
            " (sign of duplicated blocks):\n%s" % "\n".join("    %s" % s for s in dups)
        )
    dups = duplicate_blocks(bdd_path, loader)
    if dups:
        fail(
            "these section headings or bullet lines repeat within the same"
            " chapter/section (sign of duplicated blocks):\n%s"
            % "\n".join("    %s" % s for s in dups)
        )
    broken = broken_tables(bdd_path, loader)
    if broken:
        fail(
            "these sections have tables missing a header row (or the body is cut from"
            " the header by a blank line/prose):\n%s" % "\n".join("    %s" % s for s in broken)
        )

    oks = []
    if not fails:
        oks.append(
            "ok bdd.md matches test sources (%d in sources / %d in inventory)"
            % (len(src_all), len(bdd_names))
        )
    return oks, fails


def main(loader: Any = None) -> int:
    root = (
        os.path.abspath(sys.argv[1])
        if len(sys.argv) > 1
        else os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    )
    oks, fails = run(root, loader)
    # 违规已在扫描时即时输出，这里只补汇总与结论
    for line in oks:
        print("  ✓ %s" % line)
    if fails:
        print("", file=sys.stderr)
        print("error: bdd.md and test sources disagree (see above).", file=sys.stderr)
        print(
            "  - added/renamed/removed tests: register them in bdd.md within the same PR"
            " and refresh the entry's update time;",
            file=sys.stderr,
        )
        print(
            "  - description conflicts with code: code wins; fix bdd.md immediately.",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
