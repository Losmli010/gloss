#!/usr/bin/env python3
"""语言规范·英文区：纯英文区域怎么写。区域内部出现汉字或全角字符即整段
违规；判罚以带规则标识的违规条目返回，位置由调用方按路径与行号传入，标识
登记于统一登记表。区域归属与写明不变量的要求由使用方把关。当前用于源码的
不变量注释，未来其它英文区复用；本模块自身注释同规。"""

from __future__ import annotations

import os
import sys

_HOOKS = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if _HOOKS not in sys.path:
    sys.path.insert(0, _HOOKS)

import gate_report  # noqa: E402

from language_spec.chinese import CJKWISE  # noqa: E402

RULE_PROSE_ENGLISH = "PROSE-ENGLISH"

ENGLISH_REMEDY = "rewrite the whole invariant block in English, keeping symbol references"


def english_zone_problems(path: str, line: int, text: str, label: str) -> list[gate_report.Finding]:
    """纯英文区判罚：出现汉字或全角字符即违规，其余交回调用方。"""
    if CJKWISE.search(text):
        return [
            gate_report.Finding(
                rule=RULE_PROSE_ENGLISH,
                path=path,
                line=line,
                message="%s must be pure English"
                " — rewrite the whole invariant block in English,"
                " keep symbol references" % label,
                evidence=text,
                remedy=ENGLISH_REMEDY,
            )
        ]
    return []
