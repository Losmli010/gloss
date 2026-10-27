#!/usr/bin/env python3
"""注释纪律门禁·记录名：注释记录目录的命名判定。记录名由人工或助手会话
凭源文件起名，口头约定对并发会话不可靠，本门禁机器化三道检查：名字合法性
（裸名不含点；伪装后缀按解析判定，细则住映射助手与登记表）、陈旧（裸名解析不回
任何真实源文件，判罚删除）与撞名（跨文件同名未按语义家族前缀登记消歧）。
映射规则单一事实源住命名助手模块，本门禁与命名助手共用同一函数；由注释门
禁路由调用，本地提交前门禁与持续集成共用，保证判定一致；本模块自身注释同
规。判罚细则与豁免方式住登记表记录文件。

用法：经注释门禁路由调用；模块检查入口接版本库跟踪清单（仓库相对路径），
返回违规条目。
"""

from __future__ import annotations

import os
import sys

_HOOKS = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
_DEV = os.path.abspath(os.path.join(_HOOKS, "..", "dev"))
for _entry in (_HOOKS, _DEV):
    if _entry not in sys.path:
        sys.path.insert(0, _entry)

import comment_map  # noqa: E402
import gate_report  # noqa: E402

RULE_RECORD_NAME = "COMMENT-RECORD-NAME"
RULE_RECORD_STALE = "COMMENT-RECORD-STALE"
RULE_RECORD_COLLISION = "COMMENT-RECORD-COLLISION"

NAME_MESSAGE = "record file name is not a canonical <stem>_comment.md name (suffix residue?)"
NAME_REMEDY = "rename with git mv to the canonical name; ask just comment-name <source> for it"
STALE_MESSAGE = "record resolves to no tracked source file (source renamed or removed)"
STALE_REMEDY = (
    "delete the stale record in the same PR (git rm); history keeps the content now"
    " that the directory is tracked"
)
COLLISION_REMEDY = (
    "register a semantic family prefix for every member in FAMILY_STEMS"
    " (scripts/dev/comment_map.py), then git mv the records to the prefixed names"
)


def name_findings(
    rel: str, bare: str | None, index: dict[str, list[str]]
) -> list[gate_report.Finding]:
    """单条记录的名字判定：非规范形态与伪装后缀残留各出一类条目。"""
    if (
        bare is None
        or not comment_map.is_legal_bare_name(bare)
        or comment_map.residue_stem(bare, index)
    ):
        return [
            gate_report.Finding(
                rule=RULE_RECORD_NAME,
                path=rel,
                line=0,
                message=NAME_MESSAGE,
                remedy=NAME_REMEDY,
            )
        ]
    return []


def check_all(tracked: list[str]) -> list[gate_report.Finding]:
    """记录命名全量判定：映射逻辑住命名助手模块，本入口只做组装与判罚。"""
    found: list[gate_report.Finding] = []
    index = comment_map.build_index(tracked)
    for rel in comment_map.record_paths(tracked):
        bare = comment_map.split_record_name(os.path.basename(rel))
        name_problems = name_findings(rel, bare, index)
        found.extend(name_problems)
        # 名字判罚在场时陈旧判定跳过：同一条记录不双报
        if (
            not name_problems
            and bare is not None
            and bare not in index
            and bare not in comment_map.FREE_STANDING_RECORDS
        ):
            found.append(
                gate_report.Finding(
                    rule=RULE_RECORD_STALE,
                    path=rel,
                    line=0,
                    message=STALE_MESSAGE,
                    remedy=STALE_REMEDY,
                )
            )
    for stem, members in comment_map.unregistered_collisions(tracked):
        found.append(
            gate_report.Finding(
                rule=RULE_RECORD_COLLISION,
                path="",
                line=0,
                message="source stem %r is shared by %d files without family prefixes: %s"
                % (stem, len(members), ", ".join(sorted(members))),
                remedy=COLLISION_REMEDY,
            )
        )
    for stem in comment_map.free_standing_shadowing(tracked):
        found.append(
            gate_report.Finding(
                rule=RULE_RECORD_COLLISION,
                path="",
                line=0,
                message="free-standing record name %r is also a source stem" % stem,
                remedy=COLLISION_REMEDY,
            )
        )
    return found
