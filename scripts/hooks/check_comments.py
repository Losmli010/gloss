#!/usr/bin/env python3
# 注释纪律门禁：生产与测试 .rs 源码只保留文件头注释（`//!`），其余注释一律
# 迁往 docs/comments/<文件名>_comment.md。本地 `just comments`（pre-commit 的
# 一部分）与 CI 的 hooks.yml 共用此脚本，保证本地与 CI 判定一致。
#
# 覆盖五条检查：
#   只留文件头      —— `///` 文档注释、行内 `//`、行尾注释、块注释 `/* */` 一律
#                      违规；`//!` 只能出现在文件头（首个代码项之前）。
#   文件头必须存在  —— 每个被跟踪的 .rs 至少一行 `//!` 头。
#   文件头是中文    —— 每行 `//!` 必须含汉字；反引号代码段之外不允许出现
#                      相邻的两个英文词（专有名词单词放行，如 wgpu、LLM、
#                      gloss-core），防止整句英文混入。
#   SAFETY 豁免     —— `// SAFETY:` 块及其紧邻的连续注释行不判罚（clippy
#                      undocumented_unsafe_blocks 要求不变量紧贴 unsafe 块）。
#   行尾豁免标记    —— secrets:allow / lines:allow / i18n:allow /
#                      constraints:allow-context 放行所在行（同各门禁惯例）。
#
# 词法状态机区分 代码 / 注释 / 字符串 / char / 原始字符串 / 块注释（可嵌套），
# 字符串与属性内的 "//"（如 URL）不误报。
#
# 用法：python3 scripts/hooks/check_comments.py [仓库根] [文件...]
#       文件参数供单测传入临时样本；缺省扫全部被跟踪的 .rs

import os
import re
import subprocess
import sys

CJK = re.compile(r"[\u4e00-\u9fff]")
WORD = re.compile(r"[A-Za-z][A-Za-z0-9_-]*")
MARKER = re.compile(r"\b(?:secrets|lines|i18n|constraints|deps):allow\b")


def scan_files(root, explicit):
    if explicit:
        return [p if os.path.isabs(p) else os.path.join(root, p) for p in explicit]
    out = subprocess.run(
        ["git", "-C", root, "ls-files", "*.rs"],
        capture_output=True, text=True, check=True,
    ).stdout
    return [os.path.join(root, p) for p in out.splitlines() if p]


def run(root, explicit):
    problems = []

    def lex(src):
        """按字符扫描，产出注释事件：
        (seq, start_line, end_line, kind, indent_only, text)
        kind ∈ inner/doc/line/block；indent_only 指该行注释前只有空白。"""
        newlines = [m.start() for m in re.finditer("\n", src)]

        def line_of(i):
            import bisect
            return bisect.bisect_right(newlines, i) + 1

        events = []
        i, n = 0, len(src)
        seq = 0
        code_seen = False          # 是否已越过文件头（见过代码项）
        line_has_code = False      # 当前行注释前是否有代码
        line_starts_blank = True   # 当前行目前只有空白
        inner_attr_only = True     # 当前行目前只有内部属性 #![...]（不算代码项）

        while i < n:
            c = src[i]
            if c == "\n":
                i += 1
                line_has_code = False
                line_starts_blank = True
                inner_attr_only = True
                continue
            if c in " \t\r":
                i += 1
                continue
            if c == "/" and i + 1 < n and src[i + 1] == "/":
                start = i
                indent_only = line_starts_blank
                before_code = code_seen
                j = i + 2
                while j < n and src[j] != "\n":
                    j += 1
                body = src[i:j]
                if body.startswith("//!"):
                    kind = "inner"
                elif body.startswith("///"):
                    kind = "doc"
                else:
                    kind = "line"
                events.append((seq, line_of(start), line_of(j) if j < n else line_of(n - 1),
                               kind, indent_only, before_code, body))
                seq += 1
                i = j
                continue
            if c == "/" and i + 1 < n and src[i + 1] == "*":
                start = i
                before_code = code_seen
                depth = 1
                i += 2
                while i < n and depth:
                    if src.startswith("/*", i):
                        depth += 1
                        i += 2
                    elif src.startswith("*/", i):
                        depth -= 1
                        i += 2
                    else:
                        i += 1
                end = min(i, n - 1)
                events.append((seq, line_of(start), line_of(end), "block",
                               line_starts_blank, before_code, src[start:i]))
                seq += 1
                line_has_code = False
                line_starts_blank = True
                inner_attr_only = True
                continue
            if c == '"':
                line_has_code = True
                inner_attr_only = False
                i += 1
                while i < n:
                    if src[i] == "\\":
                        i += 2
                    elif src[i] == '"':
                        i += 1
                        break
                    else:
                        i += 1
                continue
            if c in "rb" and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")):
                # 原始字符串 r"..." / r#"..."#（含 b 前缀字节串）
                k = i + 1
                if c == "b" and k < n and src[k] == "r":
                    k += 1
                hashes = 0
                while k < n and src[k] == "#":
                    hashes += 1
                    k += 1
                if k < n and src[k] == '"':
                    line_has_code = True
                    inner_attr_only = False
                    i = k + 1
                    while i < n:
                        if src[i] == '"':
                            m = 0
                            while m < hashes and i + 1 + m < n and src[i + 1 + m] == "#":
                                m += 1
                            if m == hashes:
                                i += 1 + hashes
                                break
                        i += 1
                    continue
            if c == "'":
                m = re.match(r"'(\\.[^']*|[^'\\])'", src[i:i + 8])
                if m:
                    line_has_code = True
                    inner_attr_only = False
                    i += m.end()
                    continue
                i += 1  # 生命周期标注（'a）不是字面量
                continue
            if c == "#" and inner_attr_only and src[i:i + 3] == "#![":
                # 内部属性 #![...]：字符串感知地跳到配对方括号，不视为代码项
                depth = 1
                i += 3
                while i < n and depth:
                    if src[i] == '"':
                        i += 1
                        while i < n and src[i] != '"':
                            i += 2 if src[i] == "\\" else 1
                        i += 1
                        continue
                    if src[i] == "[":
                        depth += 1
                    elif src[i] == "]":
                        depth -= 1
                    i += 1
                continue
            # 其余一概视为代码
            line_has_code = True
            inner_attr_only = False
            line_starts_blank = False
            code_seen = True
            i += 1

        return events


    def header_violations(lines, rel):
        out = []
        for text in lines:
            body = text[3:].strip() if text.startswith("//!") else text.strip()
            if not body:
                continue
            if not CJK.search(body):
                out.append(f"{rel}: 文件头含无汉字的行：{body[:60]}")
            cleaned = re.sub(r"`[^`]*`", " ", body)
            toks = [(m.start(), m.end()) for m in WORD.finditer(cleaned)]
            for (s1, e1), (s2, e2) in zip(toks, toks[1:]):
                if not cleaned[e1:s2].strip():
                    out.append(f"{rel}: 文件头出现相邻英文词："
                               f"{cleaned[s1:e2]}（专有名词之间补中文或加反引号）")
                    break
        return out

    for path in scan_files(root, explicit):
        rel = os.path.relpath(path, root)
        try:
            src = open(path, encoding="utf-8").read()
        except (OSError, UnicodeDecodeError) as exc:
            problems.append(f"{rel}: 无法读取（{exc}）")
            continue
        events = lex(src)
        header_lines = []
        header_found = False
        safety_open = False
        prev_end = None
        for seq, sline, eline, kind, indent_only, before_code, text in events:
            if prev_end is not None and sline > prev_end + 1:
                safety_open = False  # 与上一条注释之间有代码或空行
            prev_end = eline
            if kind == "inner":
                if before_code:
                    problems.append(f"{rel}:{sline}: //! 出现在文件头之后，只能作文件头")
                else:
                    header_found = True
                    header_lines.append(text)
                continue
            if kind == "doc":
                problems.append(f"{rel}:{sline}: /// 文档注释 —— 内容迁 docs/comments/ 后删除")
                continue
            if kind == "block":
                problems.append(f"{rel}:{sline}: 块注释 —— 内容迁 docs/comments/ 后删除")
                continue
            # kind == "line"
            if MARKER.search(text):
                continue
            if not indent_only:
                problems.append(f"{rel}:{sline}: 行尾注释 —— 迁 docs/comments/，"
                                f"豁免标记除外")
                continue
            stripped = text.strip()
            body = stripped.lstrip("/").strip()
            if safety_open or body.startswith("SAFETY:"):
                safety_open = True
                continue
            problems.append(f"{rel}:{sline}: 行内注释 —— 迁 docs/comments/"
                            f"（SAFETY 与 :allow 除外）")
        if not header_found:
            problems.append(f"{rel}: 缺文件头 //! 注释")
        problems.extend(header_violations(header_lines, rel))
    return problems


def main():
    root = sys.argv[1] if len(sys.argv) > 1 else os.path.abspath(
        os.path.join(os.path.dirname(__file__), "..", ".."))
    explicit = sys.argv[2:]
    print(f"== 注释纪律门禁（仓库根：{root}）==")
    problems = run(root, explicit)
    if problems:
        for p in problems:
            print(f"  ✗ {p}", file=sys.stderr)
        print(f"✗ 注释纪律门禁：{len(problems)} 处违规", file=sys.stderr)
        print("  规则见本脚本头注；迁移目标 docs/comments/<文件名>_comment.md",
              file=sys.stderr)
        sys.exit(1)
    print("  ok 只留文件头：全部跟踪 .rs 通过（///、行内/行尾注释零残留，文件头均为中文）")


if __name__ == "__main__":
    main()
