#!/usr/bin/env python3
"""语言规范·代码引用：正文与代码混用的记号规则。结构性记号先豁免——行内
反引号、链接、强调、围栏、表格、列表与勾选框、日期、行尾放行标记——剥净
后无可判语言的纯记号段放行；残留代码段判罚，主体落中文区判定。行级复合
判定与标记语言文档逐行遍历是两个入口，被注释、提交信息与拉取请求内容门
禁复用；本模块自身注释同规。

模块名避开解释器标准库的交互解释器模块。
"""

import re

from language_spec.chinese import CJK, body_problems

# 各门禁共用的行尾放行标记：紧贴所在行并注明缘由即豁免
MARKER = re.compile(r"\b(?:secrets|lines|i18n|constraints|deps):allow\b")

DATE_RE = re.compile(r"\d{4}-\d{2}-\d{2}")


def strip_links(text):
    """链接与图片逐层剥除：徽章是套着图片的嵌套链接，先剥内层图片再剥外层。"""
    for pattern in (r"!\[[^\]]*\]\([^)]*\)", r"\[[^\]]*\]\([^)]*\)"):
        while True:
            stripped = re.sub(pattern, " ", text)
            if stripped == text:
                break
            text = stripped
    return text


def prose_line_problems(loc, text):
    """单行表述判定：结构性记号豁免后落中文区判定，主体必须中文。"""
    text = text.strip()
    if not text:
        return []
    text = re.sub(r"^(?:[-*+]|\d+[.、]|>)\s+", "", text)  # 列表与引用记号非表述
    text = re.sub(r"^\[[ xX]\]\s+", "", text)  # 任务列表记号：勾选框非表述
    text = re.sub(r"`[^`]*`", " ", text)  # 行内代码：命令、路径与标识符豁免
    text = strip_links(text)
    text = text.replace("**", " ")  # 强调记号：包裹的仍是中文表述，记号非内容
    text = DATE_RE.sub(" ", text)
    if re.fullmatch(r"[A-Za-z0-9_]+", text.strip()):
        return []  # 整段标识符（如类别键）豁免
    if not re.search(r"[A-Za-z\u4e00-\u9fff]", text):
        return []  # 剥去代码与日期后只剩数字符号（纯命令格、分隔符）无可判语言
    if "`" in text:
        return [
            f"{loc}: text contains a code segment — move references and samples to the record files"
        ]
    if "§" in text:
        return [
            f'{loc}: text contains a "§ section number" style external reference — cite it in the record files'
        ]
    if not CJK.search(text):
        return [f"{loc}: text must be written in Chinese: {text[:40]}"]
    return body_problems(loc, text, "text")


def markdown_problems(rel, src):
    """标记语言文档逐行受检：代码围栏、行内反引号与链接豁免；表格首列与纯
    标识符小节标题是双向核对的键，豁免；日期是语言中立数据格式，放行。"""
    out = []
    in_fence = False
    for lineno, line in enumerate(src.splitlines(), 1):
        if line.strip().startswith("```"):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        out += markdown_line_problems(rel, lineno, line)
    return out


def markdown_line_problems(rel, lineno, line):
    stripped = line.strip()
    if not stripped:
        return []
    loc = f"{rel}:{lineno}"
    heading = re.match(r"^#+\s*(.*)$", stripped)
    if heading:
        key = re.sub(r"（[^）]*）\s*$", "", heading.group(1)).strip()
        if re.fullmatch(r"[A-Za-z0-9_]+", key):
            return []  # 小节标题即测试名，双向核对的键
        stripped = heading.group(1)
    if stripped.startswith("|"):
        cells = stripped.strip("|").split("|")
        if all(re.fullmatch(r"[-: ]*", cell) for cell in cells):
            return []  # 表格分隔行
        out = []
        for cell in cells[1:]:
            out += prose_line_problems(loc, cell)
        return out
    return prose_line_problems(loc, stripped)
