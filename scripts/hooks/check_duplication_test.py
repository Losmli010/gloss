"""克隆门禁的单测：判罚口径（三规则、判定顺序短路、放行缘由校验、基线冻结
合计）、基线装入与体检清理、任务包拼装与筛选、引擎调用装配与缺席失败关闭、
并轨装入器（跟踪清单经共享装入器供给引擎）。

被测入口是门禁的纯判定函数与各模式运行函数；扫描面用测试框架（pytest）的临
时目录建一次性版本库，引擎用注入的假报告替身——单测不拉起真实引擎，真实引擎
的集成面由本仓配方实跑覆盖。行文与输出契约断言经统一诊断出口的字符串形态。
"""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).parent))

import check_duplication  # noqa: E402
import gate_report  # noqa: E402

MARKER = check_duplication.ALLOW_MARKER


# ---- 夹具助手 ----


def seed_repo(tmp_path: Path, files: dict[str, str]) -> str:
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    for name, text in files.items():
        p = tmp_path / name
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text, encoding="utf-8")
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    return str(tmp_path)


def filler(count: int, tag: str, mark: int | None = None) -> str:
    """夹具行文：计数行填充，可选在某一行尾追加放行标记加缘由。"""
    lines = ["let %s_%d = %d;" % (tag, i, i) for i in range(count)]
    if mark is not None:
        lines[mark] = lines[mark] + " // %s 测试夹具放行缘由" % MARKER
    return "\n".join(lines) + "\n"


def report_of(root: str, *sides: tuple[str, int, int, str, int, int]) -> dict:
    """引擎报告替身：绝对路径形态，统计面按克隆行合计拼装。"""
    duplicates = []
    for first, s1, e1, second, s2, e2 in sides:
        duplicates.append(
            {
                "firstFile": {"name": str(Path(root) / first), "start": s1, "end": e1},
                "secondFile": {"name": str(Path(root) / second), "start": s2, "end": e2},
                "lines": e1 - s1 + 1,
            }
        )
    total = sum(e1 - s1 + 1 for _f, s1, e1, _s, _s2, e2 in sides)
    return {
        "duplicates": duplicates,
        "statistics": {
            "total": {
                "clones": len(duplicates),
                "duplicatedLines": total,
                "lines": 100,
                "sources": 2,
                "percentage": 1.0,
            }
        },
    }


def fake_engine(report: dict, calls: list | None = None):
    def engine(root: str, files: list, min_tokens: int) -> dict:
        if calls is not None:
            calls.append((root, list(files), min_tokens))
        return report

    return engine


def pair(first: str, s1: int, e1: int, second: str, s2: int, e2: int | None = None) -> object:
    end2 = e2 if e2 is not None else s2 + (e1 - s1)
    return check_duplication.ClonePair(
        check_duplication.CloneSide(first, s1, e1), check_duplication.CloneSide(second, s2, end2)
    )


def lines_of(root: str, clones: list) -> dict:
    paths = {side.path for clone in clones for side in (clone.first, clone.second)}
    return check_duplication.load_lines(root, paths)


def entry(first: str, second: str, max_lines: int) -> check_duplication.BaselineEntry:
    return check_duplication.BaselineEntry(
        first, second, max_lines, "retain", "测试缘由", "2026-11-21"
    )


def rules(findings: list) -> set:
    return {f.rule for f in findings}


# ---- 受检面与报告归一 ----


def test_surface_filters_guarded_trees() -> None:
    tracked = [
        "crates/a.rs",
        "src/main.rs",
        "benches/b.rs",
        "docs/c.rs.md",
        "crates/x/lib.py",
        "assets/logo.svg",
    ]
    assert check_duplication.surface_files(tracked) == ["crates/a.rs", "src/main.rs"]


def test_empty_surface_fails_closed(tmp_path: Path) -> None:
    seed_repo(tmp_path, {"README.md": "没有源码\n"})
    calls: list = []
    with pytest.raises(check_duplication.GateInfra):
        check_duplication.scan(str(tmp_path), 150, fake_engine({}, calls))
    assert calls == []


def test_report_normalizes_absolute_paths(tmp_path: Path) -> None:
    report = report_of(str(tmp_path), ("src/a.rs", 10, 20, "src/b.rs", 30, 40))
    clones = check_duplication.normalize_report(report, str(tmp_path))
    assert len(clones) == 1
    assert clones[0].first.path == "src/a.rs"
    assert (clones[0].first.start, clones[0].first.end) == (10, 20)
    assert clones[0].second.path == "src/b.rs"
    assert clones[0].lines == 11
    assert clones[0].key == ("src/a.rs", "src/b.rs")


def test_report_pair_key_sorts_both_directions(tmp_path: Path) -> None:
    report = report_of(str(tmp_path), ("src/b.rs", 1, 5, "src/a.rs", 7, 11))
    clones = check_duplication.normalize_report(report, str(tmp_path))
    assert clones[0].first.path == "src/b.rs"
    assert clones[0].key == ("src/a.rs", "src/b.rs")
    assert clones[0].anchor().path == "src/a.rs"


def test_report_outside_root_fails_closed(tmp_path: Path) -> None:
    report = {
        "duplicates": [
            {
                "firstFile": {"name": "/elsewhere/a.rs", "start": 1, "end": 5},
                "secondFile": {"name": str(tmp_path / "b.rs"), "start": 1, "end": 5},
            }
        ],
        "statistics": {"total": {}},
    }
    with pytest.raises(check_duplication.GateInfra):
        check_duplication.normalize_report(report, str(tmp_path))


def test_report_missing_statistics_fails_closed(tmp_path: Path) -> None:
    with pytest.raises(check_duplication.GateInfra):
        check_duplication.normalize_report({"duplicates": []}, str(tmp_path))


# ---- 判罚规则一：新克隆红牌 ----


def test_unbaselined_pair_is_dup_new(tmp_path: Path) -> None:
    root = seed_repo(tmp_path, {"src/a.rs": filler(30, "a"), "src/b.rs": filler(30, "b")})
    clones = [pair("src/a.rs", 10, 20, "src/b.rs", 10, 20)]
    findings = check_duplication.judge_clones(clones, [], lines_of(root, clones))
    assert rules(findings) == {"DUP-NEW"}
    assert findings[0].path == "src/a.rs"
    assert findings[0].line == 10
    assert findings[0].message.startswith("unregistered clone")
    assert "src/b.rs:10-20" in findings[0].evidence
    assert findings[0].remedy


# ---- 判罚规则二：就地放行与缘由校验 ----


def test_marker_with_reason_grants_instance_without_baseline(tmp_path: Path) -> None:
    root = seed_repo(
        tmp_path,
        {"src/a.rs": filler(30, "a", mark=9), "src/b.rs": filler(30, "b")},
    )
    clones = [pair("src/a.rs", 10, 20, "src/b.rs", 10, 20)]
    findings = check_duplication.judge_clones(clones, [], lines_of(root, clones))
    assert findings == []


def test_marker_on_second_side_grants_instance(tmp_path: Path) -> None:
    root = seed_repo(
        tmp_path,
        {"src/a.rs": filler(30, "a"), "src/b.rs": filler(30, "b", mark=11)},
    )
    clones = [pair("src/a.rs", 10, 20, "src/b.rs", 12, 22)]
    findings = check_duplication.judge_clones(clones, [], lines_of(root, clones))
    assert findings == []


def test_bare_marker_reports_reason_and_falls_through(tmp_path: Path) -> None:
    lines = ["let x = 1; // %s" % MARKER] + ["let y_%d = %d;" % (i, i) for i in range(10)]
    root = seed_repo(tmp_path, {"src/a.rs": "\n".join(lines) + "\n", "src/b.rs": filler(30, "b")})
    clones = [pair("src/a.rs", 1, 5, "src/b.rs", 10, 14)]
    findings = check_duplication.judge_clones(clones, [], lines_of(root, clones))
    assert rules(findings) == {"DUP-NEW", "DUP-ALLOW-REASON"}
    reason = [f for f in findings if f.rule == "DUP-ALLOW-REASON"][0]
    assert reason.path == "src/a.rs"
    assert reason.line == 1
    assert reason.remedy == gate_report.ALLOW_REASON_REMEDY


# ---- 判罚规则三：基线冻结合计 ----


def test_baseline_allows_pair_within_frozen_total(tmp_path: Path) -> None:
    root = seed_repo(tmp_path, {"src/a.rs": filler(40, "a"), "src/b.rs": filler(40, "b")})
    clones = [
        pair("src/a.rs", 10, 19, "src/b.rs", 10, 19),
        pair("src/a.rs", 30, 39, "src/b.rs", 30, 39),
    ]
    entries = [entry("src/b.rs", "src/a.rs", 20)]
    findings = check_duplication.judge_clones(clones, entries, lines_of(root, clones))
    assert findings == []


def test_baseline_busted_pair_flags_every_clone(tmp_path: Path) -> None:
    root = seed_repo(tmp_path, {"src/a.rs": filler(40, "a"), "src/b.rs": filler(40, "b")})
    clones = [
        pair("src/a.rs", 10, 19, "src/b.rs", 10, 19),
        pair("src/a.rs", 30, 39, "src/b.rs", 30, 39),
    ]
    entries = [entry("src/a.rs", "src/b.rs", 15)]
    findings = check_duplication.judge_clones(clones, entries, lines_of(root, clones))
    assert [f.rule for f in findings] == ["DUP-NEW", "DUP-NEW"]
    for finding in findings:
        assert "exceeds the frozen max_lines 15" in finding.evidence


def test_baseline_total_counts_across_report_directions(tmp_path: Path) -> None:
    root = seed_repo(tmp_path, {"src/a.rs": filler(40, "a"), "src/b.rs": filler(40, "b")})
    clones = [
        pair("src/b.rs", 10, 19, "src/a.rs", 10, 19),
        pair("src/a.rs", 30, 39, "src/b.rs", 30, 39),
    ]
    entries = [entry("src/a.rs", "src/b.rs", 15)]
    findings = check_duplication.judge_clones(clones, entries, lines_of(root, clones))
    assert [f.rule for f in findings] == ["DUP-NEW", "DUP-NEW"]


def test_judge_output_is_deterministic(tmp_path: Path) -> None:
    root = seed_repo(
        tmp_path,
        {
            "src/a.rs": filler(40, "a", mark=29),
            "src/b.rs": filler(40, "b"),
            "src/c.rs": filler(40, "c"),
        },
    )
    clones = [
        pair("src/c.rs", 5, 9, "src/a.rs", 1, 5),
        pair("src/a.rs", 30, 39, "src/b.rs", 30, 39),
        pair("src/a.rs", 10, 19, "src/b.rs", 10, 19),
        pair("src/b.rs", 1, 4, "src/a.rs", 20, 23),
    ]
    first = check_duplication.judge_clones(clones, [], lines_of(root, clones))
    second = check_duplication.judge_clones(list(reversed(clones)), [], lines_of(root, clones))
    assert [(f.rule, f.path, f.line) for f in first] == [(f.rule, f.path, f.line) for f in second]


# ---- 基线装入与形态校验 ----


def write_baseline(path: Path, text: str) -> str:
    path.write_text(text, encoding="utf-8")
    return str(path)


def test_baseline_empty_file_loads_empty(tmp_path: Path) -> None:
    path = write_baseline(tmp_path / "baseline.toml", "# 只有说明\n")
    assert check_duplication.load_baseline(path) == []


def test_baseline_entry_loads(tmp_path: Path) -> None:
    path = write_baseline(
        tmp_path / "baseline.toml",
        '[[entry]]\nfirst = "a.rs"\nsecond = "b.rs"\nmax_lines = 44\n'
        'kind = "todo"\nreason = "待消"\nregistered = "2026-11-18"\n',
    )
    entries = check_duplication.load_baseline(path)
    assert len(entries) == 1
    assert entries[0].key == ("a.rs", "b.rs")
    assert entries[0].max_lines == 44
    assert entries[0].kind == "todo"


def test_baseline_duplicate_pair_fails_closed(tmp_path: Path) -> None:
    body = (
        '[[entry]]\nfirst = "a.rs"\nsecond = "b.rs"\nmax_lines = 10\n'
        'kind = "retain"\nreason = "r"\nregistered = "2026-11-18"\n'
        '[[entry]]\nfirst = "b.rs"\nsecond = "a.rs"\nmax_lines = 10\n'
        'kind = "retain"\nreason = "r"\nregistered = "2026-11-18"\n'
    )
    path = write_baseline(tmp_path / "baseline.toml", body)
    with pytest.raises(check_duplication.GateInfra):
        check_duplication.load_baseline(path)


@pytest.mark.parametrize(
    "mutation",
    [
        'first = "a.rs"\nsecond = "b.rs"\nkind = "retain"\nreason = "r"\nregistered = "d"\n',
        'first = "a.rs"\nsecond = "b.rs"\nmax_lines = 0\nkind = "retain"\nreason = "r"\nregistered = "d"\n',
        'first = "a.rs"\nsecond = "b.rs"\nmax_lines = 10\nkind = "defer"\nreason = "r"\nregistered = "d"\n',
        'first = "a.rs"\nsecond = "b.rs"\nmax_lines = 10\nkind = "retain"\nreason = ""\nregistered = "d"\n',
        'first = "a.rs"\nsecond = "b.rs"\nmax_lines = 10\nkind = "retain"\nreason = "r"\nregistered = "d"\nextra = 1\n',
    ],
)
def test_baseline_malformed_entry_fails_closed(tmp_path: Path, mutation: str) -> None:
    path = write_baseline(tmp_path / "baseline.toml", "[[entry]]\n" + mutation)
    with pytest.raises(check_duplication.GateInfra):
        check_duplication.load_baseline(path)


# ---- 基线体检与一键清理 ----


def test_doctor_reports_stale_entry(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture
) -> None:
    seed_repo(tmp_path, {"src/a.rs": filler(40, "a"), "src/b.rs": filler(40, "b")})
    report = report_of(str(tmp_path), ("src/a.rs", 10, 19, "src/b.rs", 10, 19))
    baseline = tmp_path / "baseline.toml"
    write_baseline(
        baseline,
        "# 说明\n"
        '[[entry]]\nfirst = "src/a.rs"\nsecond = "src/b.rs"\nmax_lines = 30\n'
        'kind = "todo"\nreason = "旧克隆"\nregistered = "2026-11-18"\n'
        '[[entry]]\nfirst = "src/a.rs"\nsecond = "src/c.rs"\nmax_lines = 10\n'
        'kind = "retain"\nreason = "已消"\nregistered = "2026-11-18"\n',
    )
    monkeypatch.setattr(check_duplication, "BASELINE_PATH", str(baseline))
    monkeypatch.setattr(sys, "argv", ["check_duplication.py", "doctor", str(tmp_path)])
    code = check_duplication.main(engine=fake_engine(report))
    out = capsys.readouterr()
    assert code == 1
    assert "error[DUP-STALE]:" in out.err
    assert "src/c.rs" in out.err


def test_doctor_fix_prunes_stale_and_keeps_live(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture
) -> None:
    seed_repo(tmp_path, {"src/a.rs": filler(40, "a"), "src/b.rs": filler(40, "b")})
    report = report_of(str(tmp_path), ("src/a.rs", 10, 19, "src/b.rs", 10, 19))
    baseline = tmp_path / "baseline.toml"
    head = "# 克隆门禁基线：说明保持原样\n"
    live = (
        '[[entry]]\nfirst = "src/a.rs"\nsecond = "src/b.rs"\nmax_lines = 30\n'
        'kind = "todo"\nreason = "旧克隆"\nregistered = "2026-11-18"\n'
    )
    stale = (
        '[[entry]]\nfirst = "src/a.rs"\nsecond = "src/c.rs"\nmax_lines = 10\n'
        'kind = "retain"\nreason = "已消"\nregistered = "2026-11-18"\n'
    )
    write_baseline(baseline, head + live + stale)
    monkeypatch.setattr(check_duplication, "BASELINE_PATH", str(baseline))
    monkeypatch.setattr(sys, "argv", ["check_duplication.py", "doctor", "--fix", str(tmp_path)])
    code = check_duplication.main(engine=fake_engine(report))
    out = capsys.readouterr()
    assert code == 0
    assert "pruned 1 stale baseline entry" in out.out
    text = baseline.read_text(encoding="utf-8")
    assert text.startswith(head)
    assert "src/c.rs" not in text
    assert "max_lines = 30" in text
    entries = check_duplication.load_baseline(str(baseline))
    assert len(entries) == 1
    assert entries[0].first == "src/a.rs"


def test_doctor_clean_baseline_passes(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture
) -> None:
    seed_repo(tmp_path, {"src/a.rs": filler(40, "a")})
    baseline = tmp_path / "baseline.toml"
    write_baseline(baseline, "# 空\n")
    monkeypatch.setattr(check_duplication, "BASELINE_PATH", str(baseline))
    monkeypatch.setattr(sys, "argv", ["check_duplication.py", "doctor", str(tmp_path)])
    code = check_duplication.main(
        engine=fake_engine({"duplicates": [], "statistics": {"total": {}}})
    )
    out = capsys.readouterr()
    assert code == 0
    assert "ok: baseline doctor passed" in out.out


# ---- 任务包拼装与筛选 ----


def test_triage_package_contains_four_components(tmp_path: Path) -> None:
    prompt = (
        "Q1 变化轴\nQ2 边界\nQ3 抽象成本\n## 输出契约（三选一，全部带缘由）\n"
        "### 词表映射\n### 仓库默认值\n"
    )
    lines = {
        "src/macos/clip.rs": filler(30, "m").split("\n"),
        "src/windows/clip.rs": filler(30, "w").split("\n"),
    }
    clone = pair("src/macos/clip.rs", 5, 15, "src/windows/clip.rs", 5, 15)
    package = check_duplication.triage_package(
        clone, prompt, lines, [entry("src/macos/clip.rs", "src/windows/clip.rs", 10)]
    )
    assert "Q1 变化轴" in package and "Q2 边界" in package and "Q3 抽象成本" in package
    assert "词表映射" in package and "仓库默认值" in package
    assert "## 2. 克隆对上下文" in package
    assert "src/macos/clip.rs:5-15" in package and "src/windows/clip.rs:5-15" in package
    assert "let m_5 = 5;" in package and "let w_5 = 5;" in package
    assert "跨平台目录对" in package and "retain" in package
    assert "基线已有同簇登记" in package and "三次规则" in package
    assert "## 3. 验证清单" in package
    assert (
        "just precommit" in package and "cargo test -p" in package and "just duplicates" in package
    )
    assert "just fmt-toml" in package
    assert "## 4. 输出契约" in package
    assert "eliminate" in package and "exempt" in package


def test_triage_package_suggests_test_helper_for_test_sides(tmp_path: Path) -> None:
    lines = {
        "crates/x/tests/stub.rs": filler(20, "t").split("\n"),
        "src/main.rs": filler(20, "s").split("\n"),
    }
    clone = pair("crates/x/tests/stub.rs", 5, 10, "src/main.rs", 5, 10)
    package = check_duplication.triage_package(clone, "内核\n", lines, [])
    assert "包内共享测试助手" in package


def test_triage_fragment_caps_at_forty_lines() -> None:
    lines = ["line_%d" % i for i in range(60)]
    side = check_duplication.CloneSide("a.rs", 1, 60)
    text = check_duplication.fragment(lines, side)
    body = [ln for ln in text.split("\n") if not ln.startswith("...")]
    assert len(body) == 40
    assert text.endswith("... (20 more lines)")


def test_triage_filter_matches_ident_and_path(tmp_path: Path) -> None:
    clones = [
        pair("src/a.rs", 1, 5, "src/b.rs", 1, 5),
        pair("src/a.rs", 10, 14, "src/b.rs", 10, 14),
    ]
    target = clones[1].ident
    assert [c.ident for c in check_duplication.select_clones(clones, target)] == [target]
    assert [c.ident for c in check_duplication.select_clones(clones, "src/b.rs")] == [
        clones[0].ident,
        clones[1].ident,
    ]
    assert check_duplication.select_clones(clones, "") == clones


def test_triage_run_embeds_real_prompt(tmp_path: Path, capsys: pytest.CaptureFixture) -> None:
    root = seed_repo(tmp_path, {"src/a.rs": filler(30, "a"), "src/b.rs": filler(30, "b")})
    report = report_of(str(tmp_path), ("src/a.rs", 10, 19, "src/b.rs", 10, 19))
    code = check_duplication.run_triage(root, "", fake_engine(report))
    out = capsys.readouterr().out
    assert code == 0
    assert "=== 克隆判定任务包 " in out
    assert "Q1 变化轴" in out and "词表映射" in out
    assert "src/a.rs:10-19" in out
    assert "== 1 task package(s) ==" in out


# ---- 引擎调用装配与失败关闭 ----


def test_engine_command_assembly(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    captured: dict = {}

    def fake_run(cmd, cwd=None, capture_output=False):
        captured["cmd"] = list(cmd)
        captured["cwd"] = cwd
        out_dir = cmd[cmd.index("--output") + 1]
        Path(out_dir, check_duplication.REPORT_NAME).write_text(
            '{"duplicates": [], "statistics": {"total": {}}}', encoding="utf-8"
        )
        return subprocess.CompletedProcess(cmd, 0, b"", b"")

    monkeypatch.setattr(check_duplication.subprocess, "run", fake_run)
    monkeypatch.setattr(check_duplication.tool_version, "pinned_version", lambda name: "9.9.9")
    surface = [str(tmp_path / "src/a.rs"), str(tmp_path / "src/b.rs")]
    report = check_duplication.run_engine(str(tmp_path), surface, 150)
    assert report["duplicates"] == []
    cmd = captured["cmd"]
    assert cmd[0] == check_duplication.ENGINE_BIN
    for flag in ("--absolute", "--min-tokens", "150", "--reporters", "json", "--silent"):
        assert flag in cmd
    assert cmd[-2:] == surface
    assert captured["cwd"] == str(tmp_path)


def test_engine_missing_fails_closed(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    def boom(cmd, cwd=None, capture_output=False):
        raise FileNotFoundError()

    monkeypatch.setattr(check_duplication.subprocess, "run", boom)
    monkeypatch.setattr(check_duplication.tool_version, "pinned_version", lambda name: "9.9.9")
    with pytest.raises(check_duplication.GateInfra, match="npm install"):
        check_duplication.run_engine(str(tmp_path), [str(tmp_path / "a.rs")], 150)


def test_engine_failure_fails_closed(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    def boom(cmd, cwd=None, capture_output=False):
        return subprocess.CompletedProcess(cmd, 2, b"", b"engine exploded")

    monkeypatch.setattr(check_duplication.subprocess, "run", boom)
    monkeypatch.setattr(check_duplication.tool_version, "pinned_version", lambda name: "9.9.9")
    with pytest.raises(check_duplication.GateInfra, match="engine exploded"):
        check_duplication.run_engine(str(tmp_path), [str(tmp_path / "a.rs")], 150)


# ---- 模式解析与退出码 ----


def test_parse_args_defaults_and_flags() -> None:
    assert check_duplication.parse_args([]) == ("punish", "", check_duplication.REPO_ROOT, False)
    assert check_duplication.parse_args(["--report"])[:2] == ("report", "")
    assert check_duplication.parse_args(["--triage=x"])[1] == "x"
    assert check_duplication.parse_args(["doctor", "--fix"]) == (
        "doctor",
        "",
        check_duplication.REPO_ROOT,
        True,
    )


def test_parse_args_rejects_mode_conflict_and_unknown_flag() -> None:
    with pytest.raises(ValueError):
        check_duplication.parse_args(["--report", "--triage"])
    with pytest.raises(ValueError):
        check_duplication.parse_args(["--wat"])


def test_punish_exit_one_with_pointer(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture
) -> None:
    root = seed_repo(tmp_path, {"src/a.rs": filler(30, "a"), "src/b.rs": filler(30, "b")})
    report = report_of(str(tmp_path), ("src/a.rs", 10, 19, "src/b.rs", 10, 19))
    monkeypatch.setattr(sys, "argv", ["check_duplication.py", root])
    code = check_duplication.main(engine=fake_engine(report))
    out = capsys.readouterr()
    assert code == 1
    assert "error[DUP-NEW]:" in out.err
    assert "    evidence: " in out.err and "    help: " in out.err
    assert "triage: run `just dup-triage`" in out.err


def test_punish_clean_repo_passes(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture
) -> None:
    seed_repo(tmp_path, {"src/a.rs": filler(30, "a")})
    monkeypatch.setattr(sys, "argv", ["check_duplication.py"])
    code = check_duplication.main(
        engine=fake_engine({"duplicates": [], "statistics": {"total": {}}})
    )
    out = capsys.readouterr()
    assert code == 0
    assert "ok: clone gate passed (0 clones scanned, surface clean)" in out.out


def test_report_mode_prints_and_never_fails(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture
) -> None:
    root = seed_repo(tmp_path, {"src/a.rs": filler(30, "a"), "src/b.rs": filler(30, "b")})
    report = report_of(str(tmp_path), ("src/a.rs", 10, 19, "src/b.rs", 10, 19))
    monkeypatch.setattr(sys, "argv", ["check_duplication.py", "--report", root])
    code = check_duplication.main(engine=fake_engine(report))
    out = capsys.readouterr()
    assert code == 0
    assert "observation report" in out.out
    assert "  10 lines /  1 clones  src/a.rs <-> src/b.rs" in out.out
    assert "src/a.rs:10 <-> src/b.rs:10" in out.out


def test_usage_error_exit_two(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture
) -> None:
    monkeypatch.setattr(sys, "argv", ["check_duplication.py", "--wat"])
    code = check_duplication.main(engine=fake_engine({}))
    err = capsys.readouterr().err
    assert code == 2
    assert "usage:" in err


def test_infra_exit_three_on_engine_failure(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture
) -> None:
    root = seed_repo(tmp_path, {"src/a.rs": filler(30, "a")})

    def broken(root: str, files: list, min_tokens: int) -> dict:
        raise check_duplication.GateInfra("engine gone")

    monkeypatch.setattr(sys, "argv", ["check_duplication.py", root])
    code = check_duplication.main(engine=broken)
    err = capsys.readouterr().err
    assert code == 3
    assert "error[INFRA]:" in err
    assert "engine gone" in err


def test_report_text_aggregates_pairs_deterministically(tmp_path: Path) -> None:
    clones = [
        pair("src/a.rs", 1, 10, "src/b.rs", 1, 10),
        pair("src/a.rs", 20, 34, "src/b.rs", 20, 34),
        pair("src/c.rs", 1, 3, "src/d.rs", 1, 3),
    ]
    report = {
        "statistics": {
            "total": {
                "clones": 3,
                "duplicatedLines": 28,
                "lines": 1000,
                "sources": 4,
                "percentage": 2.8,
            }
        }
    }
    text = check_duplication.report_text(clones, report)
    again = check_duplication.report_text(list(reversed(clones)), report)
    assert text == again
    pair_section = text.split("clones (")[0]
    assert pair_section.index("src/a.rs") < pair_section.index("src/c.rs")
    assert "totals: 3 clones, 28 duplicated lines over 1000 lines in 4 sources (2.8%)" in text


# ---- 并轨装入器 ----


class _Loader:
    """装入器替身：只实现并轨共享的跟踪清单枚举，按注入清单返回。"""

    def __init__(self, tracked: list[str]) -> None:
        self._tracked = tracked

    def tracked(self) -> list[str]:
        return list(self._tracked)


def test_merged_loader_feeds_engine_and_judges(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture
) -> None:
    root = seed_repo(tmp_path, {"src/a.rs": filler(40, "a"), "src/b.rs": filler(40, "b")})
    report = report_of(root, ("src/a.rs", 10, 19, "src/b.rs", 10, 19))
    calls = []
    baseline = tmp_path / "baseline.toml"
    write_baseline(baseline, "# 空\n")
    monkeypatch.setattr(check_duplication, "BASELINE_PATH", str(baseline))
    monkeypatch.setattr(sys, "argv", ["check_duplication.py", root])
    loader = _Loader(["src/a.rs", "src/b.rs"])
    code = check_duplication.main(engine=fake_engine(report, calls), loader=loader)
    out = capsys.readouterr()
    assert code == 1
    assert calls[0][1] == [os.path.join(root, "src/a.rs"), os.path.join(root, "src/b.rs")]
    assert "error[DUP-NEW]:" in out.err


def test_merged_loader_replaces_default_enumeration(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture
) -> None:
    root = seed_repo(tmp_path, {"src/a.rs": filler(40, "a"), "src/b.rs": filler(40, "b")})
    report = report_of(root, ("src/a.rs", 10, 19, "src/b.rs", 10, 19))
    calls = []
    baseline = tmp_path / "baseline.toml"
    write_baseline(baseline, "# 空\n")
    monkeypatch.setattr(check_duplication, "BASELINE_PATH", str(baseline))
    monkeypatch.setattr(sys, "argv", ["check_duplication.py", root])
    code = check_duplication.main(engine=fake_engine(report, calls), loader=_Loader(["src/a.rs"]))
    assert code == 1
    assert calls[0][1] == [os.path.join(root, "src/a.rs")]
