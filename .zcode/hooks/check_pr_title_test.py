"""编码助手提交前钩子的单测：字段抽取、两份模板的路由与放行面逐条覆盖。

被测入口是抽取函数与主入口：接口工具直接取入参字段，命令行按外壳词法分词
后定位子命令与旗标；主入口从标准输入读钩子载荷，校验失败退出码二阻断，取
不到字段或载荷不相关放行。模板判定读仓库根的真实正文模板，评审骨架随门禁
常量；工程目录环境变量在导入前钉到仓库根。
"""

from __future__ import annotations

import io
import json
import os
import shlex
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).parents[2]
os.environ["ZCODE_PROJECT_DIR"] = str(REPO_ROOT)

sys.path.insert(0, str(Path(__file__).parent))

import check_pr_title  # noqa: E402

CONFORMING_REVIEW = """## 摘要

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


def bash_payload(command: str) -> str:
    return json.dumps({"tool_name": "Bash", "tool_input": {"command": command}})


def mcp_payload(tool: str, fields: dict) -> str:
    return json.dumps({"tool_name": tool, "tool_input": fields})


def run_main(payload: str, capsys) -> tuple[int, str]:
    old = sys.stdin
    sys.stdin = io.StringIO(payload)
    try:
        code = check_pr_title.main()
    finally:
        sys.stdin = old
    err = capsys.readouterr().err
    return code, err


# ---- 字段抽取 ----


def test_pr_tool_yields_title_and_body() -> None:
    data = json.loads(
        mcp_payload("mcp__github__create_pull_request", {"title": "fix: 修好", "body": "正文"})
    )
    assert check_pr_title.extract(data) == ("fix: 修好", "正文", "")


def test_review_tool_yields_review_body() -> None:
    data = json.loads(
        mcp_payload("mcp__github__create_and_submit_pull_request_review", {"body": "评审"})
    )
    assert check_pr_title.extract(data) == ("", "", "评审")


def test_review_tool_pending_submit_yields_review_body() -> None:
    data = json.loads(
        mcp_payload("mcp__github__submit_pending_pull_request_review", {"body": "评审"})
    )
    assert check_pr_title.extract(data) == ("", "", "评审")


def test_bash_create_yields_title_and_body() -> None:
    data = json.loads(bash_payload('gh pr create --title "fix: 修好" --body "正文"'))
    assert check_pr_title.extract(data) == ("fix: 修好", "正文", "")


def test_bash_edit_yields_title_and_body() -> None:
    data = json.loads(bash_payload('gh pr edit 257 --title "fix: 修好" --body "正文"'))
    assert check_pr_title.extract(data) == ("fix: 修好", "正文", "")


def test_bash_template_flag_keeps_title_checked() -> None:
    data = json.loads(bash_payload('gh pr create -T my-template.md -t "fix: 修好" -b "正文"'))
    assert check_pr_title.extract(data) == ("fix: 修好", "正文", "")


def test_bash_review_yields_review_body() -> None:
    data = json.loads(bash_payload('gh pr review 257 --body "评审正文"'))
    assert check_pr_title.extract(data) == ("", "", "评审正文")


def test_bash_review_short_flag_attached() -> None:
    data = json.loads(bash_payload("gh pr review 257 -b评审正文"))
    assert check_pr_title.extract(data) == ("", "", "评审正文")


def test_bash_review_body_file_fails_open() -> None:
    data = json.loads(bash_payload("gh pr review 257 --body-file /tmp/r.md"))
    assert check_pr_title.extract(data) == ("", "", "")


def test_bash_review_without_body_fails_open() -> None:
    data = json.loads(bash_payload("gh pr review 257 --approve"))
    assert check_pr_title.extract(data) == ("", "", "")


def test_bash_comment_not_gated() -> None:
    data = json.loads(bash_payload('gh pr comment 257 --body "随手一问"'))
    assert check_pr_title.extract(data) == ("", "", "")


def test_bash_fill_fails_open() -> None:
    data = json.loads(bash_payload("gh pr create --fill"))
    assert check_pr_title.extract(data) == ("", "", "")


def test_bash_runtime_value_fails_open() -> None:
    data = json.loads(bash_payload('gh pr review 257 --body "$(cat /tmp/r.md)"'))
    assert check_pr_title.extract(data) == ("", "", "")


def test_bash_flag_after_shell_break_ignored() -> None:
    data = json.loads(bash_payload("gh pr review 257 --comment && gh pr view 257 --web"))
    assert check_pr_title.extract(data) == ("", "", "")


def test_unrelated_command_yields_nothing() -> None:
    data = json.loads(bash_payload("ls -la"))
    assert check_pr_title.extract(data) == ("", "", "")


# ---- 主入口路由 ----


def test_review_violation_blocks(capsys) -> None:
    payload = bash_payload('gh pr review 257 --body "缺骨架的评审"')
    code, err = run_main(payload, capsys)
    assert code == 2
    assert "misses required section: 摘要" in err


def test_conforming_review_passes(capsys) -> None:
    payload = bash_payload("gh pr review 257 --body %s" % shlex.quote(CONFORMING_REVIEW))
    assert run_main(payload, capsys)[0] == 0


def test_unrelated_payload_skipped(capsys) -> None:
    assert run_main(bash_payload("ls -la"), capsys)[0] == 0


def test_pr_body_still_checked(capsys) -> None:
    payload = bash_payload('gh pr create --title "fix: 修好" --body "缺模板小节"')
    code, err = run_main(payload, capsys)
    assert code == 2
    assert "misses required template section" in err
