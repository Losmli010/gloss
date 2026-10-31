"""评审结果模板门禁的单测：五段骨架、声明同源与失败关闭逐条覆盖。

被测入口是门禁的发现函数，夹具建在测试框架（pytest）的临时目录：两份模板
以最小形态组装同源基线，逐用例删改触发单一规则；正文模板缺失是无法完成判
定的前置故障，按基础设施条目判罚；评审结果模板整体缺失是失败关闭，绝不因
没检查到而放行。
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import check_review_template  # noqa: E402

PR_TEMPLATE = """## 摘要

## 测试（必填）

## 安全与隐私（必填）

## 兼容性（必填）

## 文档同步（必填）

## 回滚（必填）
"""

REVIEW_TEMPLATE = """## 摘要

## 判定

## 评审发现

## 声明核对（必填）

- **测试：** 成立
- **安全与隐私：** 成立
- **兼容性：** 成立
- **文档同步：** 成立
- **回滚：** 成立

## 结论
"""


def build_fixture(tmp_path: Path) -> None:
    manual = tmp_path / "docs" / "manual"
    manual.mkdir(parents=True)
    (manual / "pull_request_template.md").write_text(PR_TEMPLATE, encoding="utf-8")
    (manual / "review_result_template.md").write_text(REVIEW_TEMPLATE, encoding="utf-8")


def rules(tmp_path: Path) -> list[str]:
    return [f.rule for f in check_review_template.findings(str(tmp_path))]


def test_synced_templates_pass(tmp_path: Path) -> None:
    build_fixture(tmp_path)
    assert check_review_template.findings(str(tmp_path)) == []


def test_missing_review_template_fails_closed(tmp_path: Path) -> None:
    build_fixture(tmp_path)
    (tmp_path / "docs" / "manual" / "review_result_template.md").unlink()
    assert rules(tmp_path) == ["REVIEW-SECTION"]


def test_missing_section_flagged(tmp_path: Path) -> None:
    build_fixture(tmp_path)
    path = tmp_path / "docs" / "manual" / "review_result_template.md"
    path.write_text(REVIEW_TEMPLATE.replace("## 判定\n\n", ""), encoding="utf-8")
    assert rules(tmp_path) == ["REVIEW-SECTION"]


def test_missing_declaration_flagged(tmp_path: Path) -> None:
    build_fixture(tmp_path)
    path = tmp_path / "docs" / "manual" / "review_result_template.md"
    path.write_text(REVIEW_TEMPLATE.replace("- **测试：** 成立\n", ""), encoding="utf-8")
    assert rules(tmp_path) == ["REVIEW-DECL"]


def test_unknown_declaration_flagged(tmp_path: Path) -> None:
    build_fixture(tmp_path)
    path = tmp_path / "docs" / "manual" / "review_result_template.md"
    path.write_text(
        REVIEW_TEMPLATE.replace("## 结论", "- **新增声明：** 成立\n\n## 结论"),
        encoding="utf-8",
    )
    assert rules(tmp_path) == ["REVIEW-DECL"]


def test_missing_pull_request_template_is_infra(tmp_path: Path) -> None:
    build_fixture(tmp_path)
    (tmp_path / "docs" / "manual" / "pull_request_template.md").unlink()
    assert rules(tmp_path) == ["INFRA"]


def test_unreadable_review_template_is_infra(tmp_path: Path) -> None:
    build_fixture(tmp_path)
    path = tmp_path / "docs" / "manual" / "review_result_template.md"
    path.chmod(0o000)
    try:
        assert rules(tmp_path) == ["INFRA"]
    finally:
        path.chmod(0o644)


def test_unreadable_pull_request_template_is_infra(tmp_path: Path) -> None:
    build_fixture(tmp_path)
    path = tmp_path / "docs" / "manual" / "pull_request_template.md"
    path.chmod(0o000)
    try:
        assert rules(tmp_path) == ["INFRA"]
    finally:
        path.chmod(0o644)


def test_non_utf8_template_does_not_crash(tmp_path: Path) -> None:
    build_fixture(tmp_path)
    (tmp_path / "docs" / "manual" / "review_result_template.md").write_bytes(b"\xa3\xa3")
    found = check_review_template.findings(str(tmp_path))
    assert {f.rule for f in found} <= {"REVIEW-SECTION", "REVIEW-DECL"}
    assert found
