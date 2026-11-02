#!/usr/bin/env python3
"""语言规范·分诊：三刀短路与区核心判据的参数化入口。按序判定，任一刀命中
即返回单例条目——反引号、章节引用、语言（无汉字，仅中文区设刀）——命中不
再继续，也不再跑核心判据；未命中落区核心：中文区主体走中文区表述判定，英
文区走英文区判定（语言判定住英文区核心，无独立刀）。三刀的规则标识、消息
与修复指引由调用方按面传入，条目形态与短路链由本入口统一：注释块、模块级
文档字符串、函数级文档字符串与界面文案表（i18n）词条四处调用共用同一形状，
改写存量时先清反引号与章节引用，两刀短路解除后字符级病灶才会在核心判据下
显形。判罚以带规则标识的违规条目返回，位置由调用方按路径与行号传入，标识
登记于统一登记表。被注释与文案表门禁复用；本模块自身注释同规。

"""

from __future__ import annotations

from dataclasses import dataclass

import gate_report

from language_spec.chinese import CJK, body_problems
from language_spec.coderef import ZONE_ENGLISH
from language_spec.english import english_zone_problems


@dataclass(frozen=True)
class TriageFace:
    """一个判罚面的三刀参数：刀 1、2 与刀 3 的规则标识、消息与修复指引；
    刀 3 仅中文区读（英文区语言判定住核心），英文区核心的指引单独传入。"""

    coderef_rule: str
    coderef_message: str
    coderef_remedy: str
    section_rule: str
    section_message: str
    section_remedy: str
    language_rule: str = ""
    language_message: str = ""
    language_remedy: str = ""
    english_remedy: str = ""


def triage_problems(
    path: str, line: int, text: str, label: str, zone: str, face: TriageFace
) -> list[gate_report.Finding]:
    """三刀短路后落区核心：任一刀命中即返回单例条目（含不再跑核心判据），
    反引号在场时字符级罚不并发上报；证据行压平换行，跨行文本形态与既有一
    致。"""
    evidence = text.replace("\n", " ")
    if "`" in text:
        return [
            gate_report.Finding(
                rule=face.coderef_rule,
                path=path,
                line=line,
                message=face.coderef_message,
                evidence=evidence,
                remedy=face.coderef_remedy,
            )
        ]
    if "§" in text:
        return [
            gate_report.Finding(
                rule=face.section_rule,
                path=path,
                line=line,
                message=face.section_message,
                evidence=evidence,
                remedy=face.section_remedy,
            )
        ]
    if zone == ZONE_ENGLISH:
        return english_zone_problems(path, line, text, label, face.english_remedy)
    if not CJK.search(text):
        return [
            gate_report.Finding(
                rule=face.language_rule,
                path=path,
                line=line,
                message=face.language_message,
                evidence=evidence,
                remedy=face.language_remedy,
            )
        ]
    return body_problems(path, line, text, label)
