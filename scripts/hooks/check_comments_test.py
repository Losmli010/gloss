"""注释纪律门禁的单测：逐条覆盖判罚规则与豁免路径。

被测入口是门禁的运行函数，返回违规列表，不落进程；样本文件写入测试框架
（pytest）的临时目录，互不串扰；用法见持续集成的钩子工作流（逐文件执行单测）。
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
    assert any("missing file header" in p for p in problems)


def test_english_only_header_flagged(tmp_path):
    problems = run_gate(tmp_path, "//! English only header line.\n\nfn main() {}\n")
    assert any("without Chinese characters" in p for p in problems)


def test_bare_english_words_flagged(tmp_path):
    problems = run_gate(tmp_path, "//! 中文头说明 functional core 在此。\n\nfn main() {}\n")
    assert any("foreign letters" in p for p in problems)


def test_single_english_word_flagged(tmp_path):
    problems = run_gate(tmp_path, "//! 中文头借用 helper 渲染。\n\nfn main() {}\n")
    assert any("foreign letters" in p for p in problems)


def test_bare_proper_noun_flagged(tmp_path):
    problems = run_gate(tmp_path, "//! 中文头说明 wgpu 渲染。\n\nfn main() {}\n")
    assert any("foreign letters" in p for p in problems)


def test_gloss_annotation_allowed(tmp_path):
    content = "//! 中文头借用图形库（wgpu）渲染。\n\nfn main() {}\n"
    assert run_gate(tmp_path, content) == []


def test_gloss_multiple_nouns_allowed(tmp_path):
    content = "//! 渲染栈（wgpu/egui）与窗口栈（winit）组装。\n\nfn main() {}\n"
    assert run_gate(tmp_path, content) == []


def test_gloss_with_spaces_allowed(tmp_path):
    content = "//! 发布产物走发布托管（GitHub Releases）降级。\n\nfn main() {}\n"
    assert run_gate(tmp_path, content) == []


def test_gloss_mixed_with_chinese_flagged(tmp_path):
    problems = run_gate(tmp_path, "//! 中文头（用 wgpu 渲染）说明。\n\nfn main() {}\n")
    assert any("annotation bracket mixes Chinese" in p for p in problems)


def test_gloss_unknown_word_flagged(tmp_path):
    problems = run_gate(tmp_path, "//! 中文头借用图形库（wgpu-xxx）渲染。\n\nfn main() {}\n")
    assert any("outside the allowlist" in p for p in problems)


def test_unbalanced_bracket_flagged(tmp_path):
    problems = run_gate(tmp_path, "//! 中文头（缺右括号说明。\n\nfn main() {}\n")
    assert any("brackets are unpaired" in p for p in problems)


def test_nested_bracket_flagged(tmp_path):
    problems = run_gate(tmp_path, "//! 中文头（类型（范围）：主题）说明。\n\nfn main() {}\n")
    assert any("annotation brackets must not nest" in p for p in problems)


def test_halfwidth_punctuation_flagged(tmp_path):
    problems = run_gate(tmp_path, "//! 中文头, 含半角逗号。\n\nfn main() {}\n")
    assert any("foreign letters" in p for p in problems)


def test_fullwidth_punctuation_allowed(tmp_path):
    content = "//! 中文头，含全角逗号；含全角句号。数字 64 与箭头 → 放行。\n\nfn main() {}\n"
    assert run_gate(tmp_path, content) == []


def test_backtick_code_segment_flagged(tmp_path):
    problems = run_gate(tmp_path, "//! 中文头：术语 `functional core` 见记录。\n\nfn main() {}\n")
    assert any("backtick code segment" in p for p in problems)


def test_doc_reference_flagged(tmp_path):
    problems = run_gate(tmp_path, "//! 中文头：规则见项目说明文档。\n\nfn main() {}\n")
    assert problems == []
    problems = run_gate(tmp_path, "//! 中文头：规则见 AGENTS.md。\n\nfn main() {}\n")
    assert any("foreign letters" in p for p in problems)


def test_section_reference_flagged(tmp_path):
    problems = run_gate(tmp_path, "//! 中文头：规格见分发设计 §4.1。\n\nfn main() {}\n")
    assert any("section number" in p for p in problems)


def test_url_inside_string_not_flagged(tmp_path):
    content = (
        f'{HEADER}\n\nconst URL: &str = "https://example.com/a//b";\nfn main() {{ let _ = URL; }}\n'
    )
    assert run_gate(tmp_path, content) == []


# ---- 违规注释形态 ----


def test_doc_comment_flagged(tmp_path):
    problems = run_gate(tmp_path, f"{HEADER}\n\n/// 说明文字\nfn main() {{}}\n")
    assert any("/// doc comment" in p for p in problems)


def test_inline_comment_flagged(tmp_path):
    problems = run_gate(tmp_path, f"{HEADER}\n\n// 说明\nfn main() {{}}\n")
    assert any("inline comment" in p for p in problems)


def test_trailing_comment_flagged(tmp_path):
    problems = run_gate(tmp_path, f"{HEADER}\n\nfn main() {{ let _ = 1; // 尾注\n}}\n")
    assert any("trailing comment" in p for p in problems)


def test_block_comment_flagged(tmp_path):
    problems = run_gate(tmp_path, f"{HEADER}\n\n/* 块注释 */\nfn main() {{}}\n")
    assert any("block comment" in p for p in problems)


def test_inner_doc_after_code_flagged(tmp_path):
    problems = run_gate(tmp_path, f"{HEADER}\n\nfn main() {{}}\n//! 迟到的头\n")
    assert any("//! after the file header" in p for p in problems)


# ---- 不变量注释与豁免 ----


def test_safety_block_english_allowed(tmp_path):
    content = (
        f"{HEADER}\n\n"
        "// SAFETY: pointer is valid and aligned; the caller upholds the contract.\n"
        "// Continuation of the invariant, still pure English with symbols like Self.\n"
        "unsafe fn f() {}\n\n"
        "fn main() {}\n"
    )
    assert run_gate(tmp_path, content) == []


def test_safety_chinese_flagged(tmp_path):
    content = (
        f"{HEADER}\n\n"
        "// SAFETY: 指针有效且对齐，契约见调用方。\n"
        "unsafe fn f() {}\n\n"
        "fn main() {}\n"
    )
    problems = run_gate(tmp_path, content)
    assert any("SAFETY comment must be pure English" in p for p in problems)


def test_safety_continuation_chinese_flagged(tmp_path):
    content = (
        f"{HEADER}\n\n"
        "// SAFETY: pointer is valid and aligned.\n"
        "// 续行是中文，同样属于不变量注释。\n"
        "unsafe fn f() {}\n\n"
        "fn main() {}\n"
    )
    problems = run_gate(tmp_path, content)
    assert any("SAFETY comment must be pure English" in p for p in problems)


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
        "// SAFETY: first invariant\n"
        "unsafe fn f() {}\n\n"
        "fn g() {}\n\n"
        "// 隔了代码之后不再是续行，这句是普通行内注释\n"
        "fn main() {}\n"
    )
    problems = run_gate(tmp_path, content)
    assert any("inline comment" in p for p in problems)


# ---- 源码之外的文件注释 ----


def test_nonrust_mixed_language_flagged(tmp_path):
    problems = run_gate(tmp_path, "# 中文注释里夹 generic 词。\n", rel="conf.toml")
    assert any("foreign letters" in p for p in problems)


def test_nonrust_gloss_annotation_allowed(tmp_path):
    content = "# 中文注释说明图形库（wgpu）渲染，打包工具（cargo）负责打包。\n"
    assert run_gate(tmp_path, content, rel="conf.toml") == []


def test_nonrust_pure_chinese_gloss_allowed(tmp_path):
    content = "# 中文注释（本地与持续集成共用同一判定）说明。\n"
    assert run_gate(tmp_path, content, rel="conf.toml") == []


def test_nonrust_gloss_mixed_with_chinese_flagged(tmp_path):
    problems = run_gate(tmp_path, "# 中文注释（需 wgpu 渲染）说明。\n", rel="conf.toml")
    assert any("annotation bracket mixes Chinese" in p for p in problems)


def test_nonrust_code_reference_flagged(tmp_path):
    problems = run_gate(tmp_path, "# 中文注释：配置见依赖合规清单。\n", rel="conf.toml")
    assert problems == []
    problems = run_gate(tmp_path, "# 中文注释：配置见 deny.toml。\n", rel="conf.toml")
    assert any("foreign letters" in p for p in problems)


def test_nonrust_halfwidth_punctuation_flagged(tmp_path):
    problems = run_gate(tmp_path, "# 中文注释, 含半角逗号。\n", rel="conf.toml")
    assert any("foreign letters" in p for p in problems)


def test_nonrust_backtick_flagged(tmp_path):
    problems = run_gate(tmp_path, "# 中文注释用构建配方运行。\n", rel="justfile")
    assert problems == []
    problems = run_gate(tmp_path, "# 中文注释用 `just bench` 运行。\n", rel="justfile")
    assert any("code segment" in p for p in problems)


def test_nonrust_pure_english_flagged(tmp_path):
    problems = run_gate(tmp_path, "# Generated by cargo mutants\n", rel=".gitignore")
    assert any("comment must be written in Chinese" in p for p in problems)


def test_nonrust_directive_lines_exempt(tmp_path):
    content = "#!/usr/bin/env python3\nx = 1  # fmt: skip\ny = 2  # noqa: E402 —— 中文补充说明\n"
    assert run_gate(tmp_path, content, rel="sample.py") == []


def test_python_module_docstring_gated(tmp_path):
    problems = run_gate(tmp_path, '"""中文说明带 generic 词。"""\n', rel="sample.py")
    assert any("foreign letters" in p for p in problems)


def test_python_module_docstring_backtick_flagged(tmp_path):
    problems = run_gate(tmp_path, '"""中文说明：形态是 `a b` 三段。"""\n', rel="sample.py")
    assert any("code segment" in p for p in problems)


def test_python_module_docstring_pure_english_flagged(tmp_path):
    problems = run_gate(tmp_path, '"""Unit tests for the gate module."""\n', rel="sample.py")
    assert any("file header must be written in Chinese" in p for p in problems)


def test_python_interpreter_polyglot_string_exempt(tmp_path):
    content = "'''exec python3 -- \"$0\" \"$@\" # '''  # fmt: skip\n# 中文头\nx = 1\n"
    assert run_gate(tmp_path, content, rel="sample.py") == []


def test_python_function_docstring_not_gated(tmp_path):
    content = '# 中文头\n\ndef f():\n    """函数说明带 generic 词，不属于文件头。"""\n'
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
    assert any("foreign letters" in p for p in js)
    assert any("foreign letters" in p for p in css)
    assert any("foreign letters" in p for p in html)


def test_nonrust_rust_files_unaffected(tmp_path):
    content = f"{HEADER}\n\nfn main() {{ let _ = 1; }}\n"
    assert run_gate(tmp_path, content) == []
