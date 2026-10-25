#!/usr/bin/env python3
"""注释纪律门禁·文档：标记语言（Markdown）文档逐行受检——自述、说明、安
全政策与测试清单；豁免面与判罚由语言规范库的代码引用规则承载，违规以带规
则标识的条目返回。受检面策略：注释记录目录是注释的住处、提示词目录是模型
功能资产，都不在受检面。由注释门禁路由调用，与语言规范库同源共用；本模块
自身注释同规。
"""

from __future__ import annotations

import os
import sys

_HOOKS = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if _HOOKS not in sys.path:
    sys.path.insert(0, _HOOKS)

import gate_report  # noqa: E402
from language_spec.coderef import markdown_problems  # noqa: E402


def handles(rel: str) -> bool:
    """文件类型是否归本门禁：标记语言文档，记录目录与提示词目录除外。"""
    if os.path.splitext(os.path.basename(rel))[1] != ".md":
        return False
    if rel.startswith("docs" + os.sep + "comments" + os.sep):
        return False
    return "prompts" not in rel.split(os.sep)


def check(rel: str, src: str) -> list[gate_report.Finding]:
    """文档逐行受检，判定走语言规范库的标记语言遍历。"""
    return markdown_problems(rel, src)
