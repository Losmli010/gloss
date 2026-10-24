"""check_comments.py 门禁的单测：逐条覆盖判罚规则与豁免路径。

被测函数是 check_comments.run(root, explicit)（返回违规列表），不落进程；
样本文件写入 pytest 的 tmp_path，互不串扰。用法：
    python3 -m pytest scripts/hooks/check_comments_test.py
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import check_comments

HEADER = "//! 中文文件头：负责演示。"


def write_sample(tmp_path, content, rel="crates/demo/src/sample.rs"):
    p = tmp_path / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(content, encoding="utf-8")
    return rel


def run_gate(tmp_path, content, rel="crates/demo/src/sample.rs"):
    write_sample(tmp_path, content, rel)
    return check_comments.run(str(tmp_path), [rel])


# ---- 文件头与干净样本 ----


def test_clean_file_passes(tmp_path):
    assert run_gate(tmp_path, f"{HEADER}\n\nfn main() {{ let _ = 1; }}\n") == []


def test_missing_header_flagged(tmp_path):
    problems = run_gate(tmp_path, "fn main() {}\n")
    assert any("缺文件头" in p for p in problems)


def test_english_only_header_flagged(tmp_path):
    problems = run_gate(tmp_path, "//! English only header line.\n\nfn main() {}\n")
    assert any("无汉字" in p for p in problems)


def test_bare_english_words_flagged(tmp_path):
    problems = run_gate(tmp_path, "//! 中文头说明 functional core 在此。\n\nfn main() {}\n")
    assert any("英文词" in p for p in problems)


def test_single_english_word_flagged(tmp_path):
    problems = run_gate(tmp_path, "//! 中文头借用 helper 渲染。\n\nfn main() {}\n")
    assert any("英文词" in p for p in problems)


def test_proper_noun_allowed(tmp_path):
    content = "//! 中文头借用 wgpu 渲染。\n\nfn main() {}\n"
    assert run_gate(tmp_path, content) == []


def test_backtick_code_segment_flagged(tmp_path):
    problems = run_gate(tmp_path, "//! 中文头：术语 `functional core` 见记录。\n\nfn main() {}\n")
    assert any("反引号代码段" in p for p in problems)


def test_doc_reference_flagged(tmp_path):
    problems = run_gate(tmp_path, "//! 中文头：规则见 AGENTS.md。\n\nfn main() {}\n")
    assert any("英文词" in p for p in problems)


def test_section_reference_flagged(tmp_path):
    problems = run_gate(tmp_path, "//! 中文头：规格见分发设计 §4.1。\n\nfn main() {}\n")
    assert any("章节号" in p for p in problems)


def test_url_inside_string_not_flagged(tmp_path):
    content = (
        f'{HEADER}\n\nconst URL: &str = "https://example.com/a//b";\nfn main() {{ let _ = URL; }}\n'
    )
    assert run_gate(tmp_path, content) == []


# ---- 违规注释形态 ----


def test_doc_comment_flagged(tmp_path):
    problems = run_gate(tmp_path, f"{HEADER}\n\n/// 说明文字\nfn main() {{}}\n")
    assert any("/// 文档注释" in p for p in problems)


def test_inline_comment_flagged(tmp_path):
    problems = run_gate(tmp_path, f"{HEADER}\n\n// 说明\nfn main() {{}}\n")
    assert any("行内注释" in p for p in problems)


def test_trailing_comment_flagged(tmp_path):
    problems = run_gate(tmp_path, f"{HEADER}\n\nfn main() {{ let _ = 1; // 尾注\n}}\n")
    assert any("行尾注释" in p for p in problems)


def test_block_comment_flagged(tmp_path):
    problems = run_gate(tmp_path, f"{HEADER}\n\n/* 块注释 */\nfn main() {{}}\n")
    assert any("块注释" in p for p in problems)


def test_inner_doc_after_code_flagged(tmp_path):
    problems = run_gate(tmp_path, f"{HEADER}\n\nfn main() {{}}\n//! 迟到的头\n")
    assert any("//! 出现在文件头之后" in p for p in problems)


# ---- 豁免 ----


def test_safety_block_and_continuation_allowed(tmp_path):
    content = (
        f"{HEADER}\n\n"
        "// SAFETY: 指针有效且对齐，契约见调用方。\n"
        "// 续行说明不变量的第二段。\n"
        "unsafe fn f() {}\n\n"
        "fn main() {}\n"
    )
    assert run_gate(tmp_path, content) == []


def test_trailing_secrets_allow_allowed(tmp_path):
    content = (
        f"{HEADER}\n\n"
        'const TOKEN: &str = "abc"; // secrets:allow 测试夹具\n'
        "fn main() { let _ = TOKEN; }\n"
    )
    assert run_gate(tmp_path, content) == []


def test_inline_lines_allow_allowed(tmp_path):
    content = f"{HEADER}\n\n// lines:allow 自测阴性对照\nfn main() {{}}\n"
    assert run_gate(tmp_path, content) == []


def test_safety_released_after_code_gap(tmp_path):
    content = (
        f"{HEADER}\n\n"
        "// SAFETY: 第一处\n"
        "unsafe fn f() {}\n\n"
        "fn g() {}\n\n"
        "// SAFETY 之后隔了代码，这句是普通行内注释\n"
        "fn main() {}\n"
    )
    problems = run_gate(tmp_path, content)
    assert any("行内注释" in p for p in problems)


# ---- 非 Rust 文件注释 ----


def test_nonrust_mixed_language_flagged(tmp_path):
    problems = run_gate(tmp_path, "# 中文注释里夹 generic 词。\n", rel="conf.toml")
    assert any("代码引用或白名单之外的英文词" in p for p in problems)


def test_nonrust_proper_noun_allowed(tmp_path):
    content = "# 中文注释说明 wgpu 渲染，cargo 打包。\n"
    assert run_gate(tmp_path, content, rel="conf.toml") == []


def test_nonrust_code_reference_flagged(tmp_path):
    problems = run_gate(tmp_path, "# 中文注释：配置见 deny.toml。\n", rel="conf.toml")
    assert any("代码引用" in p for p in problems)


def test_nonrust_backtick_flagged(tmp_path):
    problems = run_gate(tmp_path, "# 中文注释用 `just bench` 运行。\n", rel="justfile")
    assert any("代码段" in p for p in problems)


def test_nonrust_pure_english_flagged(tmp_path):
    problems = run_gate(tmp_path, "# Generated by cargo mutants\n", rel=".gitignore")
    assert any("注释须写成中文" in p for p in problems)


def test_nonrust_directive_lines_exempt(tmp_path):
    content = "#!/usr/bin/env python3\nx = 1  # fmt: skip\ny = 2  # noqa: E402 —— 中文补充说明\n"
    assert run_gate(tmp_path, content, rel="sample.py") == []


def test_nonrust_string_hash_not_comment(tmp_path):
    content = '# 中文头\n\nconst URL = "https://example.com/a#b"\n'
    assert run_gate(tmp_path, content, rel="conf.toml") == []


def test_gitignore_midline_hash_not_comment(tmp_path):
    content = "# 中文头\n\n**/*.rs.bk\n*.pdb\n"
    assert run_gate(tmp_path, content, rel=".gitignore") == []


def test_nonrust_multiline_toml_string_skipped(tmp_path):
    content = '# 中文头\n\nbody = """\n# 这不是注释，是多行字符串\n"""\n'
    assert run_gate(tmp_path, content, rel="cliff.toml") == []


def test_js_css_html_comments_flagged(tmp_path):
    js = run_gate(tmp_path, "// 中文注释带 generic 词。\nconst a = 1;\n", rel="web/main.js")
    css = run_gate(
        tmp_path, "/* 中文注释带 generic 词。 */\nbody { color: red; }\n", rel="web/s.css"
    )
    html = run_gate(tmp_path, "<div></div>\n<!-- 中文分节 Hero -->\n", rel="web/i.html")
    assert any("英文词" in p for p in js)
    assert any("英文词" in p for p in css)
    assert any("英文词" in p for p in html)


def test_nonrust_rust_files_unaffected(tmp_path):
    content = f"{HEADER}\n\nfn main() {{ let _ = 1; }}\n"
    assert run_gate(tmp_path, content) == []
