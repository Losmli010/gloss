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

压缩合并的提交主题取自拉取请求标题并追加编号后缀，合并模拟的拼装在本
模块单点收口，拉取请求内容门禁、客户端标题钩子与持续集成的提交信息作
业都经它取模拟消息，标题自身合规而合并后超限同样拦下。

校验结果以带规则标识的违规条目列表返回，报告经统一诊断出口打印；退出
码按全脚本统一约定：零为通过、一为发现违规、二为用法错误。

用法：python3 scripts/hooks/check_commit_msg.py <msg-file> ...，或传单个减号表
示从标准输入读取；持续集成把一次推送或拉取请求（PR）的全部消息各落一个文件后一次
进程吃进，免逐条提交起解释器。合并模拟形态以旗标传入标题、打印模拟消息
供落盘。
"""

from __future__ import annotations

import re
import sys
from dataclasses import replace

import gate_report
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
# 压缩合并把拉取请求编号追加到主题尾，判定前剥掉；五位编号是常见的最长形态
ISSUE_SUFFIX_RE = re.compile(r"\s*\(#\d+\)\s*$")

RULE_COMMIT_EMPTY = "COMMIT-EMPTY"
RULE_COMMIT_FORMAT = "COMMIT-FORMAT"
RULE_COMMIT_SUBJECT = "COMMIT-SUBJECT"
RULE_COMMIT_LENGTH = "COMMIT-LENGTH"
RULE_COMMIT_BLANK_LINE = "COMMIT-BLANK-LINE"
RULE_COMMIT_PROSE = "COMMIT-PROSE"

EMPTY_REMEDY = "write a conventional commit message: <type>[optional scope]: <subject>"
FORMAT_REMEDY = (
    "use <type>[optional scope]!: <subject>; allowed types:"
    " feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert;"
    " e.g. feat: 新增翻译浮层"
)
SUBJECT_REMEDY = "write the subject content right after the colon"
LENGTH_REMEDY = "trim the header and move details into the body"
BLANK_LINE_REMEDY = "separate the header and the body with a blank line"


def split_message(msg: str) -> list[str]:
    """预处理出待校验的行序列：剥注释行、去掉开头的空白行。"""
    msg = msg.rstrip("\n")
    if "\n" in msg:
        msg = "\n".join(line for line in msg.split("\n") if not line.startswith("#"))
    lines = msg.split("\n")
    first = 0
    while first < len(lines) and not lines[first].strip():
        first += 1
    return lines[first:]


def simulate_squash(title: str) -> str:
    """压缩合并模拟消息：标题剥首尾空白后追加编号后缀，即托管平台合并产物
    的最坏形态；拼装单点在本模块，四处调用方不再各抄一份占位。"""
    return "%s (#99999)" % title.strip()


def prose_findings(lines: list[str]) -> list[gate_report.Finding]:
    """主题与正文逐行过书写规则；结构性要素豁免——编号后缀、版本库尾注与
    压缩合并清单条目的类型前缀；书写规则条目统一归入书写纪律标识。"""
    found = []
    subject = ISSUE_SUFFIX_RE.sub("", SUBJECT_RE.sub("", lines[0], count=1))
    found += prose_line_problems("commit subject", 0, subject)
    for index, line in enumerate(lines[1:], 2):
        stripped = line.strip()
        if not stripped or TRAILER_RE.match(stripped):
            continue
        entry = LIST_ENTRY_RE.sub("", stripped, count=1)
        found += prose_line_problems("commit body line %d" % index, 0, entry)
    return [replace(f, rule=RULE_COMMIT_PROSE) for f in found]


def findings(msg: str) -> list[gate_report.Finding]:
    """校验一条提交信息，返回带规则标识的违规条目；判定顺序即规范本体——
    空、格式、主题、长度、空行逐关放行，书写规则殿后收全部条目。"""
    lines = split_message(msg)
    if not lines:
        return [
            gate_report.Finding(
                rule=RULE_COMMIT_EMPTY,
                path="",
                line=0,
                message="commit message is empty",
                remedy=EMPTY_REMEDY,
            )
        ]

    header = lines[0]
    if not HEADER_RE.match(header):
        return [
            gate_report.Finding(
                rule=RULE_COMMIT_FORMAT,
                path="commit subject",
                line=0,
                message="commit message does not follow Conventional Commits",
                evidence=header,
                remedy=FORMAT_REMEDY,
            )
        ]

    subject = SUBJECT_RE.sub("", header, count=1)
    if not subject:
        return [
            gate_report.Finding(
                rule=RULE_COMMIT_SUBJECT,
                path="commit subject",
                line=0,
                message="commit header is missing a subject (content expected after the colon)",
                evidence=header,
                remedy=SUBJECT_REMEDY,
            )
        ]

    header_bytes = len(header.encode("utf-8"))
    if header_bytes > MAX_HEADER_BYTES:
        return [
            gate_report.Finding(
                rule=RULE_COMMIT_LENGTH,
                path="commit subject",
                line=0,
                message="commit header too long (%d bytes, limit %d)"
                % (header_bytes, MAX_HEADER_BYTES),
                evidence=header,
                remedy=LENGTH_REMEDY,
            )
        ]

    if len(lines) > 1 and lines[1] != "":
        return [
            gate_report.Finding(
                rule=RULE_COMMIT_BLANK_LINE,
                path="commit body line 2",
                line=0,
                message="commit header must be followed by a blank line before the body",
                evidence=lines[1],
                remedy=BLANK_LINE_REMEDY,
            )
        ]

    return prose_findings(lines)


def run(msg: str) -> list[str]:
    """校验一条 commit message，返回违规列表（渲染行，消息带位置）；通过返回
    空列表。"""
    return [gate_report.render(f) for f in findings(msg)]


def main(argv: list[str]) -> int:
    """命令行入口：逐条校验参数里的全部消息，任一违规即整体退出非零；合并
    模拟形态打印标题的压缩合并模拟消息。"""
    if len(argv) >= 2 and argv[1] == "--simulate-squash":
        if len(argv) != 3:
            print("usage: %s --simulate-squash <title>" % argv[0], file=sys.stderr)
            return gate_report.EXIT_USAGE
        print(simulate_squash(argv[2]))
        return gate_report.EXIT_PASS
    if len(argv) < 2:
        print("usage: %s <commit-message-file|-> ..." % argv[0], file=sys.stderr)
        return gate_report.EXIT_USAGE
    all_findings = []
    for path in argv[1:]:
        if path == "-":
            msg = sys.stdin.read()
        else:
            with open(path, encoding="utf-8", errors="replace") as fh:
                msg = fh.read()
        found = findings(msg)
        if found:
            for finding in found:
                gate_report.fail(finding)
            all_findings += found
        else:
            print("ok: commit message: %s" % split_message(msg)[0])
    if all_findings:
        gate_report.summary(all_findings)
        return gate_report.EXIT_VIOLATION
    return gate_report.EXIT_PASS


if __name__ == "__main__":
    sys.exit(main(sys.argv))
