#!/usr/bin/env python3
"""评审结果提交门禁：评审结果正文过评审模板骨架、声明核对与书写规则三重判
定。评审人对拉取请求（PR）提交评审时，评审结果是评审人的正式产物，与正文
模板承载的作者声明配对；骨架小节与声明清单的解析复用模板文件门禁的同一批
单点，两处判定同源不漂移。骨架缺段或声明缺项即拦；书写规则复用语言规范库
的标记语言遍历，与拉取请求正文同一口径。正文模板缺失时声明核对无法同源，
按无法完成判定处置，绝不因没检查到而放行。

校验结果以带规则标识的违规条目列表承载，报告经统一诊断出口打印；退出码
按全脚本统一约定：零为通过、一为发现违规、二为用法错误、三为无法完成判定。

用法：python3 scripts/hooks/check_review_result.py <body-file|->，正文路径传
减号读标准输入；本地手动测试时向文件或标准输入喂入内容。
"""

from __future__ import annotations

import os
import sys

import check_review_template
import gate_report
from language_spec.coderef import markdown_problems

REVIEW_SECTION_REMEDY = (
    "add the missing section, copying the heading from the review result template"
)

REVIEW_DECL_REMEDY = (
    "cover every required declaration from the pull request template in the checklist"
)


def skeleton_findings(body: str) -> list[gate_report.Finding]:
    """五段骨架：必需小节的二级标题任一缺席即拦，与模板文件门禁同源。"""
    present = set(check_review_template.matched(body, check_review_template.SECTION_RE))
    return [
        gate_report.Finding(
            rule=check_review_template.RULE_REVIEW_SECTION,
            path="",
            line=0,
            message="review result misses required section: %s" % name,
            remedy=REVIEW_SECTION_REMEDY,
        )
        for name in check_review_template.REQUIRED_SECTIONS
        if name not in present
    ]


def declaration_findings(body: str, root: str) -> list[gate_report.Finding]:
    """声明核对：清单须与正文模板的必填声明同源，缺项与多项都拦；正文模板
    缺失按无法完成判定处置。"""
    pr = check_review_template.read_template(root, check_review_template.PR_TEMPLATE_REL)
    if pr is None:
        return [
            gate_report.infra_failure(
                "pull request template is missing, cannot sync-check the review result"
            )
        ]
    expected = check_review_template.matched(pr, check_review_template.PR_DECLARATION_RE)
    block = "\n".join(check_review_template.declaration_lines(body))
    actual = check_review_template.matched(block, check_review_template.REVIEW_DECLARATION_RE)
    out = [
        gate_report.Finding(
            rule=check_review_template.RULE_REVIEW_DECL,
            path="",
            line=0,
            message="review result declaration checklist misses declaration: %s" % name,
            remedy=REVIEW_DECL_REMEDY,
        )
        for name in expected
        if name not in actual
    ]
    out += [
        gate_report.Finding(
            rule=check_review_template.RULE_REVIEW_DECL,
            path="",
            line=0,
            message="review result declaration checklist carries unknown declaration: %s" % name,
            remedy=REVIEW_DECL_REMEDY,
        )
        for name in actual
        if name not in expected
    ]
    return out


def prose_findings(body: str) -> list[gate_report.Finding]:
    """书写规则：与拉取请求正文同一遍历，标识沿用语言规范库的登记。"""
    return list(markdown_problems("review result", body))


def findings(root: str, body: str) -> list[gate_report.Finding]:
    """骨架、声明核对与书写规则三合一，返回带规则标识的违规条目。"""
    return skeleton_findings(body) + declaration_findings(body, root) + prose_findings(body)


def run(root: str, body: str) -> list[str]:
    """评审结果三重判定的运行函数，返回违规列表（软约束候选不计入），供客
    户端钩子复用。"""
    return [gate_report.render(f) for f in findings(root, body) if not f.advisory]


def main() -> int:
    root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    if len(sys.argv) < 2:
        print("usage: %s <body-file|->" % sys.argv[0], file=sys.stderr)
        return gate_report.EXIT_USAGE

    def read(path: str) -> str:
        if path == "-":
            return sys.stdin.read()
        with open(path, encoding="utf-8", errors="replace") as fh:
            return fh.read()

    body = read(sys.argv[1])
    found = findings(root, body)
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
        if any(f.rule == gate_report.RULE_INFRA for f in found):
            return gate_report.EXIT_INFRA
        return gate_report.EXIT_VIOLATION
    print("  ok: review result: skeleton, declaration sync and prose rules")
    return gate_report.EXIT_PASS


if __name__ == "__main__":
    sys.exit(main())
