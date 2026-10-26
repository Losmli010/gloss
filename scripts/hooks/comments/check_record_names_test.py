"""记录名门禁的单测：名字合法性、陈旧判罚与撞名登记逐条覆盖。

被测入口接版本库跟踪清单（仓库相对路径），不落进程不读文件；判定逻辑住映
射助手模块，助手侧的映射单测另见其单测文件。"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import check_record_names  # noqa: E402

CLEAN_REPO = [
    "src/lib.rs",
    "docs/comments/lib_comment.md",
    "docs/comments/lib_tests_comment.md",
]


def rules(tracked: list[str]) -> set[str]:
    return {f.rule for f in check_record_names.check_all(tracked)}


def test_clean_pair_passes() -> None:
    assert rules(CLEAN_REPO) == set()


def test_free_standing_record_passes() -> None:
    tracked = CLEAN_REPO + ["docs/comments/gate_rules_comment.md"]
    assert rules(tracked) == set()


def test_suffix_residue_and_disguise_flagged() -> None:
    tracked = CLEAN_REPO + [
        "docs/comments/main.rs_comment.md",
        "docs/comments/clone_rs_comment.md",
    ]
    found = rules(tracked)
    assert found == {"COMMENT-RECORD-NAME"}


def test_non_canonical_suffix_flagged() -> None:
    tracked = CLEAN_REPO + ["docs/comments/notes.md"]
    assert rules(tracked) == {"COMMENT-RECORD-NAME"}


def test_stale_record_flagged() -> None:
    tracked = CLEAN_REPO + ["docs/comments/ghost_comment.md"]
    assert rules(tracked) == {"COMMENT-RECORD-STALE"}


def test_family_prefix_registration_keeps_colliding_stems_legal() -> None:
    tracked = [
        "src/main.rs",
        "web/assets/main.js",
        "docs/comments/main_comment.md",
        "docs/comments/web_main_comment.md",
    ]
    assert rules(tracked) == set()


def test_unregistered_collision_flagged() -> None:
    tracked = ["src/style.rs", "web/style.css", "docs/comments/style_comment.md"]
    assert rules(tracked) == {"COMMENT-RECORD-COLLISION"}


def test_free_standing_shadowing_flagged() -> None:
    tracked = CLEAN_REPO + ["docs/comments/waivers_comment.md", "src/waivers.rs"]
    assert rules(tracked) == {"COMMENT-RECORD-COLLISION"}
