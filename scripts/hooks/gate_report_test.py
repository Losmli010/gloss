"""统一诊断出口的单测：报告模板逐字节锁定、汇总计数、版本库调用包装的失败
关闭路径，以及规则标识一致性——静态扫描门禁脚本目录里全部规则标识常量与违
规条目的规则实参，值必须落在登记清单内；登记表记录文件是本地文件，只在场
时参与两边集合的同步核对，缺席时跳过不判罚。
"""

import io
import re
import subprocess
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).parent))

import gate_report  # noqa: E402

HOOKS = Path(__file__).parent
SCRIPTS = sorted(p for p in HOOKS.rglob("*.py") if "__pycache__" not in p.parts)
REGISTRY_FILE = HOOKS.parent.parent / "docs" / "comments" / "gate_rules_comment.md"
CONST_DEF = re.compile(r'^(RULE_[A-Z0-9_]+) = "([A-Z][A-Z0-9-]*)"(?:\s*#.*)?$', re.M)
RULE_REF = re.compile(r'\brule=(?:"([A-Z][A-Z0-9-]*)"|([A-Z][A-Z0-9_]*))')
REGISTRY_ID = re.compile(r"\|\s*`([A-Z][A-Z0-9-]+)`\s*\|")


def rule_constants():
    out = {}
    for path in SCRIPTS:
        out.update(CONST_DEF.findall(path.read_text(encoding="utf-8")))
    return out


def test_exit_code_constants_match_the_shared_contract():
    assert (
        gate_report.EXIT_PASS,
        gate_report.EXIT_VIOLATION,
        gate_report.EXIT_USAGE,
        gate_report.EXIT_INFRA,
    ) == (0, 1, 2, 3)


def test_fail_template_with_location_evidence_and_help():
    out = io.StringIO()
    gate_report.fail(
        gate_report.Finding(
            rule="SECRET-VENDOR",
            path="crates/leak.toml",
            line=3,
            message="suspected leak",
            evidence='api_key = "..."',
            remedy="rotate it",
        ),
        file=out,
    )
    assert out.getvalue() == (
        "✗ error[SECRET-VENDOR]: suspected leak (crates/leak.toml:3)\n"
        '    证据: api_key = "..."\n'
        "    help: rotate it\n"
    )


def test_fail_template_appends_column_when_present():
    out = io.StringIO()
    gate_report.fail(
        gate_report.Finding(
            rule="SECRET-CREDENTIAL", path="a.toml", line=2, column=9, message="hit"
        ),
        file=out,
    )
    assert out.getvalue() == "✗ error[SECRET-CREDENTIAL]: hit (a.toml:2:9)\n"


def test_fail_template_without_location_and_optional_lines():
    out = io.StringIO()
    gate_report.fail(
        gate_report.Finding(rule="INFRA", path="", line=0, message="tool broke"),
        file=out,
    )
    assert out.getvalue() == "✗ error[INFRA]: tool broke\n"


def test_fail_returns_the_same_finding_for_collection():
    finding = gate_report.Finding(rule="SECRET-AUTH", path="a.txt", line=1, message="bearer leak")
    assert gate_report.fail(finding, file=io.StringIO()) is finding


def test_summary_line_counts_findings():
    one = gate_report.Finding(rule="INFRA", path="", line=0, message="x")
    single = io.StringIO()
    gate_report.summary([one], file=single)
    assert single.getvalue() == "error: 1 violation found (see above)\n"
    triple = io.StringIO()
    gate_report.summary([one, one, one], file=triple)
    assert triple.getvalue() == "error: 3 violations found (see above)\n"


def test_run_git_success_returns_completed_process(tmp_path):
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    proc = gate_report.run_git(["ls-files", "-z"], cwd=str(tmp_path))
    assert proc.returncode == 0


def test_run_git_failure_reports_infra_and_exits_three(tmp_path):
    out = io.StringIO()
    with pytest.raises(SystemExit) as exc:
        gate_report.run_git(["ls-files", "-z"], cwd=str(tmp_path), file=out)
    assert exc.value.code == gate_report.EXIT_INFRA
    text = out.getvalue()
    assert text.startswith("✗ error[INFRA]: git command failed")
    assert "help: " in text


def test_infra_failure_carries_the_shared_remedy():
    finding = gate_report.infra_failure("engine broke", evidence="boom")
    assert finding.rule == gate_report.RULE_INFRA
    assert finding.remedy == gate_report.INFRA_REMEDY
    out = io.StringIO()
    gate_report.fail(finding, file=out)
    assert "    证据: boom" in out.getvalue()


def test_rule_constants_are_registered():
    for name, value in rule_constants().items():
        assert value in gate_report.REGISTERED_RULES, "unregistered constant %s=%s" % (name, value)


def test_finding_rule_references_are_registered():
    constants = rule_constants()
    for path in SCRIPTS:
        for quoted, bare in RULE_REF.findall(path.read_text(encoding="utf-8")):
            if quoted:
                assert quoted in gate_report.REGISTERED_RULES, (
                    "%s uses unregistered rule literal %s" % (path.name, quoted)
                )
            else:
                assert bare in constants, "%s uses unknown rule constant %s" % (path.name, bare)
                assert constants[bare] in gate_report.REGISTERED_RULES


def test_registered_rules_follow_the_naming_rule():
    for rule in gate_report.REGISTERED_RULES:
        assert re.fullmatch(r"[A-Z][A-Z0-9-]*", rule), rule


def test_registry_record_file_matches_registered_rules():
    if not REGISTRY_FILE.is_file():
        pytest.skip("registry record file is local-only and absent here")
    ids = set(REGISTRY_ID.findall(REGISTRY_FILE.read_text(encoding="utf-8")))
    assert ids == set(gate_report.REGISTERED_RULES)
