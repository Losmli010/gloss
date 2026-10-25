#!/usr/bin/env python3
"""提交信息规范校验：本地提交钩子、编码助手（ZCode）客户端的拉取请求标题检查与持续集成检查
共用此单点，保证多处判定一致。

规则：标题形如“类型（可选范围）可加叹号，冒号后接主题”；类型白名单见下方常量；
标题按编码（UTF-8）字节计不超上限（跨实现一致）；正文与标题之间必须有空行。
井号起头的注释行只在多行输入时剥离——单行输入没有注释区，井号是标题本身
（如议题引用式拉取请求标题），剥离会被误报为空消息。

主题与正文逐行过书写纪律公共规则——主体全中文、专名只进全角括号注记、
行内反引号豁免；合并编号后缀、版本库尾注与压缩合并正文清单条目的类型
前缀等结构性要素豁免，判定与注释门禁、拉取请求内容门禁同源。

用法：以解释器运行本脚本，首参为消息文件路径，或传单个减号表示从标准输入读取
（本地钩子传文件路径，持续集成传减号）。
"""

import re
import sys

from language_spec.coderef import prose_line_problems

ALLOWED_TYPES = "feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert"
HEADER_RE = re.compile(r"^(%s)(\([a-zA-Z0-9._-]+\))?!?: " % ALLOWED_TYPES)
SUBJECT_RE = re.compile(r"^[a-z]+(\([a-zA-Z0-9._-]+\))?!?: ")
MAX_HEADER_BYTES = 81
# 版本库尾注（合作者、签署等结构性行）不按表述判罚
TRAILER_RE = re.compile(r"^[A-Za-z][A-Za-z0-9-]*:\s")
# 压缩合并把拉取请求内各提交的主题列成清单写进正文，条目自带类型前缀；
# 清单是托管平台生成的结构，条目前缀按主题行同规剥掉再判罚
LIST_ENTRY_RE = re.compile(r"^(?:[-*+]\s+)?(?:%s)(?:\([a-zA-Z0-9._-]+\))?!?: " % ALLOWED_TYPES)
# 压缩合并把拉取请求编号追加到主题尾，判定前剥掉
ISSUE_SUFFIX_RE = re.compile(r"\s*\(#\d+\)\s*$")


def split_message(msg):
    """预处理出待校验的行序列：剥注释行、去掉开头的空白行。"""
    msg = msg.rstrip("\n")
    if "\n" in msg:
        msg = "\n".join(line for line in msg.split("\n") if not line.startswith("#"))
    lines = msg.split("\n")
    first = 0
    while first < len(lines) and not lines[first].strip():
        first += 1
    return lines[first:]


def prose_problems(lines):
    """主题与正文逐行过书写规则；结构性要素豁免——编号后缀、版本库尾注与
    压缩合并清单条目的类型前缀。"""
    problems = []
    subject = ISSUE_SUFFIX_RE.sub("", SUBJECT_RE.sub("", lines[0], count=1))
    problems += prose_line_problems("commit subject", subject)
    for index, line in enumerate(lines[1:], 2):
        stripped = line.strip()
        if not stripped or TRAILER_RE.match(stripped):
            continue
        entry = LIST_ENTRY_RE.sub("", stripped, count=1)
        problems += prose_line_problems("commit body line %d" % index, entry)
    return problems


def run(msg):
    """校验一条 commit message，返回多行错误串；通过返回 None。"""
    lines = split_message(msg)
    if not lines:
        return "error: commit message is empty"

    header = lines[0]
    if not HEADER_RE.match(header):
        return (
            "error: commit message does not follow Conventional Commits\n"
            "\n"
            "  required format: <type>[optional scope]: <subject>\n"
            "  allowed types: %s\n"
            "  examples:\n"
            "    feat: 新增翻译浮层\n"
            "    fix(core): 修复选区读取崩溃\n"
            "    chore(deps): 升级 `egui` 到 0.31\n"
            "    docs: 扩充使用说明\n"
            "\n"
            "  you submitted: %s" % (ALLOWED_TYPES, header)
        )

    subject = SUBJECT_RE.sub("", header, count=1)
    if not subject:
        return (
            "error: commit header is missing a subject (content expected after the colon)\n"
            "  you submitted: %s" % header
        )

    header_bytes = len(header.encode("utf-8"))
    if header_bytes > MAX_HEADER_BYTES:
        return (
            "error: commit header too long (%d bytes, limit %d)\n"
            "  trim it and move details into the body\n"
            "  you submitted: %s" % (header_bytes, MAX_HEADER_BYTES, header)
        )

    if len(lines) > 1 and lines[1] != "":
        return (
            "error: commit header must be followed by a blank line before the body\n"
            "  line 2 should be empty; you wrote: %s" % lines[1]
        )

    problems = prose_problems(lines)
    if problems:
        return (
            "error: commit message violates the shared Chinese prose rules\n"
            "  proper nouns go in fullwidth bracket annotations; inline code stays in backticks\n"
            + "\n".join("  %s" % p for p in problems)
        )
    return None


def main():
    if len(sys.argv) < 2:
        print("usage: %s <commit-message-file|->" % sys.argv[0], file=sys.stderr)
        return 2
    if sys.argv[1] == "-":
        msg = sys.stdin.read()
    else:
        with open(sys.argv[1], encoding="utf-8", errors="replace") as fh:
            msg = fh.read()
    problem = run(msg)
    if problem:
        print(problem, file=sys.stderr)
        return 1
    print("✓ commit message ok: %s" % split_message(msg)[0])
    return 0


if __name__ == "__main__":
    sys.exit(main())
