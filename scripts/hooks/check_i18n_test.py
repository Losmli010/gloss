"""文案表门禁的单测：逐条覆盖字面量词法状态机、装入点集中、词条表唯一与
词条语言规范。

被测入口是门禁的运行函数，返回通过与失败两个列表；夹具建在测试框架（pytest）
的临时目录，不落版本库（扫描面走目录遍历）；用法见持续集成的钩子工作流。
字面量扫描面的四个源码目录是守卫基线，缺席即失败关闭退出。另含反馈契约
三件套：报告格式断言、汇总计数与确定性，经子进程跑脚本端到端锁输出与退出码。
"""

from __future__ import annotations

import re
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).parent))

import check_i18n  # noqa: E402
import gate_report  # noqa: E402

MARKER = "i18n:allow"

GUARD_BASES = ("src", "crates/gloss-core/src", "crates/gloss-platform/src")


def make_baseline(tmp_path: Path) -> None:
    """重建守卫基线：四个源码目录加统一装入点与中英两份词条表。"""
    for d in GUARD_BASES:
        (tmp_path / d).mkdir(parents=True, exist_ok=True)
    base = tmp_path / "crates/gloss-app/src/ui"
    catalog = tmp_path / "crates/gloss-app/i18n"
    base.mkdir(parents=True, exist_ok=True)
    catalog.mkdir(parents=True, exist_ok=True)
    (base / "i18n.rs").write_text('const ZH: &str = include_str!("../../i18n/zh.toml");\n')
    (catalog / "zh.toml").write_text('gloss_app_title = "词汇"\n')
    (catalog / "en.toml").write_text('gloss_app_title = "Gloss"\n')


def scan(
    tmp_path: Path, rel: str, content: str, expect: str | None = None
) -> tuple[list[str], list[str]]:
    """写入夹具文件并运行门禁；expect 为期望出现在输出里的片段（可选）。

    返回 (oks, fails)。基线（四个源码目录、owner 装入点与词条表）每次重建。"""
    make_baseline(tmp_path)

    p = tmp_path / rel
    if rel:
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(content + "\n" if content else "", encoding="utf-8")

    oks, fails = check_i18n.run(str(tmp_path))
    out = "\n".join(oks + fails)
    if expect is not None:
        assert expect in out
    return oks, fails


def assert_pass(tmp_path: Path, rel: str, content: str) -> None:
    _, fails = scan(tmp_path, rel, content)
    assert fails == []


def assert_fail(tmp_path: Path, rel: str, content: str, expect: str | None = None) -> None:
    _, fails = scan(tmp_path, rel, content)
    assert fails
    if expect is not None:
        out = "\n".join(fails)
        assert expect in out


# ---- 基线与非判罚面（放行）----


def test_baseline_owner_and_catalog(tmp_path: Path) -> None:
    assert_pass(tmp_path, "crates/gloss-app/src/.keep", "")


def test_chinese_comments_allowed(tmp_path: Path) -> None:
    content = "// 中文行注释\n/// 中文文档注释\nlet x = 1;"
    assert_pass(tmp_path, "crates/gloss-app/src/comments.rs", content)


def test_nested_block_comment_allowed(tmp_path: Path) -> None:
    content = "/* 外层注释 /* 内层中文 */ 仍是注释 */\nfn main() { let x = 1; }"
    assert_pass(tmp_path, "crates/gloss-app/src/block.rs", content)


def test_multiline_attribute_reason_allowed(tmp_path: Path) -> None:
    content = (
        "#[allow(\n    clippy::too_many_arguments,\n"
        '    reason = "组装点的主入口：字段袋说明"\n)]\npub fn run() {}'
    )
    assert_pass(tmp_path, "crates/gloss-app/src/attr.rs", content)


def test_cfg_test_mod_chinese_assertions_allowed(tmp_path: Path) -> None:
    content = (
        "pub fn add(a: u32, b: u32) -> u32 { a + b }\n\n#[cfg(test)]\n"
        "mod tests {\n    #[test]\n    fn t() {\n"
        '        assert_eq!(1, 1, "中文断言");\n    }\n}'
    )
    assert_pass(tmp_path, "crates/gloss-app/src/with_tests.rs", content)


def test_cfg_test_pub_crate_mod_allowed(tmp_path: Path) -> None:
    content = (
        "pub fn visible() -> u32 { 1 }\n\n#[cfg(test)]\n"
        "pub(crate) mod support {\n    pub fn need() {\n"
        '        panic!("中文指引");\n    }\n}'
    )
    assert_pass(tmp_path, "crates/gloss-app/src/support.rs", content)


def test_nested_cfg_test_mods_allowed(tmp_path: Path) -> None:
    content = (
        "pub fn visible() -> u32 { 1 }\n\n#[cfg(test)]\nmod outer {\n"
        "    use super::*;\n\n    #[cfg(test)]\n    mod inner {\n"
        "        #[test]\n        fn t() {\n"
        '            assert_eq!(1, 1, "内层中文断言");\n        }\n    }\n\n'
        "    #[test]\n    fn u() {\n"
        '        assert!(true, "外层中文断言");\n    }\n}'
    )
    assert_pass(tmp_path, "crates/gloss-app/src/nested_tests.rs", content)


def test_inner_attribute_reason_allowed(tmp_path: Path) -> None:
    content = (
        '#![allow(clippy::too_many_arguments, reason = "内联属性理由：字段袋说明")]\nfn main() {}'
    )
    assert_pass(tmp_path, "crates/gloss-app/src/inner_attr.rs", content)


def test_lifetimes_and_ascii_char_allowed(tmp_path: Path) -> None:
    content = (
        "fn pick<'a>(a: &'a str, b: &'a str) -> &'a str "
        "{ if a.len() > b.len() { a } else { b } }\n"
        "let c = 'x';\nlet s: &'static str = \"ok\";"
    )
    assert_pass(tmp_path, "crates/gloss-app/src/lifetimes.rs", content)


def test_allow_marker_on_hit_line(tmp_path: Path) -> None:
    content = 'let gear = icon_button("⚙"); // %s 图标字形，非 locale 文案\n' % MARKER
    assert_pass(tmp_path, "crates/gloss-app/src/allow_ok.rs", content)


def test_bare_marker_on_hit_line_reports_allow_reason(tmp_path: Path) -> None:
    _, fails = scan(
        tmp_path, "crates/gloss-app/src/bare_allow.rs", 'let s = "打开"; // %s' % MARKER
    )
    out = "\n".join(fails)
    assert "marker without a reason" in out
    assert "embeds a copy literal" not in out


# ---- 文案字面量（拦截）----


def test_hardcoded_chinese_string(tmp_path: Path) -> None:
    content = 'fn label() -> String {\n    "打开设置".to_owned()\n}'
    assert_fail(tmp_path, "crates/gloss-app/src/hardcoded.rs", content)


def test_raw_string_chinese(tmp_path: Path) -> None:
    assert_fail(tmp_path, "crates/gloss-app/src/raw.rs", 'let s = r#"中文正文"#;')


def test_char_literal_chinese(tmp_path: Path) -> None:
    assert_fail(tmp_path, "crates/gloss-app/src/charlit.rs", "let seal = '疏';")


def test_marker_only_covers_own_line(tmp_path: Path) -> None:
    content = 'let a = "放大"; // %s 合法字形\nlet b = "打开设置";' % MARKER
    assert_fail(tmp_path, "crates/gloss-app/src/allow_partial.rs", content)


def test_root_src_also_scanned(tmp_path: Path) -> None:
    assert_fail(tmp_path, "src/main.rs", 'fn main() {\n    let _ = "入口文案";\n}')


# ---- 词法边界 ----


def test_production_code_after_test_mod_scanned(tmp_path: Path) -> None:
    content = (
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {\n"
        '        assert_eq!(1, 1, "中文断言");\n    }\n}\n\n'
        'let _ = "模块之后的生产文案";'
    )
    assert_fail(tmp_path, "crates/gloss-app/src/late.rs", content)


def test_url_in_string_not_eaten_as_comment(tmp_path: Path) -> None:
    content = 'let url = "https://example.com/a";\nlet s = "中文";'
    assert_fail(tmp_path, "crates/gloss-app/src/url.rs", content)


def test_string_continuation_keeps_line_numbers(tmp_path: Path) -> None:
    content = 'let a = "no chinese \\\nstill ascii";\n\nlet s = "打开设置";'
    assert_fail(tmp_path, "crates/gloss-app/src/continuation.rs", content, "continuation.rs line 4")


def test_bracket_in_attribute_string_ok(tmp_path: Path) -> None:
    content = '#[allow(clippy::doc_markdown, reason = "数组 [0] 说明")]\nlet x = 1;'
    assert_pass(tmp_path, "crates/gloss-app/src/bracket.rs", content)


# ---- 扩展扫描面：核心库（gloss-core）与平台适配层（gloss-platform） ----


def test_core_hardcoded_chinese(tmp_path: Path) -> None:
    content = 'fn label() -> String {\n    "未能读取选区".to_owned()\n}'
    assert_fail(tmp_path, "crates/gloss-core/src/hardcoded.rs", content)


def test_platform_hardcoded_chinese(tmp_path: Path) -> None:
    content = 'pub fn hint() -> &\'static str {\n    "打开设置"\n}'
    assert_fail(tmp_path, "crates/gloss-platform/src/hardcoded.rs", content)


def test_prompt_rs_whole_file_exempt(tmp_path: Path) -> None:
    content = 'fn lang_name() -> &\'static str {\n    "中文"\n}'
    assert_pass(tmp_path, "crates/gloss-core/src/prompt.rs", content)


def test_core_allow_marker(tmp_path: Path) -> None:
    content = 'const DOTS: &str = "…"; // %s 诊断省略号，非 locale 文案\n' % MARKER
    assert_pass(tmp_path, "crates/gloss-core/src/allowed.rs", content)


# ---- 装入点集中与词条表唯一（拦截）----


def test_include_str_outside_owner(tmp_path: Path) -> None:
    content = 'const P: &str = include_str!("../i18n/zh.toml");'
    assert_fail(tmp_path, "crates/gloss-core/src/alias.rs", content, "alias.rs")


def test_include_bytes_outside_owner(tmp_path: Path) -> None:
    content = 'const B: &[u8] = include_bytes!("../i18n/zh.toml");'
    assert_fail(tmp_path, "crates/gloss-core/src/blob.rs", content, "blob.rs")


def test_catalog_key_outside_catalog_dir(tmp_path: Path) -> None:
    assert_fail(tmp_path, "extra.toml", 'gloss_extra_foo = "x"')


def test_plain_toml_key_allowed(tmp_path: Path) -> None:
    assert_pass(tmp_path, "plain.toml", 'app_title = "Gloss"')


# ---- 词条语言规范（拦截）----


def test_zh_entry_written_in_english_is_flagged(tmp_path: Path) -> None:
    assert_fail(
        tmp_path,
        "crates/gloss-app/i18n/zh.toml",
        'gloss_popup_retry = "Retry"',
        "the zh entry must be written in Chinese",
    )


def test_en_entry_with_chinese_is_flagged(tmp_path: Path) -> None:
    assert_fail(
        tmp_path,
        "crates/gloss-app/i18n/en.toml",
        'gloss_popup_retry = "重试"',
        "the en entry must be pure English",
    )


def test_en_entry_with_fullwidth_or_curly_is_flagged(tmp_path: Path) -> None:
    assert_fail(
        tmp_path,
        "crates/gloss-app/i18n/en.toml",
        'gloss_hint = "pick “Save” here"',
        "the en entry must be pure English",
    )


def test_self_named_language_entry_in_zh_is_flagged(tmp_path: Path) -> None:
    assert_fail(
        tmp_path,
        "crates/gloss-app/i18n/zh.toml",
        'gloss_app_title = "词汇"\ngloss_ui_language_en = "English"\n',
        "the zh entry must be written in Chinese",
    )


def test_zh_entry_with_latin_self_name_in_parentheses_passes(tmp_path: Path) -> None:
    content = 'gloss_app_title = "词汇"\ngloss_ui_language_en = "英语（English）"\n'
    assert_pass(tmp_path, "crates/gloss-app/i18n/zh.toml", content)


def test_catalog_comments_and_blanks_are_not_entries(tmp_path: Path) -> None:
    content = '# 词条说明\n\ngloss_app_title = "词汇"\n'
    assert_pass(tmp_path, "crates/gloss-app/i18n/zh.toml", content)


def test_language_rule_reports_clean_ok_line_with_entry_count(tmp_path: Path) -> None:
    oks, fails = scan(tmp_path, "", "")
    assert fails == []
    assert "entries language-checked" in "\n".join(oks)


def test_missing_catalog_file_fails_closed(tmp_path: Path) -> None:
    make_baseline(tmp_path)
    (tmp_path / "crates/gloss-app/i18n/en.toml").unlink()
    with pytest.raises(SystemExit) as exc:
        check_i18n.run(str(tmp_path))
    assert exc.value.code == gate_report.EXIT_INFRA


# ---- 目录缺失守卫（失败关闭）----


def test_missing_scan_dir_fails_closed(tmp_path: Path) -> None:
    base = tmp_path / "crates/gloss-app/src/ui"
    catalog = tmp_path / "crates/gloss-app/i18n"
    base.mkdir(parents=True)
    catalog.mkdir(parents=True)
    (base / "i18n.rs").write_text('const ZH: &str = include_str!("../../i18n/zh.toml");\n')
    (catalog / "zh.toml").write_text('gloss_app_title = "Gloss"\n')
    with pytest.raises(SystemExit) as exc:
        check_i18n.run(str(tmp_path))
    assert exc.value.code == gate_report.EXIT_INFRA


# ---- 反馈契约：报告格式、汇总计数、确定性与退出码（子进程端到端）----

GATE = str(Path(__file__).with_name("check_i18n.py"))


def run_gate(
    tmp_path: Path, files: dict[str, str], *extra: str
) -> subprocess.CompletedProcess[bytes]:
    make_baseline(tmp_path)
    for name, text in files.items():
        p = tmp_path / name
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text + "\n", encoding="utf-8")
    return subprocess.run([sys.executable, GATE, str(tmp_path)] + list(extra), capture_output=True)


def violation_fixture() -> dict[str, str]:
    return {
        "crates/gloss-app/src/hardcoded.rs": 'fn label() -> String {\n    "打开设置".to_owned()\n}',
        "crates/gloss-core/src/alias.rs": 'const P: &str = include_str!("../i18n/zh.toml");',
        "extra.toml": 'gloss_extra_foo = "x"',
    }


def test_gate_reports_rule_ids_and_violation_exit_code(tmp_path: Path) -> None:
    proc = run_gate(tmp_path, violation_fixture())
    assert proc.returncode == 1
    text = proc.stderr.decode("utf-8")
    heads = [line for line in text.splitlines() if line.startswith("error[")]
    ids = {re.match(r"^error\[([A-Z0-9-]+)\]:", line).group(1) for line in heads}
    assert ids == {"I18N-LITERAL", "I18N-LOADPOINT", "I18N-CATALOG"}
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
    proc = run_gate(tmp_path, {"crates/gloss-app/src/bare.rs": 'let s = "打开"; // %s' % MARKER})
    assert proc.returncode == 1
    text = proc.stderr.decode("utf-8")
    heads = [line for line in text.splitlines() if line.startswith("error[")]
    assert len(heads) == 1
    assert re.match(r"^error\[ALLOW-REASON\]: .+ \(.+:\d+\)$", heads[0])
    match = re.search(r"^error: (\d+) violations? found \(see above\)$", text, re.M)
    assert match
    assert int(match.group(1)) == 1


def test_gate_output_deterministic_across_runs(tmp_path: Path) -> None:
    first = run_gate(tmp_path, violation_fixture())
    second = subprocess.run([sys.executable, GATE, str(tmp_path)], capture_output=True)
    assert first.returncode == second.returncode == 1
    assert first.stderr == second.stderr
    assert first.stdout == second.stdout


def test_gate_clean_fixture_passes_with_exit_zero(tmp_path: Path) -> None:
    proc = run_gate(tmp_path, {"crates/gloss-app/src/clean.rs": "let x = compute(input);"})
    assert proc.returncode == 0
    assert "  ok: " in proc.stdout.decode("utf-8")


def test_gate_missing_scan_dir_fails_closed_with_exit_three(tmp_path: Path) -> None:
    make_baseline(tmp_path)
    shutil.rmtree(tmp_path / "crates/gloss-platform/src")
    proc = subprocess.run([sys.executable, GATE, str(tmp_path)], capture_output=True)
    assert proc.returncode == 3
    assert "error[EMPTY-SURFACE]:" in proc.stderr.decode("utf-8")


def test_gate_extra_argument_is_a_usage_error(tmp_path: Path) -> None:
    proc = run_gate(tmp_path, {}, "extra-arg")
    assert proc.returncode == 2
    assert b"usage:" in proc.stderr


def test_gate_missing_root_is_a_usage_error(tmp_path: Path) -> None:
    proc = subprocess.run([sys.executable, GATE, str(tmp_path / "nowhere")], capture_output=True)
    assert proc.returncode == 2
    assert b"usage:" in proc.stderr
