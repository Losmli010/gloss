#!/usr/bin/env python3
"""评审结果模板门禁：评审结果模板的五段骨架在场，声明核对与正文模板的必需
小节同源。评审人对拉取请求提交评审结果时，声明核对按正文模板的必填声明逐
项核对；两份模板各自演化而声明清单失同步时，评审人按过期清单核对即漏项，
故由门禁锁定同源。评审结果模板缺失按失败关闭处置，绝不因没检查到而放行。

校验结果以带规则标识的违规条目列表承载，报告经统一诊断出口打印；退出码
按全脚本统一约定：零为通过、一为发现违规、二为用法错误、三为无法完成判定。

用法：python3 scripts/hooks/check_review_template.py，无参数；仓库根取本脚本位置向上回溯。
"""

from __future__ import annotations

import os
import re
import sys

import gate_report

REVIEW_TEMPLATE_REL = os.path.join(".github", "review_result_template.md")
PR_TEMPLATE_REL = os.path.join(".github", "pull_request_template.md")

RULE_REVIEW_SECTION = "REVIEW-SECTION"
RULE_REVIEW_DECL = "REVIEW-DECL"

SECTION_REMEDY = (
    "restore the section heading, keeping the five-part skeleton of the review result template"
)
DECL_REMEDY = (
    "keep the declaration checklist in sync with the required sections of the pull request template"
)
MISSING_REMEDY = "restore the review result template; the review flow depends on it"

# 评审结果模板的五段骨架；小节指二级标题
REQUIRED_SECTIONS = ("摘要", "判定", "评审发现", "声明核对（必填）", "结论")
SECTION_RE = re.compile(r"^## (.+)$")
# 正文模板的必填小节：标题带必填标记，声明名取标记前的部分
PR_DECLARATION_RE = re.compile(r"^## (.+?)（必填）$")
# 声明核对清单里的声明条目
REVIEW_DECLARATION_RE = re.compile(r"^- \*\*(.+?)：\*\*")


def read_template(root: str, rel: str) -> str | None:
    path = os.path.join(root, rel)
    if not os.path.isfile(path):
        return None
    with open(path, encoding="utf-8", errors="replace") as fh:
        return fh.read()


def matched(text: str, pattern: re.Pattern[str]) -> list[str]:
    out = []
    for line in text.split("\n"):
        m = pattern.match(line)
        if m:
            out.append(m.group(1))
    return out


def declaration_lines(text: str) -> list[str]:
    """声明核对小节里的行：小节起点到下一个二级标题为止。"""
    out = []
    in_block = False
    for line in text.split("\n"):
        if line.startswith("## "):
            in_block = line[3:].strip() == "声明核对（必填）"
            continue
        if in_block:
            out.append(line)
    return out


def findings(root: str) -> list[gate_report.Finding]:
    """五段骨架与声明同源的全部违规条目；模板不可读按无法完成判定处置。"""
    try:
        return _findings(root)
    except OSError as exc:
        return [gate_report.infra_failure("template unreadable: %s" % exc)]


def _findings(root: str) -> list[gate_report.Finding]:
    review = read_template(root, REVIEW_TEMPLATE_REL)
    if review is None:
        return [
            gate_report.Finding(
                rule=RULE_REVIEW_SECTION,
                path=REVIEW_TEMPLATE_REL,
                line=0,
                message="review result template is missing",
                remedy=MISSING_REMEDY,
            )
        ]
    present = set(matched(review, SECTION_RE))
    out = [
        gate_report.Finding(
            rule=RULE_REVIEW_SECTION,
            path=REVIEW_TEMPLATE_REL,
            line=0,
            message="review result template misses required section: %s" % name,
            remedy=SECTION_REMEDY,
        )
        for name in REQUIRED_SECTIONS
        if name not in present
    ]
    pr = read_template(root, PR_TEMPLATE_REL)
    if pr is None:
        out.append(
            gate_report.infra_failure(
                "pull request template is missing, cannot sync-check the review result template"
            )
        )
        return out
    expected = matched(pr, PR_DECLARATION_RE)
    actual = matched("\n".join(declaration_lines(review)), REVIEW_DECLARATION_RE)
    out += [
        gate_report.Finding(
            rule=RULE_REVIEW_DECL,
            path=REVIEW_TEMPLATE_REL,
            line=0,
            message="declaration checklist misses pull request declaration: %s" % name,
            remedy=DECL_REMEDY,
        )
        for name in expected
        if name not in actual
    ]
    out += [
        gate_report.Finding(
            rule=RULE_REVIEW_DECL,
            path=REVIEW_TEMPLATE_REL,
            line=0,
            message="declaration checklist carries unknown declaration: %s" % name,
            remedy=DECL_REMEDY,
        )
        for name in actual
        if name not in expected
    ]
    return out


def main() -> int:
    root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    found = findings(root)
    for finding in found:
        gate_report.fail(finding)
    if found:
        gate_report.summary(found)
        if any(finding.rule == gate_report.RULE_INFRA for finding in found):
            return gate_report.EXIT_INFRA
        return gate_report.EXIT_VIOLATION
    print("  ok: review result template: skeleton and declaration sync")
    return gate_report.EXIT_PASS


if __name__ == "__main__":
    sys.exit(main())
