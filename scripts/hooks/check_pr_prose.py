#!/usr/bin/env python3
"""拉取请求内容门禁：标题与正文过书写纪律公共规则，正文并核对模板必需小节。
与注释门禁、提交信息门禁共用同一套判罚；本地编码助手钩子与持续集成共用此
脚本，保证多处判定一致。

标题做合并模拟校验——压缩合并的提交主题取自标题并追加编号后缀，按提交信
息规范全量判定，标题自身合规而合并后超限同样拦下。正文按标记语言逐行受检
（围栏、行内反引号与链接等结构性记号豁免），模板必需小节直接从模板文件提
取，缺哪个报哪个；模板文件缺失时跳过小节核对，不误伤。

用法：以解释器运行本脚本，首参为正文文件路径（减号读标准输入），次参可选
为标题文件路径；本地手动测试时向文件或标准输入喂入内容。
"""

from __future__ import annotations

import os
import re
import sys

import check_commit_msg
import gate_report
from language_spec.coderef import markdown_problems

# 模板必需小节清单的来源；小节指二级标题
TEMPLATE_REL = os.path.join(".github", "pull_request_template.md")
SECTION_RE = re.compile(r"^## (.+)$")


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


def section_problems(sections: list[str], body: str) -> list[str]:
    return [
        "pull request body misses required template section: %s" % s
        for s in sections
        if s not in body
    ]


def run(root: str, body: str, title: str | None = None) -> list[str]:
    """正文书写规则、模板小节与标题合并模拟三合一，返回违规列表。"""
    problems = []
    problems += section_problems(template_sections(root), body)
    problems += [gate_report.render(f) for f in markdown_problems("pull request body", body)]
    if title:
        simulated = check_commit_msg.run("%s (#99999)" % title.strip())
        if simulated:
            problems.append("pull request title fails the squash-merge simulation:\n%s" % simulated)
    return problems


def main() -> int:
    root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    if len(sys.argv) < 2:
        print("usage: %s <body-file|-> [title-file]" % sys.argv[0], file=sys.stderr)
        return 2

    def read(path: str) -> str:
        if path == "-":
            return sys.stdin.read()
        with open(path, encoding="utf-8", errors="replace") as fh:
            return fh.read()

    body = read(sys.argv[1])
    title = read(sys.argv[2]) if len(sys.argv) > 2 else None
    problems = run(root, body, title)
    if problems:
        for p in problems:
            print("  ✗ %s" % p, file=sys.stderr)
        print("✗ pull request content check: %d problems" % len(problems), file=sys.stderr)
        return 1
    print("  ok pull request content: prose rules and template sections")
    return 0


if __name__ == "__main__":
    sys.exit(main())
