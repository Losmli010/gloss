"""评审结果提交门禁的单测：空正文分支、五段骨架、声明同源与书写规则逐条
覆盖。

被测入口是门禁的发现函数与运行函数，返回违规列表，不落进程；正文模板写入
测试框架（pytest）的临时目录作同源基线，夹具正文按书写规则白名单组句；正
文模板缺失是声明核对无法完成判定的前置故障，按基础设施条目判罚；空正文与
纯空白正文并入同一条判罚；另含反馈契约：子进程端到端锁退出码。
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import check_review_result  # noqa: E402

SCRIPT = Path(__file__).parent / "check_review_result.py"

PR_TEMPLATE = """## 测试（必填）

## 安全与隐私（必填）

## 兼容性（必填）

## 文档同步（必填）

## 回滚（必填）
"""

CONFORMING = """## 摘要

- **评审对象：** 拉取请求（PR）测试编号，标题略
- **基准分支：** 主分支
- **门禁状态：** 通过
- **亮点：** 状态机不变量测试钉得扎实

## 判定

通过，机器门禁全绿且声明核对成立。

## 评审发现

无，不凑数。

## 声明核对（必填）

- **测试：** 成立
- **安全与隐私：** 成立
- **兼容性：** 成立
- **文档同步：** 成立
- **回滚：** 成立

## 结论

- **值得肯定：** 测试与文档同步完整
- **阻塞项：** 无
- **给作者的动作：** 无
- **提交前自证：** 已逐条复核
"""


def make_root(tmp_path: Path) -> str:
    (tmp_path / "docs" / "manual").mkdir(parents=True, exist_ok=True)
    (tmp_path / "docs" / "manual" / "pull_request_template.md").write_text(
        PR_TEMPLATE, encoding="utf-8"
    )
    return str(tmp_path)


def rules(root: str, body: str) -> list[str]:
    return [f.rule for f in check_review_result.findings(root, body)]


# ---- 空正文分支 ----


def test_empty_body_flags_review_empty_only(tmp_path: Path) -> None:
    found = check_review_result.findings(make_root(tmp_path), "")
    assert [f.rule for f in found] == ["REVIEW-EMPTY"]


def test_whitespace_only_body_flags_review_empty(tmp_path: Path) -> None:
    found = check_review_result.findings(make_root(tmp_path), " \n\t\n　\n")
    assert [f.rule for f in found] == ["REVIEW-EMPTY"]


def test_empty_body_remedy_points_to_writing_the_result(tmp_path: Path) -> None:
    (finding,) = check_review_result.findings(make_root(tmp_path), "")
    assert finding.remedy == check_review_result.REVIEW_EMPTY_REMEDY


# ---- 五段骨架 ----


def test_conforming_submission_passes(tmp_path: Path) -> None:
    assert check_review_result.findings(make_root(tmp_path), CONFORMING) == []


def test_missing_section_flagged(tmp_path: Path) -> None:
    body = CONFORMING.replace("## 判定\n\n通过，机器门禁全绿且声明核对成立。\n\n", "")
    assert rules(make_root(tmp_path), body) == ["REVIEW-SECTION"]


# ---- 声明核对同源 ----


def test_missing_declaration_flagged(tmp_path: Path) -> None:
    body = CONFORMING.replace("- **回滚：** 成立\n", "")
    assert rules(make_root(tmp_path), body) == ["REVIEW-DECL"]


def test_unknown_declaration_flagged(tmp_path: Path) -> None:
    body = CONFORMING.replace("## 结论", "- **新增声明：** 成立\n\n## 结论")
    assert rules(make_root(tmp_path), body) == ["REVIEW-DECL"]


def test_declaration_outside_checklist_block_ignored(tmp_path: Path) -> None:
    body = CONFORMING.replace("## 评审发现", "## 评审发现\n\n- **回滚：** 成立\n")
    assert rules(make_root(tmp_path), body) == []


def test_missing_pull_request_template_is_infra(tmp_path: Path) -> None:
    root = make_root(tmp_path)
    (tmp_path / "docs" / "manual" / "pull_request_template.md").unlink()
    assert rules(root, CONFORMING) == ["INFRA"]


# ---- 书写规则 ----


def test_prose_violation_flagged(tmp_path: Path) -> None:
    body = CONFORMING.replace("通过，机器门禁全绿且声明核对成立。", "This is an English sentence.")
    found = check_review_result.findings(make_root(tmp_path), body)
    assert any(f.rule.startswith("PROSE-") for f in found)


# ---- 运行函数与反馈契约 ----


def test_run_returns_rendered_strings(tmp_path: Path) -> None:
    body = CONFORMING.replace("## 判定\n\n通过，机器门禁全绿且声明核对成立。\n\n", "")
    problems = check_review_result.run(make_root(tmp_path), body)
    assert len(problems) == 1
    assert "misses required section" in problems[0]


def test_run_flags_empty_body_as_single_violation(tmp_path: Path) -> None:
    problems = check_review_result.run(make_root(tmp_path), "")
    assert len(problems) == 1
    assert "empty or whitespace only" in problems[0]


# 命令行用例对照仓库根的真实正文模板判定（被测脚本的仓库根取自脚本位置），
# 兼作夹具与真实模板的同源哨兵
def test_cli_exit_codes(tmp_path: Path) -> None:
    good = tmp_path / "good.md"
    good.write_text(CONFORMING, encoding="utf-8")
    passed = subprocess.run([sys.executable, str(SCRIPT), str(good)], capture_output=True)
    assert passed.returncode == 0

    bad = tmp_path / "bad.md"
    bad.write_text("## 摘要\n\n缺少骨架的正文\n", encoding="utf-8")
    violated = subprocess.run([sys.executable, str(SCRIPT), str(bad)], capture_output=True)
    assert violated.returncode == 1
    assert b"REVIEW-SECTION" in violated.stderr

    blank = tmp_path / "blank.md"
    blank.write_text(" \n\t\n", encoding="utf-8")
    empty = subprocess.run([sys.executable, str(SCRIPT), str(blank)], capture_output=True)
    assert empty.returncode == 1
    assert b"REVIEW-EMPTY" in empty.stderr

    usage = subprocess.run([sys.executable, str(SCRIPT)], capture_output=True)
    assert usage.returncode == 2
