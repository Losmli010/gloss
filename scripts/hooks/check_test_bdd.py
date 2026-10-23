#!/usr/bin/env python3
# BDD 清单一致性门禁：docs/tests/bdd.md 与测试源码双向核对。
#   - 测试源码侧：git 跟踪 .rs 里的 #[test] / #[tokio::test] 函数，加上
#     Cargo.toml 声明的 [[test]] 目标（harness = false 自检），每一项都必须
#     登记在 bdd.md；
#   - 清单侧：bdd.md 的每个条目必须能在测试源码里找到同名测试。
# 条目的描述文字不在校验面（描述与代码冲突时以代码为准，见 AGENTS.md「测试」节）。
# 本地 `just test-bdd`（pre-commit 的一部分）与 CI 的 Test BDD check job 共用此脚本。
#
# 用法：scripts/hooks/check_test_bdd.py [仓库根]   # 缺省为本脚本的上一级目录

import os
import re
import subprocess
import sys
from collections import Counter

BDD_REL = os.path.join("docs", "tests", "bdd.md")
FN_RE = re.compile(r"fn\s+([A-Za-z0-9_]+)")
TEST_ATTR_RE = re.compile(r"^\s*#\[(?:tokio::)?test\]")
MANUAL_CHAPTER = "## 发版人工步骤"


def tracked_rs_files(root):
    out = subprocess.run(
        ["git", "-C", root, "ls-files", "-z", "--", "*.rs"],
        capture_output=True,
        check=True,
    ).stdout
    return [p.decode("utf-8", "replace") for p in out.split(b"\0") if p]


def src_test_names(root):
    """git 跟踪 .rs 里的 #[test] / #[tokio::test] 函数名。

    属性行触发 pending；跳过中间的其它属性行（如 #[ignore = \"...\"]）取最近的
    fn 名；属性与 fn 同行也支持。"""
    names = set()
    for rel in tracked_rs_files(root):
        pending = False
        with open(os.path.join(root, rel), encoding="utf-8", errors="replace") as fh:
            for line in fh:
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


def manifest_files(root):
    out = [os.path.join(root, "Cargo.toml")]
    crates = os.path.join(root, "crates")
    if os.path.isdir(crates):
        for name in sorted(os.listdir(crates)):
            m = os.path.join(crates, name, "Cargo.toml")
            if os.path.isfile(m):
                out.append(m)
    return out


def target_test_names(root):
    """[[test]] 声明的测试目标名（harness = false 自检二进制没有 #[test] 函数）。"""
    names = set()
    for manifest in manifest_files(root):
        if not os.path.isfile(manifest):
            continue
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


def bdd_entries(bdd_path):
    """清单侧条目名：表格取首列，人工测试取 ### 小节标题；剥掉快照重名的
    （popup）/（settings）限定后缀；表头、分隔行与 crates/... 文件标题经标识符
    过滤自然排除。「## 发版人工步骤」节登记无自动化测试源码的发版链路运维步骤，
    整节豁免双向核对。"""
    names = []
    exempt = False
    with open(bdd_path, encoding="utf-8") as fh:
        for line in fh:
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


def duplicate_rows(bdd_path):
    """整行精确重复（同名同目标同场景同时间）＝改名后旧行未删的真实特征。
    同名不同描述的沿革行（历史重录记录）是有意保留，不拦。"""
    rows = []
    exempt = False
    with open(bdd_path, encoding="utf-8") as fh:
        for line in fh:
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


def duplicate_chapters(bdd_path):
    chapters = []
    with open(bdd_path, encoding="utf-8") as fh:
        for line in fh:
            if line.startswith("## "):
                chapters.append(line.rstrip("\n"))
    return sorted(k for k, c in Counter(chapters).items() if c > 1)


def duplicate_blocks(bdd_path):
    """同一 ## 章节内重复的 ### 小节标题，或同一小节内逐字重复的「- 」条目行
    ——人工测试条目整段重复拼接的痕迹（表格行的重复由整行精确重复检查覆盖）。"""
    dups = set()
    chapter = ""
    section = ""
    exempt = False
    h3 = set()
    bullets = set()
    with open(bdd_path, encoding="utf-8") as fh:
        for raw in fh:
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


def broken_tables(bdd_path):
    """结构校验：非豁免小节里的每个表格必须是良构 GFM 表——表行连成一个块，
    且块的第二行是分隔行。空行或正文混在表头与表体之间会把表体从表头上
    切断（渲染退化为纯文本），一律按缺表头列出。"""
    broken = {}
    section = ""
    exempt = False
    state = {"in_run": False, "run_len": 0, "run_ok": False}

    def end_run():
        if state["in_run"] and not state["run_ok"] and not exempt and section != "":
            broken[section] = True
        state["in_run"] = False

    with open(bdd_path, encoding="utf-8") as fh:
        for raw in fh:
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


def run(root):
    """双向核对，返回 (oks, fails)。"""
    bdd_path = os.path.join(root, BDD_REL)
    if not os.path.isfile(bdd_path):
        raise SystemExit(
            "错误：找不到 %s\n  测试行为清单是 git 追踪文件，别删除或改名；"
            "目录被 gitignore 例外放行。" % BDD_REL
        )

    src_names = src_test_names(root)
    target_names = target_test_names(root)
    bdd_names = bdd_entries(bdd_path)

    if not src_names and not target_names:
        raise SystemExit(
            "错误：没有从测试源码提取到任何测试（git 跟踪的 .rs 与 [[test]] 目标均为空）。"
        )

    fails = []
    # 两侧各自已去重，但 fn 名与 [[test]] 目标名可能同名（如 overlay 自检），
    # 合并后须再去重，否则差集把多出的那份误报成「未登记」。
    src_all = src_names | target_names
    stale = sorted(bdd_names - src_all)
    missing = sorted(src_all - bdd_names)
    if stale:
        fails.append(
            "以下 bdd.md 条目在测试源码中不存在（改名或删除后未同步）：\n%s"
            % "\n".join("    %s" % s for s in stale)
        )
    if missing:
        fails.append("以下测试未登记进 bdd.md：\n%s" % "\n".join("    %s" % s for s in missing))
    dups = duplicate_rows(bdd_path)
    if dups:
        fails.append(
            "以下 bdd.md 条目重复登记（同名条目数超过源码同名测试数，"
            "改名后旧行未删？）：\n%s" % "\n".join("    %s" % s for s in dups)
        )
    dups = duplicate_chapters(bdd_path)
    if dups:
        fails.append(
            "以下 ## 章节标题重复出现（整段重复拼接的痕迹）：\n%s"
            % "\n".join("    %s" % s for s in dups)
        )
    dups = duplicate_blocks(bdd_path)
    if dups:
        fails.append(
            "以下小节标题或条目行在同一章节/小节内重复出现"
            "（整段重复拼接的痕迹）：\n%s" % "\n".join("    %s" % s for s in dups)
        )
    broken = broken_tables(bdd_path)
    if broken:
        fails.append(
            "以下小节的表格缺表头（或表体被空行/正文从表头切断）：\n%s"
            % "\n".join("    %s" % s for s in broken)
        )

    oks = []
    if not fails:
        oks.append(
            "bdd.md 与测试源码一致（源码 %d 项 / 清单 %d 项）" % (len(src_all), len(bdd_names))
        )
    return oks, fails


def main():
    root = (
        os.path.abspath(sys.argv[1])
        if len(sys.argv) > 1
        else os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    )
    oks, fails = run(root)
    for line in oks:
        print("  ✓ %s" % line)
    if fails:
        for line in fails:
            print("  ✗ %s" % line, file=sys.stderr)
        print("", file=sys.stderr)
        print("错误：bdd.md 与测试源码不一致（见上）。", file=sys.stderr)
        print(
            "  - 新增 / 改名 / 删除测试：在同一 PR 内同步登记 bdd.md 并刷新该条目更新时间；",
            file=sys.stderr,
        )
        print("  - 描述与代码冲突：以代码为准，立即修正 bdd.md。", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
