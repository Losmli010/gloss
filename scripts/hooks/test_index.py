#!/usr/bin/env python3
"""测试行为清单总览生成器：根总纲的总览节（类别×包计数矩阵加登记文件行数）。

登记清单按包落位在文档目录（docs/tests/）下，总览节是唯一的跨文件视图，由本模块机械生成：计数按章归类——表格计登记行（表头与分隔行除外），人工两类计条目小节标题，逐源文件小节标题（路径形态）不是条目。生成与校验共用本模块：总览再生成配方（just test-index）调用命令入口写回，行为清单门禁（check_test_bdd）比对现场与再生结果判索引过期。登记文件清单、列序与类目序全部机械推导，无手工映射。

用法：python3 scripts/hooks/test_index.py [repo-root]，缺省为本脚本的上一级目录；
总览节已在根清单内则原位替换，缺失则插到首个章标题之前。
"""

from __future__ import annotations

import os
import re
import sys

TESTS_DIR = "docs/tests"
ROOT_INVENTORY = "docs/tests/bdd.md"
OVERVIEW_TITLE = "总览"
OVERVIEW_CHAPTER = "## " + OVERVIEW_TITLE
UNIT_CHAPTER = "## 单元测试"

# 列序：根总纲打头，包清单按仓库分层顺序（与指令文档的目录结构一致），其余文件按名排在后面
COLUMN_ORDER = ("gloss-core", "gloss-platform", "gloss-app", "gloss-eval", "gloss")
# 类目行序：与根清单的组织段同源，登记文件里出现额外章名时按名排在标准类目之后
CATEGORY_ORDER = ("人工测试", "发版人工步骤", "集成测试", "性能测试", "快照测试", "单元测试")

HEADER_ROW_RE = re.compile(r"^\s*\|\s*(测试名称|类别|---)")
QUALIFIED_TAIL_RE = re.compile(r"（[^（）]*）\s*$")

SECTION_PROSE = (
    "本节由 `just test-index` 生成，请勿手改；`just test-bdd` 校验本节与登记文件一致。"
    "计数口径：表格计登记行（表头与分隔行除外），条目小节按标题计，逐源文件小节标题不是条目。"
)
LINE_COUNT_PREFIX = "登记文件行数："


def inventory_from_tracked(tracked: list[str]) -> list[str]:
    """登记文件清单：文档目录（docs/tests/）下、版本库跟踪清单在册的标记语言文档，
    按名排序；仓库相对路径一律以斜杠分隔比较，与版本库清单的输出形态一致。"""
    prefix = TESTS_DIR + "/"
    return sorted(rel for rel in tracked if rel.startswith(prefix) and rel.endswith(".md"))


def inventory_from_disk(root: str) -> list[str]:
    """登记文件清单（工作区面）：再生成入口用，目录里现存的登记文件按名排序。"""
    base = os.path.join(root, TESTS_DIR)
    if not os.path.isdir(base):
        return []
    return sorted(TESTS_DIR + "/" + name for name in os.listdir(base) if name.endswith(".md"))


def column_order(rels: list[str]) -> list[str]:
    """总览列序：根总纲（总览节所在文件）打头，包清单按分层顺序，其余登记文件按名
    排在其后。"""
    known: dict[str, str] = {}
    rest: list[str] = []
    for rel in rels:
        if rel == ROOT_INVENTORY:
            continue
        stem = os.path.splitext(os.path.basename(rel))[0]
        if stem in COLUMN_ORDER:
            known[stem] = rel
        else:
            rest.append(rel)
    ordered = [ROOT_INVENTORY] if ROOT_INVENTORY in rels else []
    ordered += [known[stem] for stem in COLUMN_ORDER if stem in known]
    return ordered + sorted(rest)


def package_prefix(rel: str) -> str | None:
    """登记文件的包前缀（机械推导，无手工映射）：文件名整段即包名
    （gloss-core、gloss-platform、gloss-app、gloss-eval），推到对应包的源码目录前缀；
    入口包推到入口源码目录前缀；根总纲（bdd.md）与推导不出的文件返回空值（None），
    不参与包内对账。"""
    stem = os.path.splitext(os.path.basename(rel))[0]
    if stem == "bdd":
        return None
    if stem.startswith("gloss-"):
        return "crates/%s/src/" % stem
    if stem == "gloss":
        return "src/"
    return None


def heading_path(raw: str) -> str | None:
    """三级小节标题里的源文件路径：剥限定符尾巴与反引号后含分隔符才算路径形态。"""
    text = re.sub(r"^#+\s*", "", raw).strip()
    text = QUALIFIED_TAIL_RE.sub("", text).strip()
    text = text.strip("`").strip()
    return text if "/" in text else None


def chapter_entries(lines: list[str]) -> int:
    """单章条目计数：表格计登记行（表头、分隔行除外），非路径形态的小节标题计条目。"""
    count = 0
    for line in lines:
        if line.startswith("###"):
            if heading_path(line) is None:
                count += 1
        elif line.startswith("|") and not HEADER_ROW_RE.match(line):
            count += 1
    return count


def chapter_counts(text: str) -> dict[str, int]:
    """登记文件按章计数：章名取裸标题（剥前缀记号），总览章是生成产物不计；
    同名章跨文件聚合由调用方完成。"""
    counts: dict[str, int] = {}
    title = ""
    body: list[str] = []
    for line in text.split("\n"):
        if line.startswith("## "):
            if title and title != OVERVIEW_TITLE:
                counts[title] = counts.get(title, 0) + chapter_entries(body)
            title = line[3:].strip()
            body = []
            continue
        body.append(line)
    if title and title != OVERVIEW_TITLE:
        counts[title] = counts.get(title, 0) + chapter_entries(body)
    return counts


def line_count(text: str) -> int:
    """登记文件行数：按行分割的物理行数，与逐行校验的行集合一致。"""
    return len(text.splitlines())


def render(counts: dict[str, dict[str, int]], rels: list[str], lines: dict[str, int]) -> list[str]:
    """总览节全文（含章标题）：类别×登记文件计数矩阵加登记文件行数一行。"""
    titles = [t for t in CATEGORY_ORDER if t in counts]
    titles += sorted(t for t in counts if t not in CATEGORY_ORDER)
    out = [OVERVIEW_CHAPTER, "", SECTION_PROSE, ""]
    head = "| 类别 | " + " | ".join(_column_head(rel) for rel in rels) + " | 合计 |"
    sep = "| " + " | ".join(["---"] * (len(rels) + 2)) + " |"
    out += [head, sep]
    for title in titles:
        per = [counts.get(title, {}).get(rel, 0) for rel in rels]
        out.append("| %s | %s | %d |" % (title, " | ".join(str(n) for n in per), sum(per)))
    out.append("")
    out.append(
        LINE_COUNT_PREFIX
        + "、".join("%s %d 行" % (_column_head(rel), lines[rel]) for rel in rels)
        + "。"
    )
    return out


def _column_head(rel: str) -> str:
    """列头与行数行里的文件名形态：根总纲以行内代码书写本文件（不自链），其余登记
    文件带相对链接。"""
    if rel == ROOT_INVENTORY:
        return "`bdd.md`（本文件）"
    name = os.path.basename(rel)
    return "[%s](%s)" % (name, name)


def overview_span(text: str) -> tuple[int, int]:
    """根清单里总览节的行区间（含章标题，半开区间）：到下一章标题或文件尾为止；
    总览章缺失时区间落在首个章标题之前（插入点），连章标题都没有则落在文件尾。"""
    lines = text.split("\n")
    starts = [i for i, line in enumerate(lines) if line.rstrip() == OVERVIEW_CHAPTER]
    if starts:
        start = starts[0]
        end = len(lines)
        for i in range(start + 1, len(lines)):
            if lines[i].startswith("## "):
                end = i
                break
        return start, end
    chapters = [i for i, line in enumerate(lines) if line.startswith("## ")]
    if chapters:
        return chapters[0], chapters[0]
    return len(lines), len(lines)


def build_section(root: str, rels: list[str], texts: dict[str, str]) -> list[str]:
    """再生总览节：逐份登记文件按章计数聚合后渲染。"""
    counts: dict[str, dict[str, int]] = {}
    for rel in rels:
        for title, n in chapter_counts(texts[rel]).items():
            counts.setdefault(title, {})
            counts[title][rel] = counts[title].get(rel, 0) + n
    lines = {rel: line_count(texts[rel]) for rel in rels}
    return render(counts, column_order(rels), lines)


def regenerate(root: str) -> bool:
    """把再生总览节写回根清单：内容不变不落盘，返回是否改写过。

    总览节携带根清单自身的行数，写回会改变根清单长度，故迭代到再生成结果
    稳定为止（总览行数与数字位数无关，两次内收敛）。"""
    rels = inventory_from_disk(root)
    if ROOT_INVENTORY not in rels:
        raise SystemExit("error: %s is missing; nothing to index" % ROOT_INVENTORY)
    changed = False
    for _ in range(4):
        texts = {rel: open(os.path.join(root, rel), encoding="utf-8").read() for rel in rels}
        section = "\n".join(build_section(root, rels, texts)).strip("\n")
        root_text = texts[ROOT_INVENTORY]
        lines = root_text.split("\n")
        start, end = overview_span(root_text)
        current = "\n".join(lines[start:end]).strip("\n")
        if current == section:
            return changed
        changed = True
        body = section.split("\n")
        if end < len(lines) or root_text.endswith("\n"):
            body.append("")
        lines[start:end] = body
        with open(os.path.join(root, ROOT_INVENTORY), "w", encoding="utf-8") as fh:
            fh.write("\n".join(lines))
    return changed


def main() -> int:
    root = (
        os.path.abspath(sys.argv[1])
        if len(sys.argv) > 1
        else os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    )
    changed = regenerate(root)
    print("  ok: overview %s" % ("regenerated" if changed else "already fresh"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
