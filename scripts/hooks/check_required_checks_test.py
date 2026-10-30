"""必需检查对账门禁的单测：打桩的接口（API）响应与工作流夹具逐条覆盖。

被测入口是对账的纯函数层（收集、差集、清单解析），不触网不调命令行工具
（gh）；接口响应桩取自真实分支保护载荷的形态，工作流夹具按仓库现行工作
流的形态最小化组装，逐用例删改触发单一规则。三条计划指定样例：全绿、缺
一个必需检查名的红灯、评审专属作业不误报；另有评审专属作业缺失不漏报、
行内评论作业收集进对账面且按豁免登记不报缺必需（红牌定位）、
非拉取请求（PR）链路工作流不误报、推送条件作业不误报、矩阵展开名豁免、
解析失败关闭与远端地址解析的边界。
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import check_required_checks  # noqa: E402

CI_WORKFLOW = """\
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  fmt:
    name: Format
    runs-on: ubuntu-latest
    steps:
      - name: Checkout
        uses: actions/checkout@v7

  build:
    name: Build (${{ matrix.target }})
    strategy:
      matrix:
        include:
          - os: macos-latest
            target: aarch64-apple-darwin
    runs-on: ${{ matrix.os }}
    steps:
      - name: Build
        run: just build-target ${{ matrix.target }}
"""

HOOKS_WORKFLOW = """\
name: Hooks

on:
  push:
    branches: [main]
  pull_request:
    types: [opened, synchronize, reopened, edited]
  pull_request_review:
    types: [submitted, edited]
  pull_request_review_comment:
    types: [created, edited]

jobs:
  text-gates:
    name: Text gates
    if: >-
      github.event_name != 'pull_request_review' &&
      github.event_name != 'pull_request_review_comment'
    runs-on: ubuntu-latest
    steps:
      - name: Checkout
        uses: actions/checkout@v7

  review-result:
    name: Review result check
    if: >-
      github.event_name == 'pull_request_review' &&
      github.event.action != 'dismissed' &&
      !endsWith(github.event.review.user.login, '[bot]')
    runs-on: ubuntu-latest
    steps:
      - name: Check review result
        run: python3 scripts/hooks/check_review_result.py body

  review-comment:
    name: Review comment check
    if: >-
      github.event_name == 'pull_request_review_comment' &&
      !endsWith(github.event.comment.user.login, '[bot]') &&
      github.event.comment.body != ''
    runs-on: ubuntu-latest
    steps:
      - name: Check review comment
        run: python3 scripts/hooks/check_review_comment.py body
"""

RECONCILE_WORKFLOW = """\
name: Hooks

on:
  push:
    branches: [main]
  pull_request:
    types: [opened, synchronize, reopened, edited]

jobs:
  reconcile:
    name: Required checks reconcile
    if: github.event_name == 'push'
    runs-on: ubuntu-latest
    steps:
      - name: Reconcile
        run: python3 scripts/hooks/check_required_checks.py
"""

REVIEW_COMMENT_WORKFLOW = """\
name: Hooks

on:
  pull_request_review_comment:
    types: [created, edited]

jobs:
  review-comment:
    name: Review comment check
    if: >-
      github.event_name == 'pull_request_review_comment' &&
      !endsWith(github.event.comment.user.login, '[bot]') &&
      github.event.comment.body != ''
    runs-on: ubuntu-latest
    steps:
      - name: Check review comment
        run: python3 scripts/hooks/check_review_comment.py body
"""

SCHEDULED_WORKFLOW = """\
name: Nightly

on:
  schedule:
    - cron: "0 2 * * *"
  workflow_dispatch:

jobs:
  audit:
    name: Nightly audit
    runs-on: ubuntu-latest
    steps:
      - name: Audit
        run: echo ok
"""

# 打桩的接口响应：主分支保护载荷，形态取自实机接口返回
PROTECTION_PAYLOAD = {
    "url": "https://api.github.com/repos/Losmli010/gloss/branches/main/protection",
    "required_status_checks": {
        "url": "https://api.github.com/repos/Losmli010/gloss/branches/main"
        "/protection/required_status_checks",
        "strict": True,
        "contexts": ["Format", "Text gates", "Review result check"],
        "contexts_url": "https://api.github.com/repos/Losmli010/gloss/branches/main"
        "/protection/required_status_checks/contexts",
    },
    "enforce_admins": {
        "url": "https://api.github.com/repos/Losmli010/gloss/branches"
        "/main/protection/enforce_admins",
        "enabled": True,
    },
}


def write_workflows(tmp_path: Path, workflows: dict[str, str]) -> Path:
    """工作流夹具落盘：键为文件名，返回工作流目录。"""
    directory = tmp_path / ".github" / "workflows"
    directory.mkdir(parents=True)
    for name, text in workflows.items():
        (directory / name).write_text(text, encoding="utf-8")
    return directory


def reconcile(directory: Path, contexts: list[str]) -> list[str]:
    """收集加差集的一步入口：返回缺失的检查名清单；收集面基础设施故障直接
    让用例失败，故障路径由专门用例覆盖。"""
    jobs, infra = check_required_checks.collect_expected_jobs(str(directory))
    assert infra == []
    return [job.check for job in check_required_checks.missing_required(jobs, contexts)]


def test_all_green_stub_passes(tmp_path: Path) -> None:
    directory = write_workflows(tmp_path, {"ci.yml": CI_WORKFLOW, "hooks.yml": HOOKS_WORKFLOW})
    contexts = ["Format", "Text gates", "Review result check"]
    assert reconcile(directory, contexts) == []


def test_missing_one_context_fails(tmp_path: Path) -> None:
    directory = write_workflows(tmp_path, {"ci.yml": CI_WORKFLOW, "hooks.yml": HOOKS_WORKFLOW})
    jobs, infra = check_required_checks.collect_expected_jobs(str(directory))
    assert infra == []
    findings = check_required_checks.missing_required(jobs, ["Format", "Review result check"])
    assert [job.check for job in findings] == ["Text gates"]


def test_review_only_job_not_falsely_flagged_when_required(tmp_path: Path) -> None:
    directory = write_workflows(tmp_path, {"hooks.yml": HOOKS_WORKFLOW})
    contexts = ["Text gates", "Review result check"]
    assert reconcile(directory, contexts) == []


def test_review_only_job_missing_from_required_is_flagged(tmp_path: Path) -> None:
    directory = write_workflows(tmp_path, {"hooks.yml": HOOKS_WORKFLOW})
    assert reconcile(directory, ["Text gates"]) == ["Review result check"]


def test_review_comment_job_collected_under_comment_event(tmp_path: Path) -> None:
    directory = write_workflows(tmp_path, {"hooks.yml": REVIEW_COMMENT_WORKFLOW})
    jobs, infra = check_required_checks.collect_expected_jobs(str(directory))
    assert infra == []
    assert [(job.check, job.events) for job in jobs] == [
        ("Review comment check", ("pull_request_review_comment",))
    ]


def test_review_comment_job_exempted_not_flagged_missing(tmp_path: Path) -> None:
    # 红牌定位：作业收集进对账面但不进必需清单，闸门作业齐备时缺必需不报，
    # 豁免缘由显式登记
    directory = write_workflows(tmp_path, {"hooks.yml": HOOKS_WORKFLOW})
    jobs, _ = check_required_checks.collect_expected_jobs(str(directory))
    gate_contexts = [
        "Text gates",
        "Commit message check",
        "PR content check",
        "Review result check",
    ]
    assert check_required_checks.missing_required(jobs, gate_contexts) == []
    flagged = [
        job.check for job in check_required_checks.missing_required(jobs, gate_contexts[:-1])
    ]
    assert flagged == ["Review result check"]
    notes = check_required_checks.exemption_notes(jobs)
    assert any("Review comment check" in note and "exempted" in note for note in notes)


def test_comment_event_jobs_filtered_per_condition(tmp_path: Path) -> None:
    # 评论事件声明后各作业按条件归位：纯文本门禁随事件守卫退出评审与评论
    # 事件，评审结果与行内评论两作业各落其专属事件
    directory = write_workflows(tmp_path, {"hooks.yml": HOOKS_WORKFLOW})
    jobs, _ = check_required_checks.collect_expected_jobs(str(directory))
    events = {job.check: job.events for job in jobs}
    assert events == {
        "Text gates": ("pull_request",),
        "Review result check": ("pull_request_review",),
        "Review comment check": ("pull_request_review_comment",),
    }


def test_schedule_only_workflow_not_expected(tmp_path: Path) -> None:
    directory = write_workflows(tmp_path, {"nightly.yml": SCHEDULED_WORKFLOW})
    assert reconcile(directory, []) == []


def test_push_only_job_not_expected_under_pull_request(tmp_path: Path) -> None:
    directory = write_workflows(tmp_path, {"reconcile.yml": RECONCILE_WORKFLOW})
    assert reconcile(directory, []) == []


def test_matrix_expanded_name_is_exempted(tmp_path: Path) -> None:
    directory = write_workflows(tmp_path, {"ci.yml": CI_WORKFLOW})
    assert reconcile(directory, ["Format"]) == []
    jobs, _ = check_required_checks.collect_expected_jobs(str(directory))
    notes = check_required_checks.exemption_notes(jobs)
    assert any("Build (${{ matrix.target }})" in note and "exempted" in note for note in notes)


def test_unparseable_workflow_fails_closed(tmp_path: Path) -> None:
    directory = write_workflows(tmp_path, {"broken.yml": "jobs:\n  - not: a map\n"})
    jobs, infra = check_required_checks.collect_expected_jobs(str(directory))
    assert jobs == []
    assert [finding.rule for finding in infra] == ["INFRA"]


def test_flow_mapping_trigger_fails_closed(tmp_path: Path) -> None:
    directory = write_workflows(
        tmp_path,
        {
            "flow.yml": (
                "name: T\n"
                "on: {pull_request: null}\n"
                "jobs:\n"
                "  a:\n"
                "    name: A\n"
                "    runs-on: ubuntu-latest\n"
                "    steps:\n"
                "      - name: Go\n"
                "        run: echo hi\n"
            )
        },
    )
    jobs, infra = check_required_checks.collect_expected_jobs(str(directory))
    assert jobs == []
    assert [finding.rule for finding in infra] == ["INFRA"]


def test_flow_mapping_element_inside_flow_list_fails_closed(tmp_path: Path) -> None:
    directory = write_workflows(
        tmp_path,
        {
            "flow.yml": (
                "name: T\n"
                "on: [push, {pull_request: null}]\n"
                "jobs:\n"
                "  a:\n"
                "    name: A\n"
                "    runs-on: ubuntu-latest\n"
                "    steps:\n"
                "      - name: Go\n"
                "        run: echo hi\n"
            )
        },
    )
    jobs, infra = check_required_checks.collect_expected_jobs(str(directory))
    assert jobs == []
    assert [finding.rule for finding in infra] == ["INFRA"]


def test_plain_scalar_and_flow_list_triggers_stay_supported(tmp_path: Path) -> None:
    directory = write_workflows(
        tmp_path,
        {
            "scalar.yml": (
                "name: T\n"
                "on: push\n"
                "jobs:\n"
                "  a:\n"
                "    name: A\n"
                "    runs-on: ubuntu-latest\n"
                "    steps:\n"
                "      - name: Go\n"
                "        run: echo hi\n"
            ),
            "list.yml": (
                "name: T\n"
                "on: [push, pull_request]\n"
                "jobs:\n"
                "  b:\n"
                "    name: B\n"
                "    runs-on: ubuntu-latest\n"
                "    steps:\n"
                "      - name: Go\n"
                "        run: echo hi\n"
            ),
        },
    )
    assert reconcile(directory, []) == ["B"]


def test_condition_evaluation_covers_event_filters() -> None:
    evaluate = check_required_checks.evaluate_condition
    runs_on = check_required_checks.runs_on
    assert evaluate("github.event_name != 'pull_request_review'", "pull_request") is True
    assert evaluate("github.event_name != 'pull_request_review'", "pull_request_review") is False
    assert evaluate("github.event_name == 'pull_request'", "pull_request") is True
    assert evaluate("github.event_name == 'pull_request'", "pull_request_review") is False
    assert evaluate("github.event_name == 'push'", "pull_request") is False
    assert (
        runs_on(
            "github.event_name == 'pull_request_review' && github.event.review.body != ''",
            "pull_request_review",
        )
        is True
    )
    assert runs_on("unparsed ${{ expression }}", "pull_request") is True
    assert runs_on("", "pull_request") is True


def test_protection_payload_contexts_extracted() -> None:
    raw = json.dumps(PROTECTION_PAYLOAD)
    assert check_required_checks.parse_protection_payload(raw) == [
        "Format",
        "Text gates",
        "Review result check",
    ]


def test_protection_payload_wrong_shape_raises() -> None:
    for raw in ("[]", json.dumps({"required_status_checks": {}}), json.dumps({})):
        try:
            check_required_checks.parse_protection_payload(raw)
        except RuntimeError:
            continue
        raise AssertionError("payload shape must fail closed: %s" % raw)


def test_remote_url_parses_owner_and_name() -> None:
    parse = check_required_checks.parse_remote_url
    assert parse("https://github.com/Losmli010/gloss.git") == "Losmli010/gloss"
    assert parse("git@github.com:Losmli010/gloss.git") == "Losmli010/gloss"
    assert parse("ssh://git@github.com/Losmli010/gloss") == "Losmli010/gloss"
    try:
        parse("not-a-remote")
    except RuntimeError:
        return
    raise AssertionError("unparsable remote must raise")


def test_gh_failure_raises_with_permission_hint(monkeypatch) -> None:
    def fake_run(cmd, capture_output):
        return subprocess.CompletedProcess(cmd, 1, stdout=b"", stderr=b"HTTP 403: Forbidden")

    monkeypatch.setattr(check_required_checks.subprocess, "run", fake_run)
    try:
        check_required_checks.fetch_required_contexts("Losmli010/gloss")
    except RuntimeError as exc:
        assert "administration:read" in str(exc)
        return
    raise AssertionError("gh failure must raise")
