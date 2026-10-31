#!/usr/bin/env python3
"""评审行内评论门禁：评论正文过书写纪律公共规则。评审人对拉取请求（PR）的
单条评论与评审附带的行内评论是评审交流的产物，正文与拉取请求正文、评审结果
同受书写纪律约束；判定面只有书写规则一项——单条评论没有模板骨架，小节与声
明判定不套，与评审结果门禁的差异在此。机器人评论与空评论由调用方（钩子工作
流的作业条件）排除，本脚本不做第二份排除判定，空正文进判定面即无违规放行，
排除语义单点在调用方，两处口径不会漂移。书写规则复用语言规范库的标记语言遍
历，与拉取请求正文同一口径；标识沿用登记表内的书写规则标识，不另设。作业定
位红牌：不进必需检查清单——红灯靠人看到后修复，不拦合并。

校验结果以带规则标识的违规条目列表承载，报告经统一诊断出口打印；退出码
按全脚本统一约定：零为通过、一为发现违规、二为用法错误。

用法：python3 scripts/hooks/check_review_comment.py <body-file|->，正文路径传
减号读标准输入；本地手动测试时向文件或标准输入喂入内容。
"""

from __future__ import annotations

import sys

import gate_report
from language_spec.coderef import markdown_problems


def findings(body: str) -> list[gate_report.Finding]:
    """评论正文书写规则：与拉取请求正文同一遍历，标识沿用登记表内的书写规
    则标识，软约束候选保留原标识与警告属性。"""
    return list(markdown_problems("review comment", body))


def run(body: str) -> list[str]:
    """评论正文书写规则的运行函数，返回违规列表，软约束候选不计入。"""
    return [gate_report.render(f) for f in findings(body) if not f.advisory]


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: %s <body-file|->" % sys.argv[0], file=sys.stderr)
        return gate_report.EXIT_USAGE

    def read(path: str) -> str:
        if path == "-":
            return sys.stdin.read()
        with open(path, encoding="utf-8", errors="replace") as fh:
            return fh.read()

    body = read(sys.argv[1])
    found = findings(body)
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
    print("  ok: review comment: prose rules")
    return gate_report.EXIT_PASS


if __name__ == "__main__":
    sys.exit(main())
