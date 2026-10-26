"""密钥扫描门禁的单测：逐条覆盖服务商模式、通用凭据赋值与放行标记。

被测入口是门禁的运行函数，返回违规列表；在测试框架（pytest）的临时目录里建
一次性版本库作扫描面，端到端等价靠种子违规样本，匹配引擎要求正则检索工具
（ripgrep）在场。夹具里的疑似密钥一律由片段拼接生成，不让字面量出现在
本文件（否则门禁自命中）；用法见持续集成的钩子工作流。另含反馈契约三件套：
报告格式断言、汇总计数与确定性，经子进程跑脚本端到端锁输出与退出码。
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import check_secrets  # noqa: E402

MARKER = "secrets:allow"
TAIL12 = "0" * 12
TAIL16 = "0" * 16
TAIL20 = "0" * 20
TAIL35 = "a" * 35
TAIL36 = "a" * 36
TAIL40 = "a" * 40
AWS_TAIL16 = "A" * 16


def scan(tmp_path: Path, rel: str, content: str) -> list[str]:
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    p = tmp_path / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(content + "\n", encoding="utf-8")
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    return check_secrets.run(str(tmp_path))


# ---- 干净文件（放行）----


def test_clean_code_passes(tmp_path: Path) -> None:
    assert scan(tmp_path, "clean.rs", "let x = compute(input);") == []


def test_cancellation_token_no_false_positive(tmp_path: Path) -> None:
    content = "use tokio_util::sync::CancellationToken;\nlet cancel = CancellationToken::new();"
    assert scan(tmp_path, "clean2.rs", content) == []


def test_task_prefix_no_sk_trigger(tmp_path: Path) -> None:
    assert scan(tmp_path, "task-prefix.rs", 'let s = "task-%s";' % TAIL40) == []


def test_bare_token_keyword_not_scanned(tmp_path: Path) -> None:
    assert scan(tmp_path, "bare-token.rs", 'let token = "cmd+shift+%s";' % TAIL12) == []


def test_short_quoted_value_passes(tmp_path: Path) -> None:
    assert scan(tmp_path, "short-quoted.toml", 'password = "123456"') == []


def test_short_unquoted_value_passes(tmp_path: Path) -> None:
    assert scan(tmp_path, "short-unquoted.env", "API_KEY=abc123") == []


# ---- 服务商密钥格式（拦截）----


def test_openai_key(tmp_path: Path) -> None:
    problems = scan(tmp_path, "leak-openai.txt", 'key = "sk-%s"' % TAIL40)
    assert any("suspected hardcoded secret" in p for p in problems)


def test_anthropic_key(tmp_path: Path) -> None:
    problems = scan(tmp_path, "leak-anthropic.txt", 'key = "sk-ant-%s"' % TAIL40)
    assert any("suspected hardcoded secret" in p for p in problems)


def test_aws_akia(tmp_path: Path) -> None:
    problems = scan(tmp_path, "leak-aws.txt", 'key = "AKIA%s"' % AWS_TAIL16)
    assert any("suspected hardcoded secret" in p for p in problems)


def test_github_classic_pat(tmp_path: Path) -> None:
    problems = scan(tmp_path, "leak-github.txt", 'key = "ghp_%s"' % TAIL36)
    assert any("suspected hardcoded secret" in p for p in problems)


def test_github_fine_grained_pat(tmp_path: Path) -> None:
    problems = scan(tmp_path, "leak-github-pat.txt", 'key = "github_pat_%s"' % TAIL20)
    assert any("suspected hardcoded secret" in p for p in problems)


def test_gitlab_pat(tmp_path: Path) -> None:
    problems = scan(tmp_path, "leak-gitlab.txt", 'key = "glpat-%s"' % TAIL20)
    assert any("suspected hardcoded secret" in p for p in problems)


def test_slack_token(tmp_path: Path) -> None:
    problems = scan(tmp_path, "leak-slack.txt", 'key = "xoxb-%s"' % TAIL20)
    assert any("suspected hardcoded secret" in p for p in problems)


def test_google_api_key(tmp_path: Path) -> None:
    problems = scan(tmp_path, "leak-google.txt", 'key = "AIza%s"' % TAIL35)
    assert any("suspected hardcoded secret" in p for p in problems)


def test_private_key_block(tmp_path: Path) -> None:
    # 夹具运行时拼接，避免字面量让门禁扫到本文件自命中
    header = "-----BEGIN %s-----" % "RSA PRIVATE KEY"
    footer = "-----END %s-----" % "RSA PRIVATE KEY"
    content = header + "\nabcdef\n" + footer
    problems = scan(tmp_path, "leak-key.pem", content)
    assert any("suspected hardcoded secret" in p for p in problems)


# ---- 通用凭据赋值（拦截）----


def test_quoted_api_key(tmp_path: Path) -> None:
    problems = scan(tmp_path, "leak-generic.toml", 'api_key = "%s"' % TAIL16)
    assert any("suspected hardcoded credential" in p for p in problems)


def test_unquoted_api_key(tmp_path: Path) -> None:
    problems = scan(tmp_path, "leak-generic.env", "API_KEY=%s" % TAIL20)
    assert any("suspected hardcoded credential" in p for p in problems)


def test_credential_colon_json(tmp_path: Path) -> None:
    problems = scan(tmp_path, "leak-generic.json", '"credential": "%s"' % TAIL16)
    assert any("suspected hardcoded credential" in p for p in problems)


def test_authorization_bearer(tmp_path: Path) -> None:
    problems = scan(tmp_path, "leak-bearer.txt", "authorization: Bearer %s" % TAIL20)
    assert any("suspected hardcoded credential" in p for p in problems)


# ---- secrets:allow 放行标记 ----


def test_marker_on_hit_line_passes(tmp_path: Path) -> None:
    content = 'api_key = "%s" # %s 测试夹具' % (TAIL16, MARKER)
    assert scan(tmp_path, "allow-ok.toml", content) == []


def test_marker_only_covers_own_line(tmp_path: Path) -> None:
    content = 'api_key = "%s" # %s 只放行本行\napi_key = "%s"' % (TAIL16, MARKER, TAIL20)
    problems = scan(tmp_path, "allow-partial.toml", content)
    assert any("suspected hardcoded credential" in p for p in problems)


def test_marker_on_clean_line_harmless(tmp_path: Path) -> None:
    assert scan(tmp_path, "allow-noise.txt", "let x = 1; # %s" % MARKER) == []


def test_bare_marker_on_hit_line_reports_allow_reason(tmp_path: Path) -> None:
    problems = scan(tmp_path, "bare-allow.toml", 'api_key = "%s" # %s' % (TAIL16, MARKER))
    assert any("allow marker without reason" in p for p in problems)
    assert not any("suspected hardcoded" in p for p in problems)


def test_bare_marker_elsewhere_than_own_line_still_flags_own_line(tmp_path: Path) -> None:
    content = 'api_key = "%s" # %s\napi_key = "%s"' % (TAIL16, MARKER, TAIL20)
    problems = scan(tmp_path, "bare-partial.toml", content)
    assert any("allow marker without reason" in p for p in problems)
    assert any("suspected hardcoded credential" in p for p in problems)


# ---- 模式清单与引擎参数（直接覆盖）----


def test_pattern_sources_unique() -> None:
    sources = [
        p.pattern
        for p in check_secrets.CASE_SENSITIVE_PATTERNS + check_secrets.CASE_INSENSITIVE_PATTERNS
    ]
    assert len(sources) == len(set(sources))


def test_case_groups_declare_flags() -> None:
    for p in check_secrets.CASE_SENSITIVE_PATTERNS:
        assert not p.flags & re.IGNORECASE
    for p in check_secrets.CASE_INSENSITIVE_PATTERNS:
        assert p.flags & re.IGNORECASE


def test_engine_expressions_carry_inline_flags() -> None:
    expressions = check_secrets.engine_expressions()
    sensitive = [p.pattern for p in check_secrets.CASE_SENSITIVE_PATTERNS]
    insensitive = [p.pattern for p in check_secrets.CASE_INSENSITIVE_PATTERNS]
    expanded = [s.replace("\\s", check_secrets.SPACE_SUPERSET) for s in insensitive]
    assert expressions[: len(sensitive)] == sensitive
    assert expressions[len(sensitive) :] == ["(?i)" + s for s in expanded]


def test_space_superset_covers_interpreter_whitespace() -> None:
    interpreter_space = re.compile(r"\s")
    superset = re.compile(check_secrets.SPACE_SUPERSET)
    for code in (0x1C, 0x1D, 0x1E, 0x1F, 0x09, 0x20):
        assert interpreter_space.search(chr(code))
        assert superset.search(chr(code))


def test_pattern_hits_drops_marker_lines() -> None:
    pattern = check_secrets.CASE_INSENSITIVE_PATTERNS[0]
    file_hits = {
        2: 'api_key = "%s"' % TAIL16,
        1: 'password = "%s"' % TAIL16,
        3: 'api_key = "%s" # %s' % (TAIL16, MARKER),
        4: "let x = compute(input);",
    }
    assert check_secrets.pattern_hits(pattern, file_hits) == [
        (1, 'password = "%s"' % TAIL16),
        (2, 'api_key = "%s"' % TAIL16),
    ]


# ---- 二进制与异常编码（引擎差异面）----


def test_binary_asset_file_skipped(tmp_path: Path) -> None:
    content = ('api_key = "%s"\n' % TAIL16).encode() + b"\x00\x01tail"
    p = tmp_path / "binary-asset.bin"
    p.write_bytes(content)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    assert check_secrets.run(str(tmp_path)) == []


def test_utf16_bom_file_scanned(tmp_path: Path) -> None:
    content = b"\xff\xfe junk\n" + ('api_key = "%s"\n' % TAIL16).encode()
    p = tmp_path / "bom-leak.txt"
    p.write_bytes(content)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    problems = check_secrets.run(str(tmp_path))
    assert any("suspected hardcoded credential" in q for q in problems)


def test_utf8_bom_file_scanned(tmp_path: Path) -> None:
    content = b"\xef\xbb\xbf" + ('api_key = "%s"\n' % TAIL16).encode()
    p = tmp_path / "utf8-bom-leak.txt"
    p.write_bytes(content)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    problems = check_secrets.run(str(tmp_path))
    assert any("suspected hardcoded credential" in q for q in problems)


def test_invalid_utf8_elsewhere_still_reported(tmp_path: Path) -> None:
    content = b"\x80 junk\n" + ('api_key = "%s"\n' % TAIL16).encode()
    p = tmp_path / "mixed-bad-utf8.txt"
    p.write_bytes(content)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    problems = check_secrets.run(str(tmp_path))
    assert any("suspected hardcoded credential" in q for q in problems)


def test_invalid_utf8_on_hit_line_reported(tmp_path: Path) -> None:
    content = ('api_key = "aa\xffa%s"\n' % TAIL16).encode("utf-8", "surrogateescape")
    p = tmp_path / "bad-utf8-hit.txt"
    p.write_bytes(content)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    problems = check_secrets.run(str(tmp_path))
    assert any("suspected hardcoded credential" in q for q in problems)


def test_crlf_line_reported(tmp_path: Path) -> None:
    problems = scan(tmp_path, "crlf-leak.toml", 'api_key = "%s"\r' % TAIL16)
    assert any("suspected hardcoded credential" in q for q in problems)


# ---- 受检面边界（工作区缺失与被忽略但已跟踪）----


def test_worktree_missing_file_skipped(tmp_path: Path) -> None:
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    p = tmp_path / "removed-leak.toml"
    p.write_text('api_key = "%s"\n' % TAIL16, encoding="utf-8")
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    os.remove(p)
    assert check_secrets.run(str(tmp_path)) == []


def test_ignored_but_tracked_file_scanned(tmp_path: Path) -> None:
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    (tmp_path / ".gitignore").write_text("forced-leak.txt\n", encoding="utf-8")
    p = tmp_path / "forced-leak.txt"
    p.write_text('api_key = "%s"\n' % TAIL16, encoding="utf-8")
    subprocess.run(
        ["git", "-C", str(tmp_path), "add", ".gitignore"], check=True, capture_output=True
    )
    subprocess.run(
        ["git", "-C", str(tmp_path), "add", "-f", "forced-leak.txt"],
        check=True,
        capture_output=True,
    )
    problems = check_secrets.run(str(tmp_path))
    assert any("suspected hardcoded credential" in q for q in problems)


# ---- 评审差异面：异常换行符与控制分隔符 ----


def seed_repo(tmp_path: Path, files: dict[str, str]) -> list[str]:
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    for name, text in files.items():
        (tmp_path / name).write_text(text + "\n", encoding="utf-8")
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    return check_secrets.run(str(tmp_path))


def test_exotic_line_break_in_value_reported(tmp_path: Path) -> None:
    files = {
        "leak-nel.txt": 'api_key = "%s"' % ("aaaabbbb" + chr(0x85) + TAIL16),
        "leak-ls.txt": 'api_key = "%s"' % ("aaaabbbb" + chr(0x2028) + TAIL16),
        "leak-ps.txt": 'api_key = "%s"' % ("aaaabbbb" + chr(0x2029) + TAIL16),
    }
    problems = seed_repo(tmp_path, files)
    flagged = {q.splitlines()[0].rsplit(": ", 1)[1] for q in problems}
    assert flagged == set(files)


def test_control_separator_assignment_reported(tmp_path: Path) -> None:
    files = {
        "leak-fs.txt": 'api_key%s=%s"aaaabbbbccccdddd"' % (chr(0x1C), chr(0x1C)),
        "leak-gs.txt": 'api_key%s=%s"aaaabbbbccccdddd"' % (chr(0x1D), chr(0x1D)),
        "leak-rs.txt": 'api_key%s=%s"aaaabbbbccccdddd"' % (chr(0x1E), chr(0x1E)),
        "leak-us.txt": 'api_key%s=%s"aaaabbbbccccdddd"' % (chr(0x1F), chr(0x1F)),
    }
    problems = seed_repo(tmp_path, files)
    flagged = {q.splitlines()[0].rsplit(": ", 1)[1] for q in problems}
    assert flagged == set(files)


# ---- 反馈契约：报告格式、汇总计数、确定性与退出码（子进程端到端）----

GATE = str(Path(__file__).with_name("check_secrets.py"))


def run_gate(
    tmp_path: Path, files: dict[str, str], *extra: str
) -> subprocess.CompletedProcess[bytes]:
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    for name, text in files.items():
        p = tmp_path / name
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text + "\n", encoding="utf-8")
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    return subprocess.run([sys.executable, GATE, str(tmp_path)] + list(extra), capture_output=True)


def violation_fixture() -> dict[str, str]:
    return {
        "leak-vendor.txt": 'key = "sk-%s"' % TAIL40,
        "leak-credential.toml": 'api_key = "%s"' % TAIL16,
        "leak-auth.txt": "authorization: Bearer %s" % TAIL20,
    }


def test_gate_reports_rule_ids_and_violation_exit_code(tmp_path: Path) -> None:
    proc = run_gate(tmp_path, violation_fixture())
    assert proc.returncode == 1
    text = proc.stderr.decode("utf-8")
    heads = [line for line in text.splitlines() if line.startswith("error[")]
    ids = {re.match(r"^error\[([A-Z0-9-]+)\]:", line).group(1) for line in heads}
    assert ids == {"SECRET-VENDOR", "SECRET-CREDENTIAL", "SECRET-AUTH"}
    for line in heads:
        assert re.match(r"^error\[[A-Z0-9-]+\]: .+ \(.+:\d+\)$", line)
    assert "    evidence: " in text
    assert "    help: " in text


def test_gate_summary_count_matches_printed_details(tmp_path: Path) -> None:
    proc = run_gate(tmp_path, violation_fixture())
    text = proc.stderr.decode("utf-8")
    details = sum(1 for line in text.splitlines() if line.startswith("error["))
    match = re.search(r"^error: (\d+) violations? found \(see above\)$", text, re.M)
    assert match
    assert int(match.group(1)) == details == 3


def test_gate_allow_reason_rule_id_and_exit_code(tmp_path: Path) -> None:
    proc = run_gate(tmp_path, {"bare-allow.toml": 'api_key = "%s" # %s' % (TAIL16, MARKER)})
    assert proc.returncode == 1
    text = proc.stderr.decode("utf-8")
    heads = [line for line in text.splitlines() if line.startswith("error[")]
    assert len(heads) == 1
    assert re.match(r"^error\[ALLOW-REASON\]: .+ \(.+:\d+\)$", heads[0])
    assert "    evidence: " in text
    assert "    help: " in text
    match = re.search(r"^error: (\d+) violations? found \(see above\)$", text, re.M)
    assert match
    assert int(match.group(1)) == 1


def test_gate_output_deterministic_across_runs(tmp_path: Path) -> None:
    first = run_gate(tmp_path, violation_fixture())
    second = subprocess.run([sys.executable, GATE, str(tmp_path)], capture_output=True)
    assert first.returncode == second.returncode == 1
    assert first.stderr == second.stderr
    assert first.stdout == second.stdout


def test_gate_clean_repo_passes_with_exit_zero(tmp_path: Path) -> None:
    proc = run_gate(tmp_path, {"clean.rs": "let x = compute(input);"})
    assert proc.returncode == 0
    assert proc.stdout.decode("utf-8").startswith("ok")


def test_gate_extra_argument_is_a_usage_error(tmp_path: Path) -> None:
    proc = run_gate(tmp_path, {"clean.rs": "let x = 1;"}, "extra-arg")
    assert proc.returncode == 2
    assert b"usage:" in proc.stderr


def test_gate_missing_root_is_a_usage_error(tmp_path: Path) -> None:
    proc = subprocess.run([sys.executable, GATE, str(tmp_path / "nowhere")], capture_output=True)
    assert proc.returncode == 2
    assert b"usage:" in proc.stderr


def test_gate_git_failure_is_infra_and_fail_closed(tmp_path: Path) -> None:
    bare = tmp_path / "not-a-repo"
    bare.mkdir()
    proc = subprocess.run([sys.executable, GATE, str(bare)], capture_output=True)
    assert proc.returncode == 3
    assert "error[INFRA]:" in proc.stderr.decode("utf-8")
