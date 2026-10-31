#!/usr/bin/env python3
"""拉取请求内容门禁：标题与正文过书写纪律公共规则，正文并核对模板必需小节。
与注释门禁、提交信息门禁共用同一套判罚；本地编码助手钩子与持续集成共用此
脚本，保证多处判定一致。

标题做合并模拟校验——压缩合并的提交主题取自标题并追加编号后缀，拼装经提
交信息门禁的合并模拟单点，按提交信息规范全量判定，标题自身合规而合并后
超限同样拦下。正文按标记语言逐行受检（围栏、行内反引号与链接等结构性记
号豁免），模板必需小节直接从模板文件提取，缺哪个报哪个；模板文件缺失时
跳过小节核对，不误伤。

校验结果以带规则标识的违规条目列表承载，报告经统一诊断出口打印；退出码
按全脚本统一约定：零为通过、一为发现违规、二为用法错误。

用法：python3 scripts/hooks/check_pr_prose.py <body-file|- [title-file]>，正文路径传
减号读标准输入；本地手动测试时向文件或标准输入喂入内容。
"""

from __future__ import annotations

import os
import re
import sys
from dataclasses import replace

import check_commit_msg
import gate_report
from language_spec.coderef import markdown_problems

# 模板必需小节清单的来源；小节指二级标题
TEMPLATE_REL = os.path.join(".github", "pull_request_template.md")
SECTION_RE = re.compile(r"^## (.+)$")

RULE_PR_SECTION = "PR-SECTION"
RULE_PR_PROSE = "PR-PROSE"
RULE_PR_TITLE_SIM = "PR-TITLE-SIM"

SECTION_REMEDY = "add the missing section, copying the heading from the pull request template"
TITLE_SIM_REMEDY = "shorten or reword the title so the squash-merge commit message passes"


def template_sections(root: str) -> list[str]:
    """从模板文件提取必需小节标题；模板缺失返回空表，跳过小节核对。"""
    path = os.path.join(root, TEMPLATE_REL)
    if not os.path.isfile(path):
        return []
    out = []
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            m = SECTION_RE.match(line.rstrip("\n"))
            if m:
                out.append(m.group(1).strip())
    return out


def section_findings(sections: list[str], body: str) -> list[gate_report.Finding]:
    return [
        gate_report.Finding(
            rule=RULE_PR_SECTION,
            path="",
            line=0,
            message="pull request body misses required template section: %s" % s,
            remedy=SECTION_REMEDY,
        )
        for s in sections
        if s not in body
    ]


def body_findings(body: str) -> list[gate_report.Finding]:
    """正文书写规则：硬违规统一归入正文标识，软约束候选保留原标识与警告属
    性走警告通道。"""
    return [
        f if f.advisory else replace(f, rule=RULE_PR_PROSE)
        for f in markdown_problems("pull request body", body)
    ]


def title_findings(title: str) -> list[gate_report.Finding]:
    out = []
    for f in check_commit_msg.findings(check_commit_msg.simulate_squash(title)):
        if f.advisory:
            out.append(
                replace(
                    f,
                    message="pull request title fails the squash-merge simulation: %s" % f.message,
                )
            )
            continue
        out.append(
            replace(
                f,
                rule=RULE_PR_TITLE_SIM,
                message="pull request title fails the squash-merge simulation: %s" % f.message,
                remedy=TITLE_SIM_REMEDY,
            )
        )
    return out


def findings(root: str, body: str, title: str | None = None) -> list[gate_report.Finding]:
    """正文模板小节、正文书写规则与标题合并模拟三合一，返回带规则标识的
    违规条目。"""
    out = section_findings(template_sections(root), body)
    out += body_findings(body)
    if title:
        out += title_findings(title)
    return out


def run(root: str, body: str, title: str | None = None) -> list[str]:
    """正文书写规则、模板小节与标题合并模拟三合一，返回违规列表，软约束候
    选不计入。"""
    return [gate_report.render(f) for f in findings(root, body, title) if not f.advisory]


def main() -> int:
    root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    if len(sys.argv) < 2:
        print("usage: %s <body-file|-> [title-file]" % sys.argv[0], file=sys.stderr)
        return gate_report.EXIT_USAGE

    def read(path: str) -> str:
        if path == "-":
            return sys.stdin.read()
        with open(path, encoding="utf-8", errors="replace") as fh:
            return fh.read()

    body = read(sys.argv[1])
    title = read(sys.argv[2]) if len(sys.argv) > 2 else None
    found = findings(root, body, title)
    advisories = [f for f in found if f.advisory]
    found = [f for f in found if not f.advisory]
    for finding in advisories:
        gate_report.warn(finding)
    if advisories:
        gate_report.warning_summary(advisories)
    for finding in found:
        gate_report.fail(finding)
    if found:
        gate_report.summary(found)
        return gate_report.EXIT_VIOLATION
    print("  ok: pull request content: prose rules and template sections")
    return gate_report.EXIT_PASS


if __name__ == "__main__":
    sys.exit(main())
