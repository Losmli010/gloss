#!/usr/bin/env python3
"""注释纪律门禁·文档：标记语言（Markdown）文档逐行受检——自述、说明、安
全政策、测试清单与提示词模板；豁免面与行级判罚由语言规范库的代码引用规则
承载；文档的结构判罚（标题层级、重名标题、围栏闭合）移交排版门禁工具
（rumdl），本门禁只管书写纪律。违规以带规则标识的条目返回。受检面策略：
注释记录目录是注释的住处，不在受检面；提示词模板入受检面，英文模板目录
的行文按路径段分派英文区判据。由注释门禁路由调用，与语言规范库同源共用；
本模块自身注释同规。
"""

from __future__ import annotations

import os
import sys

_HOOKS = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if _HOOKS not in sys.path:
    sys.path.insert(0, _HOOKS)

import gate_report  # noqa: E402
from language_spec.coderef import ZONE_CHINESE, ZONE_ENGLISH, markdown_problems  # noqa: E402


def handles(rel: str) -> bool:
    """文件类型是否归本门禁：标记语言文档，记录目录除外。"""
    if os.path.splitext(os.path.basename(rel))[1] != ".md":
        return False
    return not rel.startswith("docs" + os.sep + "comments" + os.sep)


def prose_zone(rel: str) -> str:
    """文档行文的表述区归属：提示词目录下的英文模板目录按路径段分派英文
    区判据，其余标记语言文档落中文区主体判定；区归属由本门禁把关，判据在
    语言规范库。"""
    parts = rel.split(os.sep)
    if "prompts" in parts:
        index = parts.index("prompts")
        if index + 1 < len(parts) and parts[index + 1] == "en":
            return ZONE_ENGLISH
    return ZONE_CHINESE


def check(rel: str, src: str) -> list[gate_report.Finding]:
    """文档逐行受检，判定走语言规范库的标记语言遍历入口，区归属随路径分派。"""
    return markdown_problems(rel, src, prose_zone(rel))
