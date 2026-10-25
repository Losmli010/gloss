#!/usr/bin/env python3
"""注释纪律门禁·文档：标记语言（Markdown）文档逐行加结构受检——自述、说
明、安全政策与测试清单；豁免面与行级判罚由语言规范库的代码引用规则承载，
标题跳级、同文档重名标题与结尾围栏不闭合三条结构判罚为文档子门禁自有，提
交信息与拉取请求内容门禁不共用；违规以带规则标识的条目返回。受检面策略：
注释记录目录是注释的住处、提示词目录是模型功能资产，都不在受检面。由注释
门禁路由调用，与语言规范库同源共用；本模块自身注释同规。
"""

from __future__ import annotations

import os
import sys

_HOOKS = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if _HOOKS not in sys.path:
    sys.path.insert(0, _HOOKS)

import gate_report  # noqa: E402
from language_spec.coderef import (  # noqa: E402
    markdown_problems,
    markdown_structure_problems,
)


def handles(rel: str) -> bool:
    """文件类型是否归本门禁：标记语言文档，记录目录与提示词目录除外。"""
    if os.path.splitext(os.path.basename(rel))[1] != ".md":
        return False
    if rel.startswith("docs" + os.sep + "comments" + os.sep):
        return False
    return "prompts" not in rel.split(os.sep)


def check(rel: str, src: str) -> list[gate_report.Finding]:
    """文档逐行与结构两遍受检，判定走语言规范库的标记语言两个入口。"""
    return markdown_problems(rel, src) + markdown_structure_problems(rel, src)
