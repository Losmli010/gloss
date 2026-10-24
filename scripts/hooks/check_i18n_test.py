"""文案表门禁的单测：逐条覆盖字面量词法状态机、装入点集中与词条表唯一。

被测入口是门禁的运行函数，返回通过与失败两个列表；夹具建在测试框架（pytest）
的临时目录，不落版本库（扫描面走目录遍历）；用法见持续集成的钩子工作流。
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import check_i18n  # noqa: E402

MARKER = "i18n:allow"


def scan(tmp_path, rel, content, expect=None):
    """写入夹具文件并运行门禁；expect 为期望出现在输出里的片段（可选）。

    返回 (oks, fails)。基线（owner 装入点与词条表）每次重建。"""
    base = tmp_path / "crates/gloss-app/src/ui"
    catalog = tmp_path / "crates/gloss-app/i18n"
    base.mkdir(parents=True, exist_ok=True)
    catalog.mkdir(parents=True, exist_ok=True)
    (base / "i18n.rs").write_text('const ZH: &str = include_str!("../../i18n/zh.toml");\n')
    (catalog / "zh.toml").write_text('gloss_app_title = "Gloss"\n')

    p = tmp_path / rel
    if rel:
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(content + "\n" if content else "", encoding="utf-8")

    oks, fails = check_i18n.run(str(tmp_path))
    out = "\n".join(oks + fails)
    if expect is not None:
        assert expect in out
    return oks, fails


def assert_pass(tmp_path, rel, content):
    _, fails = scan(tmp_path, rel, content)
    assert fails == []


def assert_fail(tmp_path, rel, content, expect=None):
    _, fails = scan(tmp_path, rel, content)
    assert fails
    if expect is not None:
        out = "\n".join(fails)
        assert expect in out


# ---- 基线与非判罚面（放行）----


def test_baseline_owner_and_catalog(tmp_path):
    assert_pass(tmp_path, "crates/gloss-app/src/.keep", "")


def test_chinese_comments_allowed(tmp_path):
    content = "// 中文行注释\n/// 中文文档注释\nlet x = 1;"
    assert_pass(tmp_path, "crates/gloss-app/src/comments.rs", content)


def test_nested_block_comment_allowed(tmp_path):
    content = "/* 外层注释 /* 内层中文 */ 仍是注释 */\nfn main() { let x = 1; }"
    assert_pass(tmp_path, "crates/gloss-app/src/block.rs", content)


def test_multiline_attribute_reason_allowed(tmp_path):
    content = (
        "#[allow(\n    clippy::too_many_arguments,\n"
        '    reason = "组装点的主入口：字段袋说明"\n)]\npub fn run() {}'
    )
    assert_pass(tmp_path, "crates/gloss-app/src/attr.rs", content)


def test_cfg_test_mod_chinese_assertions_allowed(tmp_path):
    content = (
        "pub fn add(a: u32, b: u32) -> u32 { a + b }\n\n#[cfg(test)]\n"
        "mod tests {\n    #[test]\n    fn t() {\n"
        '        assert_eq!(1, 1, "中文断言");\n    }\n}'
    )
    assert_pass(tmp_path, "crates/gloss-app/src/with_tests.rs", content)


def test_cfg_test_pub_crate_mod_allowed(tmp_path):
    content = (
        "pub fn visible() -> u32 { 1 }\n\n#[cfg(test)]\n"
        "pub(crate) mod support {\n    pub fn need() {\n"
        '        panic!("中文指引");\n    }\n}'
    )
    assert_pass(tmp_path, "crates/gloss-app/src/support.rs", content)


def test_nested_cfg_test_mods_allowed(tmp_path):
    content = (
        "pub fn visible() -> u32 { 1 }\n\n#[cfg(test)]\nmod outer {\n"
        "    use super::*;\n\n    #[cfg(test)]\n    mod inner {\n"
        "        #[test]\n        fn t() {\n"
        '            assert_eq!(1, 1, "内层中文断言");\n        }\n    }\n\n'
        "    #[test]\n    fn u() {\n"
        '        assert!(true, "外层中文断言");\n    }\n}'
    )
    assert_pass(tmp_path, "crates/gloss-app/src/nested_tests.rs", content)


def test_inner_attribute_reason_allowed(tmp_path):
    content = (
        '#![allow(clippy::too_many_arguments, reason = "内联属性理由：字段袋说明")]\nfn main() {}'
    )
    assert_pass(tmp_path, "crates/gloss-app/src/inner_attr.rs", content)


def test_lifetimes_and_ascii_char_allowed(tmp_path):
    content = (
        "fn pick<'a>(a: &'a str, b: &'a str) -> &'a str "
        "{ if a.len() > b.len() { a } else { b } }\n"
        "let c = 'x';\nlet s: &'static str = \"ok\";"
    )
    assert_pass(tmp_path, "crates/gloss-app/src/lifetimes.rs", content)


def test_allow_marker_on_hit_line(tmp_path):
    content = 'let gear = icon_button("⚙"); // %s 图标字形，非 locale 文案\n' % MARKER
    assert_pass(tmp_path, "crates/gloss-app/src/allow_ok.rs", content)


# ---- 文案字面量（拦截）----


def test_hardcoded_chinese_string(tmp_path):
    content = 'fn label() -> String {\n    "打开设置".to_owned()\n}'
    assert_fail(tmp_path, "crates/gloss-app/src/hardcoded.rs", content)


def test_raw_string_chinese(tmp_path):
    assert_fail(tmp_path, "crates/gloss-app/src/raw.rs", 'let s = r#"中文正文"#;')


def test_char_literal_chinese(tmp_path):
    assert_fail(tmp_path, "crates/gloss-app/src/charlit.rs", "let seal = '疏';")


def test_marker_only_covers_own_line(tmp_path):
    content = 'let a = "放大"; // %s 合法字形\nlet b = "打开设置";' % MARKER
    assert_fail(tmp_path, "crates/gloss-app/src/allow_partial.rs", content)


def test_root_src_also_scanned(tmp_path):
    assert_fail(tmp_path, "src/main.rs", 'fn main() {\n    let _ = "入口文案";\n}')


# ---- 词法边界 ----


def test_production_code_after_test_mod_scanned(tmp_path):
    content = (
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {\n"
        '        assert_eq!(1, 1, "中文断言");\n    }\n}\n\n'
        'let _ = "模块之后的生产文案";'
    )
    assert_fail(tmp_path, "crates/gloss-app/src/late.rs", content)


def test_url_in_string_not_eaten_as_comment(tmp_path):
    content = 'let url = "https://example.com/a";\nlet s = "中文";'
    assert_fail(tmp_path, "crates/gloss-app/src/url.rs", content)


def test_string_continuation_keeps_line_numbers(tmp_path):
    content = 'let a = "no chinese \\\nstill ascii";\n\nlet s = "打开设置";'
    assert_fail(tmp_path, "crates/gloss-app/src/continuation.rs", content, "continuation.rs line 4")


def test_bracket_in_attribute_string_ok(tmp_path):
    content = '#[allow(clippy::doc_markdown, reason = "数组 [0] 说明")]\nlet x = 1;'
    assert_pass(tmp_path, "crates/gloss-app/src/bracket.rs", content)


# ---- 扩展扫描面：核心库（gloss-core）与平台适配层（gloss-platform） ----


def test_core_hardcoded_chinese(tmp_path):
    content = 'fn label() -> String {\n    "未能读取选区".to_owned()\n}'
    assert_fail(tmp_path, "crates/gloss-core/src/hardcoded.rs", content)


def test_platform_hardcoded_chinese(tmp_path):
    content = 'pub fn hint() -> &\'static str {\n    "打开设置"\n}'
    assert_fail(tmp_path, "crates/gloss-platform/src/hardcoded.rs", content)


def test_prompt_rs_whole_file_exempt(tmp_path):
    content = 'fn lang_name() -> &\'static str {\n    "中文"\n}'
    assert_pass(tmp_path, "crates/gloss-core/src/prompt.rs", content)


def test_core_allow_marker(tmp_path):
    content = 'const DOTS: &str = "…"; // %s 诊断省略号，非 locale 文案\n' % MARKER
    assert_pass(tmp_path, "crates/gloss-core/src/allowed.rs", content)


# ---- 装入点集中与词条表唯一（拦截）----


def test_include_str_outside_owner(tmp_path):
    content = 'const P: &str = include_str!("../i18n/zh.toml");'
    assert_fail(tmp_path, "crates/gloss-core/src/alias.rs", content, "alias.rs")


def test_include_bytes_outside_owner(tmp_path):
    content = 'const B: &[u8] = include_bytes!("../i18n/zh.toml");'
    assert_fail(tmp_path, "crates/gloss-core/src/blob.rs", content, "blob.rs")


def test_catalog_key_outside_catalog_dir(tmp_path):
    assert_fail(tmp_path, "extra.toml", 'gloss_extra_foo = "x"')


def test_plain_toml_key_allowed(tmp_path):
    assert_pass(tmp_path, "plain.toml", 'app_title = "Gloss"')
