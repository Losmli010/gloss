"""密钥扫描门禁的单测：逐条覆盖服务商模式、通用凭据赋值与放行标记。

被测入口是门禁的运行函数，返回违规列表；在测试框架（pytest）的临时目录里建
一次性版本库作扫描面，端到端等价靠种子违规样本，匹配引擎要求正则检索工具
（ripgrep）在场。夹具里的疑似密钥一律由片段拼接生成，不让字面量出现在
本文件（否则门禁自命中）；用法见持续集成的钩子工作流。
"""

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


def scan(tmp_path, rel, content):
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    p = tmp_path / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(content + "\n", encoding="utf-8")
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    return check_secrets.run(str(tmp_path))


# ---- 干净文件（放行）----


def test_clean_code_passes(tmp_path):
    assert scan(tmp_path, "clean.rs", "let x = compute(input);") == []


def test_cancellation_token_no_false_positive(tmp_path):
    content = "use tokio_util::sync::CancellationToken;\nlet cancel = CancellationToken::new();"
    assert scan(tmp_path, "clean2.rs", content) == []


def test_task_prefix_no_sk_trigger(tmp_path):
    assert scan(tmp_path, "task-prefix.rs", 'let s = "task-%s";' % TAIL40) == []


def test_bare_token_keyword_not_scanned(tmp_path):
    assert scan(tmp_path, "bare-token.rs", 'let token = "cmd+shift+%s";' % TAIL12) == []


def test_short_quoted_value_passes(tmp_path):
    assert scan(tmp_path, "short-quoted.toml", 'password = "123456"') == []


def test_short_unquoted_value_passes(tmp_path):
    assert scan(tmp_path, "short-unquoted.env", "API_KEY=abc123") == []


# ---- 服务商密钥格式（拦截）----


def test_openai_key(tmp_path):
    problems = scan(tmp_path, "leak-openai.txt", 'key = "sk-%s"' % TAIL40)
    assert any("suspected hardcoded secret" in p for p in problems)


def test_anthropic_key(tmp_path):
    problems = scan(tmp_path, "leak-anthropic.txt", 'key = "sk-ant-%s"' % TAIL40)
    assert any("suspected hardcoded secret" in p for p in problems)


def test_aws_akia(tmp_path):
    problems = scan(tmp_path, "leak-aws.txt", 'key = "AKIA%s"' % AWS_TAIL16)
    assert any("suspected hardcoded secret" in p for p in problems)


def test_github_classic_pat(tmp_path):
    problems = scan(tmp_path, "leak-github.txt", 'key = "ghp_%s"' % TAIL36)
    assert any("suspected hardcoded secret" in p for p in problems)


def test_github_fine_grained_pat(tmp_path):
    problems = scan(tmp_path, "leak-github-pat.txt", 'key = "github_pat_%s"' % TAIL20)
    assert any("suspected hardcoded secret" in p for p in problems)


def test_gitlab_pat(tmp_path):
    problems = scan(tmp_path, "leak-gitlab.txt", 'key = "glpat-%s"' % TAIL20)
    assert any("suspected hardcoded secret" in p for p in problems)


def test_slack_token(tmp_path):
    problems = scan(tmp_path, "leak-slack.txt", 'key = "xoxb-%s"' % TAIL20)
    assert any("suspected hardcoded secret" in p for p in problems)


def test_google_api_key(tmp_path):
    problems = scan(tmp_path, "leak-google.txt", 'key = "AIza%s"' % TAIL35)
    assert any("suspected hardcoded secret" in p for p in problems)


def test_private_key_block(tmp_path):
    # 夹具运行时拼接，避免字面量让门禁扫到本文件自命中
    header = "-----BEGIN %s-----" % "RSA PRIVATE KEY"
    footer = "-----END %s-----" % "RSA PRIVATE KEY"
    content = header + "\nabcdef\n" + footer
    problems = scan(tmp_path, "leak-key.pem", content)
    assert any("suspected hardcoded secret" in p for p in problems)


# ---- 通用凭据赋值（拦截）----


def test_quoted_api_key(tmp_path):
    problems = scan(tmp_path, "leak-generic.toml", 'api_key = "%s"' % TAIL16)
    assert any("suspected hardcoded credential" in p for p in problems)


def test_unquoted_api_key(tmp_path):
    problems = scan(tmp_path, "leak-generic.env", "API_KEY=%s" % TAIL20)
    assert any("suspected hardcoded credential" in p for p in problems)


def test_credential_colon_json(tmp_path):
    problems = scan(tmp_path, "leak-generic.json", '"credential": "%s"' % TAIL16)
    assert any("suspected hardcoded credential" in p for p in problems)


def test_authorization_bearer(tmp_path):
    problems = scan(tmp_path, "leak-bearer.txt", "authorization: Bearer %s" % TAIL20)
    assert any("suspected hardcoded credential" in p for p in problems)


# ---- secrets:allow 放行标记 ----


def test_marker_on_hit_line_passes(tmp_path):
    content = 'api_key = "%s" # %s' % (TAIL16, MARKER)
    assert scan(tmp_path, "allow-ok.toml", content) == []


def test_marker_only_covers_own_line(tmp_path):
    content = 'api_key = "%s" # %s\napi_key = "%s"' % (TAIL16, MARKER, TAIL20)
    problems = scan(tmp_path, "allow-partial.toml", content)
    assert any("suspected hardcoded credential" in p for p in problems)


def test_marker_on_clean_line_harmless(tmp_path):
    assert scan(tmp_path, "allow-noise.txt", "let x = 1; # %s" % MARKER) == []


# ---- 模式清单与引擎参数（直接覆盖）----


def test_pattern_sources_unique():
    sources = [
        p.pattern
        for p in check_secrets.CASE_SENSITIVE_PATTERNS + check_secrets.CASE_INSENSITIVE_PATTERNS
    ]
    assert len(sources) == len(set(sources))


def test_case_groups_declare_flags():
    for p in check_secrets.CASE_SENSITIVE_PATTERNS:
        assert not p.flags & re.IGNORECASE
    for p in check_secrets.CASE_INSENSITIVE_PATTERNS:
        assert p.flags & re.IGNORECASE


def test_engine_expressions_carry_inline_flags():
    expressions = check_secrets.engine_expressions()
    sensitive = [p.pattern for p in check_secrets.CASE_SENSITIVE_PATTERNS]
    insensitive = [p.pattern for p in check_secrets.CASE_INSENSITIVE_PATTERNS]
    assert expressions[: len(sensitive)] == sensitive
    assert expressions[len(sensitive) :] == ["(?i)" + s for s in insensitive]


def test_pattern_hits_drops_marker_lines():
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


def test_binary_asset_file_skipped(tmp_path):
    content = ('api_key = "%s"\n' % TAIL16).encode() + b"\x00\x01tail"
    p = tmp_path / "binary-asset.bin"
    p.write_bytes(content)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    assert check_secrets.run(str(tmp_path)) == []


def test_utf16_bom_file_scanned(tmp_path):
    content = b"\xff\xfe junk\n" + ('api_key = "%s"\n' % TAIL16).encode()
    p = tmp_path / "bom-leak.txt"
    p.write_bytes(content)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    problems = check_secrets.run(str(tmp_path))
    assert any("suspected hardcoded credential" in q for q in problems)


def test_utf8_bom_file_scanned(tmp_path):
    content = b"\xef\xbb\xbf" + ('api_key = "%s"\n' % TAIL16).encode()
    p = tmp_path / "utf8-bom-leak.txt"
    p.write_bytes(content)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    problems = check_secrets.run(str(tmp_path))
    assert any("suspected hardcoded credential" in q for q in problems)


def test_invalid_utf8_elsewhere_still_reported(tmp_path):
    content = b"\x80 junk\n" + ('api_key = "%s"\n' % TAIL16).encode()
    p = tmp_path / "mixed-bad-utf8.txt"
    p.write_bytes(content)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    problems = check_secrets.run(str(tmp_path))
    assert any("suspected hardcoded credential" in q for q in problems)


def test_invalid_utf8_on_hit_line_reported(tmp_path):
    content = ('api_key = "aa\xffa%s"\n' % TAIL16).encode("utf-8", "surrogateescape")
    p = tmp_path / "bad-utf8-hit.txt"
    p.write_bytes(content)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    problems = check_secrets.run(str(tmp_path))
    assert any("suspected hardcoded credential" in q for q in problems)


def test_crlf_line_reported(tmp_path):
    problems = scan(tmp_path, "crlf-leak.toml", 'api_key = "%s"\r' % TAIL16)
    assert any("suspected hardcoded credential" in q for q in problems)


# ---- 受检面边界（工作区缺失与被忽略但已跟踪）----


def test_worktree_missing_file_skipped(tmp_path):
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    p = tmp_path / "removed-leak.toml"
    p.write_text('api_key = "%s"\n' % TAIL16, encoding="utf-8")
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    os.remove(p)
    assert check_secrets.run(str(tmp_path)) == []


def test_ignored_but_tracked_file_scanned(tmp_path):
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
