"""注释纪律门禁的单测：逐条覆盖判罚规则、豁免路径与跟踪清单枚举。

被测入口是门禁的运行函数，返回违规渲染行列表，不落进程；样本文件写入测试
框架（pytest）的临时目录，互不串扰；枚举测试在临时目录建一次性版本库，用
法见持续集成的钩子工作流（逐文件执行单测）。另含反馈契约三件套：报告格式
断言、汇总计数与确定性，经子进程跑脚本端到端锁输出与退出码。
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import check_comments  # noqa: E402

HEADER = "//! 中文文件头：负责演示。"


def write_sample(tmp_path: Path, content: str, rel: str = "crates/demo/src/sample.rs") -> str:
    p = tmp_path / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(content, encoding="utf-8")
    return rel


def run_gate(tmp_path: Path, content: str, rel: str = "crates/demo/src/sample.rs") -> list[str]:
    write_sample(tmp_path, content, rel)
    return check_comments.run(str(tmp_path), [rel])


# ---- 文件头与干净样本 ----


def test_clean_file_passes(tmp_path: Path) -> None:
    assert run_gate(tmp_path, f"{HEADER}\n\nfn main() {{ let _ = 1; }}\n") == []


def test_missing_header_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, "fn main() {}\n")
    assert any("missing file header" in p for p in problems)


def test_english_only_header_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, "//! English only header line.\n\nfn main() {}\n")
    assert any("without Chinese characters" in p for p in problems)


def test_bare_english_words_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, "//! 中文头说明 functional core 在此。\n\nfn main() {}\n")
    assert any("foreign letters" in p for p in problems)


def test_single_english_word_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, "//! 中文头借用 helper 渲染。\n\nfn main() {}\n")
    assert any("foreign letters" in p for p in problems)


def test_bare_proper_noun_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, "//! 中文头说明 wgpu 渲染。\n\nfn main() {}\n")
    assert any("foreign letters" in p for p in problems)


def test_gloss_annotation_allowed(tmp_path: Path) -> None:
    content = "//! 中文头借用图形库（wgpu）渲染。\n\nfn main() {}\n"
    assert run_gate(tmp_path, content) == []


def test_gloss_multiple_nouns_allowed(tmp_path: Path) -> None:
    content = "//! 渲染栈（wgpu/egui）与窗口栈（winit）组装。\n\nfn main() {}\n"
    assert run_gate(tmp_path, content) == []


def test_gloss_with_spaces_allowed(tmp_path: Path) -> None:
    content = "//! 图形栈走界面库（egui winit）与图形后端（wgpu）。\n\nfn main() {}\n"
    assert run_gate(tmp_path, content) == []


def test_gloss_mixed_with_chinese_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, "//! 中文头（用 wgpu 渲染）说明。\n\nfn main() {}\n")
    assert any("annotation bracket mixes Chinese" in p for p in problems)


def test_gloss_unknown_word_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, "//! 中文头借用图形库（wgpu-xxx）渲染。\n\nfn main() {}\n")
    assert any("outside the allowlist" in p for p in problems)


def test_unbalanced_bracket_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, "//! 中文头（缺右括号说明。\n\nfn main() {}\n")
    assert any("brackets are unpaired" in p for p in problems)


def test_nested_bracket_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, "//! 中文头（类型（范围）：主题）说明。\n\nfn main() {}\n")
    assert any("annotation brackets must not nest" in p for p in problems)


def test_halfwidth_punctuation_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, "//! 中文头, 含半角逗号。\n\nfn main() {}\n")
    assert any("foreign letters" in p for p in problems)


def test_fullwidth_punctuation_allowed(tmp_path: Path) -> None:
    content = "//! 中文头，含全角逗号；含全角句号。数字 64 与箭头 → 放行。\n\nfn main() {}\n"
    assert run_gate(tmp_path, content) == []


def test_backtick_code_segment_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, "//! 中文头：术语 `functional core` 见记录。\n\nfn main() {}\n")
    assert any("backtick code segment" in p for p in problems)


def test_doc_reference_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, "//! 中文头：规则见项目说明文档。\n\nfn main() {}\n")
    assert problems == []
    problems = run_gate(tmp_path, "//! 中文头：规则见 AGENTS.md。\n\nfn main() {}\n")
    assert any("foreign letters" in p for p in problems)


def test_section_reference_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, "//! 中文头：规格见分发设计 §4.1。\n\nfn main() {}\n")
    assert any("section number" in p for p in problems)


def test_url_inside_string_not_flagged(tmp_path: Path) -> None:
    content = (
        f'{HEADER}\n\nconst URL: &str = "https://example.com/a//b";\nfn main() {{ let _ = URL; }}\n'
    )
    assert run_gate(tmp_path, content) == []


# ---- 违规注释形态 ----


def test_doc_comment_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, f"{HEADER}\n\n/// 说明文字\nfn main() {{}}\n")
    assert any("/// doc comment" in p for p in problems)


def test_inline_comment_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, f"{HEADER}\n\n// 说明\nfn main() {{}}\n")
    assert any("inline comment" in p for p in problems)


def test_trailing_comment_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, f"{HEADER}\n\nfn main() {{ let _ = 1; // 尾注\n}}\n")
    assert any("trailing comment" in p for p in problems)


def test_block_comment_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, f"{HEADER}\n\n/* 块注释 */\nfn main() {{}}\n")
    assert any("block comment" in p for p in problems)


def test_inner_doc_after_code_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, f"{HEADER}\n\nfn main() {{}}\n//! 迟到的头\n")
    assert any("//! after the file header" in p for p in problems)


# ---- 不变量注释与豁免 ----


def test_safety_block_english_allowed(tmp_path: Path) -> None:
    content = (
        f"{HEADER}\n\n"
        "// SAFETY: pointer is valid and aligned; the caller upholds the contract.\n"
        "// Continuation of the invariant, still pure English with symbols like Self.\n"
        "unsafe fn f() {}\n\n"
        "fn main() {}\n"
    )
    assert run_gate(tmp_path, content) == []


def test_safety_chinese_flagged(tmp_path: Path) -> None:
    content = (
        f"{HEADER}\n\n"
        "// SAFETY: 指针有效且对齐，契约见调用方。\n"
        "unsafe fn f() {}\n\n"
        "fn main() {}\n"
    )
    problems = run_gate(tmp_path, content)
    assert any("SAFETY comment must be pure English" in p for p in problems)


def test_safety_continuation_chinese_flagged(tmp_path: Path) -> None:
    content = (
        f"{HEADER}\n\n"
        "// SAFETY: pointer is valid and aligned.\n"
        "// 续行是中文，同样属于不变量注释。\n"
        "unsafe fn f() {}\n\n"
        "fn main() {}\n"
    )
    problems = run_gate(tmp_path, content)
    assert any("SAFETY comment must be pure English" in p for p in problems)


def test_trailing_secrets_allow_allowed(tmp_path: Path) -> None:
    content = (
        f"{HEADER}\n\n"
        'const TOKEN: &str = "abc"; // secrets:allow 测试夹具\n'
        "fn main() { let _ = TOKEN; }\n"
    )
    assert run_gate(tmp_path, content) == []


def test_inline_lines_allow_allowed(tmp_path: Path) -> None:
    content = f"{HEADER}\n\n// lines:allow 自测阴性对照\nfn main() {{}}\n"
    assert run_gate(tmp_path, content) == []


def test_bare_marker_in_trailing_comment_flagged(tmp_path: Path) -> None:
    content = (
        f'{HEADER}\n\nconst TOKEN: &str = "abc"; // secrets:allow\nfn main() {{ let _ = TOKEN; }}\n'
    )
    problems = run_gate(tmp_path, content)
    assert any("marker without a reason" in p for p in problems)
    assert not any("trailing comment" in p for p in problems)


def test_bare_marker_in_inline_comment_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, f"{HEADER}\n\n// deps:allow\nfn main() {{}}\n")
    assert any("marker without a reason" in p for p in problems)
    assert not any("inline comment" in p for p in problems)


def test_full_form_context_marker_with_reason_allowed(tmp_path: Path) -> None:
    content = (
        f"{HEADER}\n\n"
        'let gear = icon_button("⚙"); // constraints:allow-context 阴性对照\n'
        "fn main() {}\n"
    )
    assert run_gate(tmp_path, content) == []


def test_bare_marker_in_script_comment_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, "# lines:allow\n", rel="conf.toml")
    assert any("marker without a reason" in p for p in problems)


def test_marker_with_reason_in_script_comment_allowed(tmp_path: Path) -> None:
    assert run_gate(tmp_path, "# i18n:allow 图标字形对照\n", rel="conf.toml") == []


def test_safety_released_after_code_gap(tmp_path: Path) -> None:
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


def test_nonrust_mixed_language_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, "# 中文注释里夹 generic 词。\n", rel="conf.toml")
    assert any("foreign letters" in p for p in problems)


def test_nonrust_gloss_annotation_allowed(tmp_path: Path) -> None:
    content = "# 中文注释说明图形库（wgpu）渲染，打包工具（cargo）负责打包。\n"
    assert run_gate(tmp_path, content, rel="conf.toml") == []


def test_nonrust_pure_chinese_gloss_allowed(tmp_path: Path) -> None:
    content = "# 中文注释（本地与持续集成共用同一判定）说明。\n"
    assert run_gate(tmp_path, content, rel="conf.toml") == []


def test_nonrust_gloss_mixed_with_chinese_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, "# 中文注释（需 wgpu 渲染）说明。\n", rel="conf.toml")
    assert any("annotation bracket mixes Chinese" in p for p in problems)


def test_nonrust_code_reference_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, "# 中文注释：配置见依赖合规清单。\n", rel="conf.toml")
    assert problems == []
    problems = run_gate(tmp_path, "# 中文注释：配置见 deny.toml。\n", rel="conf.toml")
    assert any("foreign letters" in p for p in problems)


def test_nonrust_halfwidth_punctuation_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, "# 中文注释, 含半角逗号。\n", rel="conf.toml")
    assert any("foreign letters" in p for p in problems)


def test_nonrust_backtick_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, "# 中文注释用构建配方运行。\n", rel="justfile")
    assert problems == []
    problems = run_gate(tmp_path, "# 中文注释用 `just bench` 运行。\n", rel="justfile")
    assert any("code segment" in p for p in problems)


def test_nonrust_pure_english_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, "# Generated by cargo mutants\n", rel=".gitignore")
    assert any("comment must be written in Chinese" in p for p in problems)


def test_nonrust_directive_lines_exempt(tmp_path: Path) -> None:
    content = "#!/usr/bin/env python3\nx = 1  # fmt: skip\ny = 2  # noqa: E402 —— 中文补充说明\n"
    assert run_gate(tmp_path, content, rel="sample.py") == []


def test_python_module_docstring_gated(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, '"""中文说明带 generic 词。"""\n', rel="sample.py")
    assert any("foreign letters" in p for p in problems)


def test_python_module_docstring_backtick_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, '"""中文说明：形态是 `a b` 三段。"""\n', rel="sample.py")
    assert any("code segment" in p for p in problems)


def test_python_module_docstring_pure_english_flagged(tmp_path: Path) -> None:
    problems = run_gate(tmp_path, '"""Unit tests for the gate module."""\n', rel="sample.py")
    assert any("file header must be written in Chinese" in p for p in problems)


def test_python_interpreter_polyglot_string_exempt(tmp_path: Path) -> None:
    content = "'''exec python3 -- \"$0\" \"$@\" # '''  # fmt: skip\n# 中文头\nx = 1\n"
    assert run_gate(tmp_path, content, rel="sample.py") == []


def test_python_function_docstring_not_gated(tmp_path: Path) -> None:
    content = '# 中文头\n\ndef f():\n    """函数说明带 generic 词，不属于文件头。"""\n'
    assert run_gate(tmp_path, content, rel="sample.py") == []


def test_nonrust_string_hash_not_comment(tmp_path: Path) -> None:
    content = '# 中文头\n\nconst URL = "https://example.com/a#b"\n'
    assert run_gate(tmp_path, content, rel="conf.toml") == []


def test_gitignore_midline_hash_not_comment(tmp_path: Path) -> None:
    content = "# 中文头\n\n**/*.rs.bk\n*.pdb\n"
    assert run_gate(tmp_path, content, rel=".gitignore") == []


def test_nonrust_multiline_toml_string_skipped(tmp_path: Path) -> None:
    content = '# 中文头\n\nbody = """\n# 这不是注释，是多行字符串\n"""\n'
    assert run_gate(tmp_path, content, rel="cliff.toml") == []


def test_js_css_html_comments_flagged(tmp_path: Path) -> None:
    js = run_gate(tmp_path, "// 中文注释带 generic 词。\nconst a = 1;\n", rel="web/main.js")
    css = run_gate(
        tmp_path, "/* 中文注释带 generic 词。 */\nbody { color: red; }\n", rel="web/s.css"
    )
    html = run_gate(tmp_path, "<div></div>\n<!-- 中文分节 Hero -->\n", rel="web/i.html")
    assert any("foreign letters" in p for p in js)
    assert any("foreign letters" in p for p in css)
    assert any("foreign letters" in p for p in html)


def test_nonrust_rust_files_unaffected(tmp_path: Path) -> None:
    content = f"{HEADER}\n\nfn main() {{ let _ = 1; }}\n"
    assert run_gate(tmp_path, content) == []


# ---- 标记语言文档同规用例 ----


def run_md(tmp_path: Path, content: str, rel: str = "docs/tests/bdd.md") -> list[str]:
    return run_gate(tmp_path, content, rel=rel)


def test_bdd_table_first_cell_and_date_exempt(tmp_path: Path) -> None:
    content = (
        "| 测试名称 | 测试目标 | 测试场景 | 更新时间 |\n"
        "| --- | --- | --- | --- |\n"
        "| some_test_name | 密钥存取桩往返 | 给定输入则读回写入值 | 2026-09-19 |\n"
    )
    assert run_md(tmp_path, content) == []


def test_bdd_prose_bare_word_flagged(tmp_path: Path) -> None:
    problems = run_md(tmp_path, "| some_test_name | 中文目标 generic 场景 | 2026-09-19 |\n")
    assert any("foreign letters" in p for p in problems)


def test_bdd_identifier_heading_exempt(tmp_path: Path) -> None:
    content = "### some_test_name（2026-09-30，边界）\n- 测试目标：中文描述。\n"
    assert run_md(tmp_path, content) == []


def test_bdd_chinese_heading_checked(tmp_path: Path) -> None:
    problems = run_md(tmp_path, "### 中文小节带 generic 词\n")
    assert any("foreign letters" in p for p in problems)


def test_bdd_pure_command_cell_exempt(tmp_path: Path) -> None:
    content = "| 类别 | 数量 | 运行 |\n| --- | --- | --- |\n| 单元测试 | 629 | `just test` |\n"
    assert run_md(tmp_path, content) == []


def test_bdd_code_fence_exempt(tmp_path: Path) -> None:
    content = "### some_test_name\n- 测试步骤：\n\n```bash\njust test -- generic\n```\n"
    assert run_md(tmp_path, content) == []


def test_md_root_doc_gated(tmp_path: Path) -> None:
    problems = run_md(tmp_path, "# 应用名\n\nGloss is a tool.\n", rel="SECURITY.md")
    assert any("must be written in Chinese" in p for p in problems)


def test_md_comments_record_dir_exempt(tmp_path: Path) -> None:
    content = "# 任意记录\n\nEnglish prose with `code` and generic words.\n"
    assert run_md(tmp_path, content, rel="docs/comments/demo_comment.md") == []


def test_md_prompts_dir_exempt(tmp_path: Path) -> None:
    content = "You are a translation assistant. Reply in English.\n"
    assert run_md(tmp_path, content, rel="crates/gloss-core/prompts/en/word_card.md") == []


def test_md_nested_badge_link_exempt(tmp_path: Path) -> None:
    content = "[![CI](https://example.com/ci.svg)](https://example.com/ci)\n# 应用\n"
    assert run_md(tmp_path, content) == []


def test_md_task_list_marker_exempt(tmp_path: Path) -> None:
    content = "- [x] 已完成的条目\n- [ ] 待办条目\n"
    assert run_md(tmp_path, content) == []


def test_md_bold_markers_exempt(tmp_path: Path) -> None:
    content = "体验命门是 **克制** 与 **稳定**。\n"
    assert run_md(tmp_path, content) == []


def test_md_gloss_annotation_allowed(tmp_path: Path) -> None:
    content = "# 应用\n\n划词翻译工具，渲染走图形后端（wgpu）。\n"
    assert run_md(tmp_path, content) == []


def test_md_identifier_heading_exempt_at_root(tmp_path: Path) -> None:
    content = "## License\n\nMIT\n"
    assert run_md(tmp_path, content, rel="README.md") == []


def test_md_structure_penalties_enforced(tmp_path: Path) -> None:
    problems = run_md(tmp_path, "## 小节\n\n#### 跳级\n\n## 小节\n")
    assert any("heading level jumps" in p for p in problems)
    assert any("repeats an earlier heading" in p for p in problems)
    problems = run_md(tmp_path, "## 小节\n\n```sh\njust test\n")
    assert any("never closed" in p for p in problems)


def test_md_structure_clean_doc_passes(tmp_path: Path) -> None:
    content = "# 标题\n\n## 小节\n\n### 次节\n\n```sh\njust test\n```\n"
    assert run_md(tmp_path, content) == []


# ---- 不变量注释邻接与词法边界 ----


def test_safety_not_adjacent_flagged(tmp_path: Path) -> None:
    content = f"{HEADER}\n\n// SAFETY: documents nothing adjacent\nlet ok = 1;\n"
    problems = run_gate(tmp_path, content)
    assert any("not adjacent to an unsafe block" in p for p in problems)


def test_safety_separated_by_statement_flagged(tmp_path: Path) -> None:
    content = (
        f"{HEADER}\n\n// SAFETY: separated by a full statement\n"
        "let step = 1;\nunsafe { boom(); }\n"
    )
    problems = run_gate(tmp_path, content)
    assert any("not adjacent to an unsafe block" in p for p in problems)


def test_safety_adjacent_multiline_block_allowed(tmp_path: Path) -> None:
    content = (
        f"{HEADER}\n\n// SAFETY: the pointer outlives the call\n"
        "// and the call only reads it.\nlet v = unsafe { read(ptr) };\n"
    )
    assert run_gate(tmp_path, content) == []


def test_safety_adjacent_unsafe_fn_allowed(tmp_path: Path) -> None:
    content = f"{HEADER}\n\n// SAFETY: the caller upholds the invariant.\npub unsafe fn raw() -> i32 {{ 0 }}\n"
    assert run_gate(tmp_path, content) == []


def test_safety_unsafe_word_in_string_not_counted(tmp_path: Path) -> None:
    content = f'{HEADER}\n\n// SAFETY: mentions unsafe in a string only\nlet s = "unsafe";\n'
    problems = run_gate(tmp_path, content)
    assert any("not adjacent to an unsafe block" in p for p in problems)


def test_safety_unsafe_in_long_identifier_not_counted(tmp_path: Path) -> None:
    content = f"{HEADER}\n\n// SAFETY: long identifier is not the keyword\nlet unsafe_mode = 1;\n"
    problems = run_gate(tmp_path, content)
    assert any("not adjacent to an unsafe block" in p for p in problems)


def test_long_unicode_char_literal_keeps_lexer_honest(tmp_path: Path) -> None:
    content = (
        f"{HEADER}\n\nfn emoji() {{ let e = '\\u{{1F600}}'; let s = \"a//b\"; let _ = (e, s); }}\n"
    )
    assert run_gate(tmp_path, content) == []


def test_block_comment_preserves_trailing_classification(tmp_path: Path) -> None:
    content = f"{HEADER}\n\nlet a = 1; /* 块 */ // 尾注\n"
    problems = run_gate(tmp_path, content)
    assert any("trailing comment" in p for p in problems)
    assert not any("inline comment" in p for p in problems)


# ---- 字面数据与引号边界 ----


def test_js_multiline_template_literal_not_comment(tmp_path: Path) -> None:
    content = (
        "const tpl = `first line\n"
        "// not a comment inside the template\n"
        "third ${x}`;\n"
        "// 中文注释收尾。\n"
    )
    assert run_gate(tmp_path, content, rel="web/main.js") == []


def test_js_comment_after_template_still_checked(tmp_path: Path) -> None:
    content = "const tpl = `a\nb`;\n// english tail\n"
    problems = run_gate(tmp_path, content, rel="web/main.js")
    assert any("must be written in Chinese" in p for p in problems)


def test_html_attribute_comment_marker_ignored(tmp_path: Path) -> None:
    content = '<a title="<!-- not a comment">链接</a>\n<!-- 中文注释 -->\n'
    assert run_gate(tmp_path, content, rel="web/i.html") == []


def test_yaml_block_scalar_literal_content_exempt(tmp_path: Path) -> None:
    content = "desc: |\n  # literal heading in the scalar\n  正文文本\nkey: 值\n"
    assert run_gate(tmp_path, content, rel="wf.yml") == []


def test_yaml_run_block_comments_still_checked(tmp_path: Path) -> None:
    content = "run: |\n  # english shell comment\n"
    problems = run_gate(tmp_path, content, rel="wf.yml")
    assert any("must be written in Chinese" in p for p in problems)


def test_sh_heredoc_content_exempt(tmp_path: Path) -> None:
    content = "cat <<EOF\n# literal heredoc line\nEOF\n# 中文注释。\n"
    assert run_gate(tmp_path, content, rel="run.sh") == []


def test_sh_unterminated_heredoc_still_checked(tmp_path: Path) -> None:
    content = "cat <<EOF\n# english line without terminator\n"
    problems = run_gate(tmp_path, content, rel="run.sh")
    assert any("must be written in Chinese" in p for p in problems)


def test_default_root_is_repo_root() -> None:
    assert (Path(check_comments.REPO_ROOT) / "justfile").is_file()


def test_non_ascii_tracked_name_still_scanned(tmp_path: Path) -> None:
    (tmp_path / "源码.rs").write_text("fn main() {}\n", encoding="utf-8")
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    problems = check_comments.run(str(tmp_path), [])
    assert any("missing file header" in p for p in problems)


# ---- 反馈契约：报告格式、汇总计数、确定性与退出码（子进程端到端）----


GATE = str(Path(__file__).with_name("check_comments.py"))


def make_repo(tmp_path: Path, files: dict[str, str]) -> None:
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    for name, text in files.items():
        p = tmp_path / name
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text, encoding="utf-8")
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)


def run_gate_script(tmp_path: Path) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run([sys.executable, GATE, str(tmp_path)], capture_output=True)


def violation_fixture() -> dict[str, str]:
    return {
        "crates/demo/src/bad.rs": "fn main() {}\n",
        "conf.toml": "# Generated by cargo mutants\n",
        "docs/note.md": "# 说明\n\nThis is an English note.\n",
    }


def test_gate_reports_rule_ids_and_violation_exit_code(tmp_path: Path) -> None:
    make_repo(tmp_path, violation_fixture())
    proc = run_gate_script(tmp_path)
    assert proc.returncode == 1
    text = proc.stderr.decode("utf-8")
    heads = [line for line in text.splitlines() if line.startswith("✗ error[")]
    ids = {re.match(r"^✗ error\[([A-Z0-9-]+)\]:", line).group(1) for line in heads}
    assert ids == {
        "COMMENT-RUST-HEADER-MISSING",
        "COMMENT-SCRIPT-LANGUAGE",
        "PROSE-LANGUAGE",
    }
    for line in heads:
        assert re.match(r"^✗ error\[[A-Z0-9-]+\]: .+ \(.+(:\d+)?\)$", line)
    assert "    证据: " in text
    assert "    help: " in text


def test_gate_summary_count_matches_printed_details(tmp_path: Path) -> None:
    make_repo(tmp_path, violation_fixture())
    text = run_gate_script(tmp_path).stderr.decode("utf-8")
    details = sum(1 for line in text.splitlines() if line.startswith("✗ error["))
    match = re.search(r"^error: (\d+) violations? found \(see above\)$", text, re.M)
    assert match
    assert int(match.group(1)) == details == 3


def test_gate_output_deterministic_across_runs(tmp_path: Path) -> None:
    make_repo(tmp_path, violation_fixture())
    first = run_gate_script(tmp_path)
    second = run_gate_script(tmp_path)
    assert first.returncode == second.returncode == 1
    assert first.stderr == second.stderr
    assert first.stdout == second.stdout


def test_gate_clean_fixture_passes_with_exit_zero(tmp_path: Path) -> None:
    make_repo(
        tmp_path,
        {
            "crates/demo/src/ok.rs": "//! 中文文件头：演示用。\n\nfn main() { let _ = 1; }\n",
            "docs/note.md": "# 说明\n\n中文正文，借用图形库（wgpu）渲染。\n",
        },
    )
    proc = run_gate_script(tmp_path)
    assert proc.returncode == 0
    assert "  ✓ ok " in proc.stdout.decode("utf-8")


def test_gate_empty_tracked_surface_fails_closed_with_exit_three(tmp_path: Path) -> None:
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    proc = run_gate_script(tmp_path)
    assert proc.returncode == 3
    assert "✗ error[EMPTY-SURFACE]:" in proc.stderr.decode("utf-8")


def test_gate_non_directory_root_is_a_usage_error(tmp_path: Path) -> None:
    proc = subprocess.run([sys.executable, GATE, str(tmp_path / "nowhere")], capture_output=True)
    assert proc.returncode == 2
    assert b"usage:" in proc.stderr
