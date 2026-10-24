"""拉取请求内容门禁的单测：书写规则套用、模板小节核对与标题合并模拟。

被测入口是门禁的运行函数，返回违规列表，不落进程；模板写入测试框架
（pytest）的临时目录，互不串扰；用法见持续集成的钩子工作流。
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import check_pr_prose  # noqa: E402

TEMPLATE = "## 测试（必填）\n\n## 安全与隐私（必填）\n"


def make_root(tmp_path, template=TEMPLATE):
    (tmp_path / ".github").mkdir(parents=True, exist_ok=True)
    (tmp_path / ".github" / "pull_request_template.md").write_text(template, encoding="utf-8")
    return str(tmp_path)


# ---- 模板小节 ----


def test_missing_section_flagged(tmp_path):
    problems = check_pr_prose.run(make_root(tmp_path), "## 测试（必填）\n\n正文\n")
    assert any("misses required template section" in p for p in problems)


def test_all_sections_present_ok(tmp_path):
    body = "## 测试（必填）\n\n跑了测试\n\n## 安全与隐私（必填）\n\n无\n"
    assert check_pr_prose.run(make_root(tmp_path), body) == []


def test_missing_template_skips_section_check(tmp_path):
    body = "## 随意小节\n\n中文正文。\n"
    assert check_pr_prose.run(str(tmp_path), body) == []


# ---- 正文书写规则 ----


def test_body_bare_word_flagged(tmp_path):
    problems = check_pr_prose.run(
        make_root(tmp_path), "## 测试（必填）\n\n改动见 docs/tests/bdd.md\n"
    )
    assert any("foreign letters" in p for p in problems)


def test_body_backtick_and_fence_ok(tmp_path):
    body = (
        "## 测试（必填）\n\n改动见 `docs/tests/bdd.md`\n\n```sh\njust test\n```\n"
        "## 安全与隐私（必填）\n\n无\n"
    )
    assert check_pr_prose.run(make_root(tmp_path), body) == []


def test_body_links_and_checkboxes_ok(tmp_path):
    body = (
        "## 测试（必填）\n\n- [x] 已跑 `just precommit`\n"
        "说明见 [指南](https://example.com/guide)\n"
        "## 安全与隐私（必填）\n\n无\n"
    )
    assert check_pr_prose.run(make_root(tmp_path), body) == []


def test_body_english_line_flagged(tmp_path):
    problems = check_pr_prose.run(make_root(tmp_path), "## 测试（必填）\n\nJust run the tests.\n")
    assert any("must be written in Chinese" in p for p in problems)


# ---- 标题合并模拟 ----


def test_title_chinese_ok(tmp_path):
    body = "## 测试（必填）\n\n无\n\n## 安全与隐私（必填）\n\n无\n"
    assert check_pr_prose.run(make_root(tmp_path), body, title="feat: 新增翻译浮层") == []


def test_title_bare_word_flagged(tmp_path):
    body = "## 测试（必填）\n\n无\n\n## 安全与隐私（必填）\n\n无\n"
    problems = check_pr_prose.run(make_root(tmp_path), body, title="feat: 添加 login 重试")
    assert any("squash-merge simulation" in p for p in problems)


def test_title_english_format_flagged(tmp_path):
    body = "## 测试（必填）\n\n无\n"
    problems = check_pr_prose.run(make_root(tmp_path), body, title="Bump egui from 0.31 to 0.32")
    assert any("squash-merge simulation" in p for p in problems)
