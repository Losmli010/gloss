"""第一版内容快照门禁的单测：摘要配方、豁免解析、比对决策与接口载荷逐条
覆盖，外加打桩的端到端锁退出码与基线重写。

被测入口是门禁的纯函数层（配方、豁免、比对、载荷解析），不触网不调命令行
工具（gh）；托管平台交互经模块属性打桩，计划指定的三条样例——改写红灯、
恢复转绿、豁免改写后基线更新——分别落在比对决策与打桩主流程两处，豁免改
写后基线更新以重写调用收到的摘要等于当前摘要断言；快照缺席与描述字段残缺
按无法完成判定失败关闭；另含反馈契约：子进程端到端锁用法退出码。
"""

from __future__ import annotations

import hashlib
import subprocess
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).parent))

import check_pr_snapshot  # noqa: E402
import check_required_checks  # noqa: E402

SCRIPT = Path(__file__).parent / "check_pr_snapshot.py"

TITLE = "feat(gates): 第一版内容快照锁定"
BODY_V1 = "## 摘要\n\n- **基准分支：** 主分支\n"
BODY_V2 = "## 摘要\n\n- **基准分支：** 主分支，验收期补写实测结果\n"
BODY_EXEMPT = "`snapshot:allow` 验收期整体改写，基线随当前版重置\n\n## 摘要\n\n- 改写后的内容\n"
BODY_MARKER_NO_REASON = "snapshot:allow\n"

SLUG = "example/gloss"
NUMBER = "5"
HEAD = "a" * 40


def digest_of(title: str, body: str) -> str:
    return check_pr_snapshot.content_digest(title, body)


# ---- 摘要配方 ----


def test_content_digest_is_the_documented_recipe() -> None:
    assert (
        digest_of(TITLE, BODY_V1)
        == hashlib.sha256((TITLE + "\n" + BODY_V1).encode("utf-8")).hexdigest()
    )


def test_content_digest_changes_with_title_and_body() -> None:
    base = digest_of(TITLE, BODY_V1)
    assert digest_of("改后的标题", BODY_V1) != base
    assert digest_of(TITLE, BODY_V2) != base


def test_empty_body_folds_into_the_recipe() -> None:
    assert digest_of(TITLE, "") == hashlib.sha256((TITLE + "\n").encode("utf-8")).hexdigest()


# ---- 豁免解析 ----


def test_marker_with_reason_on_first_line_is_valid() -> None:
    exempt, problems = check_pr_snapshot.parse_exemption(BODY_EXEMPT)
    assert exempt and problems == []


def test_marker_without_reason_is_invalid_with_allow_reason() -> None:
    exempt, problems = check_pr_snapshot.parse_exemption(BODY_MARKER_NO_REASON)
    assert not exempt
    assert [f.rule for f in problems] == ["ALLOW-REASON"]
    assert problems[0].path == "pull request body" and problems[0].line == 1


def test_backticked_marker_without_reason_is_invalid_too() -> None:
    exempt, problems = check_pr_snapshot.parse_exemption("`snapshot:allow`\n\n## 摘要\n")
    assert not exempt
    assert [f.rule for f in problems] == ["ALLOW-REASON"]


def test_marker_on_later_lines_is_no_exemption() -> None:
    body = "## 摘要\n\n`snapshot:allow` 首行之外的标记不算豁免\n"
    assert check_pr_snapshot.parse_exemption(body) == (False, [])


def test_body_without_marker_is_no_exemption() -> None:
    assert check_pr_snapshot.parse_exemption(BODY_V1) == (False, [])


def test_marker_not_at_line_start_is_no_exemption() -> None:
    body = "豁免声明：`snapshot:allow` 评审人要求整体改写\n\n## 摘要\n"
    assert check_pr_snapshot.parse_exemption(body) == (False, [])


def test_first_line_mention_of_marker_is_no_exemption() -> None:
    body = "实现 snapshot:allow 标记的解析\n\n## 摘要\n"
    assert check_pr_snapshot.parse_exemption(body) == (False, [])


def test_marker_prefix_of_longer_token_is_no_exemption() -> None:
    body = "snapshot:allowxyz 缘由\n"
    assert check_pr_snapshot.parse_exemption(body) == (False, [])


# ---- 比对决策：计划指定三条样例（生产入口经此决策，回调惰性取快照）----


def test_rewritten_content_flags_pr_snapshot() -> None:
    rewrite, problems = check_pr_snapshot.compare_findings(
        digest_of(TITLE, BODY_V2), BODY_V2, lambda: digest_of(TITLE, BODY_V1)
    )
    assert not rewrite
    assert [f.rule for f in problems] == ["PR-SNAPSHOT"]


def test_restored_content_passes_against_the_baseline() -> None:
    rewrite, problems = check_pr_snapshot.compare_findings(
        digest_of(TITLE, BODY_V1), BODY_V1, lambda: digest_of(TITLE, BODY_V1)
    )
    assert not rewrite and problems == []


def test_exemption_rewrite_updates_the_baseline() -> None:
    taken = []

    def stored() -> str:
        taken.append(1)
        return digest_of(TITLE, BODY_V1)

    rewrite, problems = check_pr_snapshot.compare_findings(
        digest_of(TITLE, BODY_EXEMPT), BODY_EXEMPT, stored
    )
    assert rewrite and problems == []
    assert taken == []  # 豁免在场不需要快照，回调不触发


def test_absent_snapshot_turns_into_infra_finding() -> None:
    def absent() -> str:
        raise RuntimeError("no Content snapshot status on the head commit")

    rewrite, problems = check_pr_snapshot.compare_findings(
        digest_of(TITLE, BODY_V1), BODY_V1, absent
    )
    assert not rewrite
    assert [f.rule for f in problems] == ["INFRA"]


def test_snapshot_finding_evidence_carries_both_digests() -> None:
    finding = check_pr_snapshot.snapshot_finding("a" * 64, "b" * 64)
    assert "sha256:" + "a" * 64 in finding.evidence
    assert "sha256:" + "b" * 64 in finding.evidence
    assert finding.remedy == check_pr_snapshot.SNAPSHOT_REMEDY
    assert check_pr_snapshot.EXEMPT_MARKER in finding.remedy


# ---- 接口载荷 ----


def test_snapshot_payload_carries_fixed_context_and_digest() -> None:
    payload = check_pr_snapshot.snapshot_payload("c" * 64)
    assert payload == {
        "state": "success",
        "context": "Content snapshot",
        "description": "sha256:" + "c" * 64,
    }
    assert len(payload["description"]) <= 140


def test_parse_pull_content_folds_null_body() -> None:
    title, body = check_pr_snapshot.parse_pull_content('{"title": "T", "body": null}')
    assert (title, body) == ("T", "")


def test_parse_pull_content_rejects_wrong_shape() -> None:
    with pytest.raises(RuntimeError):
        check_pr_snapshot.parse_pull_content('{"title": 3}')
    with pytest.raises(RuntimeError):
        check_pr_snapshot.parse_pull_content("[]")


def test_find_snapshot_status_takes_the_newest_entry() -> None:
    raw = (
        '[{"context": "CI", "description": "x"},'
        ' {"context": "Content snapshot", "description": "sha256:%s"},'
        ' {"context": "Content snapshot", "description": "sha256:%s"}]' % ("a" * 64, "b" * 64)
    )
    entry = check_pr_snapshot.find_snapshot_status(raw)
    assert entry is not None and entry["description"] == "sha256:" + "a" * 64


def test_find_snapshot_status_absent_returns_none() -> None:
    assert check_pr_snapshot.find_snapshot_status('[{"context": "CI"}]') is None
    with pytest.raises(RuntimeError):
        check_pr_snapshot.find_snapshot_status("{}")


def test_parse_status_digest_rejects_malformed_description() -> None:
    with pytest.raises(RuntimeError):
        check_pr_snapshot.parse_status_digest({"description": "sha256:short"})
    with pytest.raises(RuntimeError):
        check_pr_snapshot.parse_status_digest({"description": "digest:" + "a" * 64})
    assert check_pr_snapshot.parse_status_digest({"description": "sha256:" + "a" * 64}) == "a" * 64


# ---- 打桩主流程：基线重写与失败关闭 ----


class GhStub:
    """打桩的接口层：取数返回预设标题与正文，重写与快照读取按用例接线。"""

    def __init__(self, title: str, body: str, stored: str | None) -> None:
        self.title = title
        self.body = body
        self.stored = stored
        self.written: list[tuple[str, str]] = []

    def install(self, monkeypatch: pytest.MonkeyPatch) -> None:
        monkeypatch.setattr(check_required_checks, "repo_slug", lambda root: SLUG)
        monkeypatch.setattr(
            check_pr_snapshot, "fetch_pull_content", lambda slug, number: (self.title, self.body)
        )
        monkeypatch.setattr(
            check_pr_snapshot,
            "write_snapshot",
            lambda slug, sha, digest: self.written.append((sha, digest)),
        )
        if self.stored is None:

            def raise_absent(slug: str, sha: str) -> str:
                raise RuntimeError("no %s status" % check_pr_snapshot.CONTEXT)

            monkeypatch.setattr(check_pr_snapshot, "fetch_snapshot_digest", raise_absent)
        else:
            monkeypatch.setattr(
                check_pr_snapshot, "fetch_snapshot_digest", lambda slug, sha: self.stored
            )

    def run(self, mode: str) -> int:
        sys.argv = ["check_pr_snapshot.py", mode, NUMBER, HEAD]
        return check_pr_snapshot.main()


def test_write_mode_posts_current_digest_as_baseline(monkeypatch: pytest.MonkeyPatch) -> None:
    stub = GhStub(TITLE, BODY_V1, None)
    stub.install(monkeypatch)
    assert stub.run("write") == 0
    assert stub.written == [(HEAD, digest_of(TITLE, BODY_V1))]


def test_compare_rewrite_puts_current_version_as_new_baseline(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    stub = GhStub(TITLE, BODY_EXEMPT, digest_of(TITLE, BODY_V1))
    stub.install(monkeypatch)
    assert stub.run("compare") == 0
    assert stub.written == [(HEAD, digest_of(TITLE, BODY_EXEMPT))]


def test_compare_mismatch_exits_violation_without_rewrite(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    stub = GhStub(TITLE, BODY_V2, digest_of(TITLE, BODY_V1))
    stub.install(monkeypatch)
    assert stub.run("compare") == 1
    assert stub.written == []
    assert "PR-SNAPSHOT" in capsys.readouterr().err


def test_compare_match_passes_without_rewrite(monkeypatch: pytest.MonkeyPatch) -> None:
    stub = GhStub(TITLE, BODY_V1, digest_of(TITLE, BODY_V1))
    stub.install(monkeypatch)
    assert stub.run("compare") == 0
    assert stub.written == []


def test_compare_marker_without_reason_still_flags_violation(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    stub = GhStub(TITLE, BODY_MARKER_NO_REASON, digest_of(TITLE, BODY_V1))
    stub.install(monkeypatch)
    assert stub.run("compare") == 1
    err = capsys.readouterr().err
    assert "ALLOW-REASON" in err and "PR-SNAPSHOT" in err


def test_compare_absent_snapshot_fails_closed(monkeypatch: pytest.MonkeyPatch) -> None:
    stub = GhStub(TITLE, BODY_V1, None)
    stub.install(monkeypatch)
    assert stub.run("compare") == 3


# ---- 反馈契约 ----


def test_cli_usage_exit_codes() -> None:
    usage = subprocess.run([sys.executable, str(SCRIPT)], capture_output=True)
    assert usage.returncode == 2
    unknown = subprocess.run(
        [sys.executable, str(SCRIPT), "verify", "1", HEAD], capture_output=True
    )
    assert unknown.returncode == 2
