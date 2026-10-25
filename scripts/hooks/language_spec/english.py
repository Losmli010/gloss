#!/usr/bin/env python3
"""语言规范·英文区：纯英文区域怎么写。区域内部出现汉字或全角字符即整段
违规；判罚不进记录文件，区域归属与写明不变量的要求由使用方把关。当前用
于源码的不变量注释，未来其它英文区复用；本模块自身注释同规。"""

from __future__ import annotations

from language_spec.chinese import CJKWISE


def english_zone_problems(loc: str, text: str, label: str) -> list[str]:
    """纯英文区判罚：出现汉字或全角字符即违规，其余交回调用方。"""
    if CJKWISE.search(text):
        return [
            f"{loc}: {label} must be pure English"
            " — rewrite the whole invariant block in English,"
            " keep symbol references"
        ]
    return []
