"""提交信息门禁的单测：逐条覆盖提交规范判定与边界。

被测入口是门禁的校验函数，返回错误串或空，不落进程；用法见持续集成的钩子工作流。
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import check_commit_msg  # noqa: E402


def assert_ok(msg):
    assert check_commit_msg.run(msg) is None


def assert_reject(msg):
    assert check_commit_msg.run(msg) is not None


# ---- 合法用例 ----


def test_basic_feat():
    assert_ok("feat: 添加翻译浮层")


def test_basic_fix():
    assert_ok("fix: 修复选区读取崩溃")


def test_with_scope():
    assert_ok("fix(core): 修复选区读取崩溃")


def test_docs_type():
    assert_ok("docs: 补充使用说明")


def test_chore_with_scope():
    assert_ok("chore(deps): 升级 `egui` 到 0.31")


def test_revert_type():
    assert_ok("revert: 回滚上一次提交")


def test_breaking_mark():
    assert_ok("feat!: 破坏性接口（API）变更")


def test_scope_with_digits_and_dot():
    assert_ok("fix(ci.yml): 修复流水线配置")


def test_body_after_blank_line():
    assert_ok("feat: 标题\n\n这里是正文内容")


def test_multiline_body():
    assert_ok("feat: 标题\n\n正文第一行\n正文第二行")


# ---- 非法用例 ----


def test_no_type_prefix():
    assert_reject("添加了某个功能")


def test_unknown_type_update():
    assert_reject("update: 更新依赖")


def test_uppercase_type():
    assert_reject("FEAT: 大写前缀")


def test_missing_colon():
    assert_reject("feat 缺少冒号")


def test_missing_space_after_colon():
    assert_reject("feat:没有空格")


def test_empty_message():
    assert_reject("")


def test_whitespace_only():
    assert_reject("   ")


def test_scope_with_space():
    assert_reject("fix(bad scope): 非法 scope")


def test_scope_with_comma():
    assert_reject("fix(bad,scope): scope 不允许逗号")


def test_empty_scope():
    assert_reject("fix(): 空 scope")


# ---- 边界用例 ----


def test_header_exactly_81_bytes():
    assert_ok("feat: " + "字" * 25)


def test_header_over_81_bytes():
    assert_reject("feat: " + "字" * 26)


def test_body_without_blank_line():
    assert_reject("feat: 标题\n直接正文无空行")


def test_single_char_subject():
    assert_ok("feat: 改")


def test_two_char_subject():
    assert_ok("fix: 修好")


def test_cjk_subject():
    assert_ok("feat: 标题")


def test_empty_subject():
    assert_reject("feat: ")


def test_single_line_hash_not_stripped():
    # 拉取请求（PR）标题场景：单行井号开头不按注释剥离，按格式错误拒绝
    assert_reject("#97: 新增钩子")


def test_multiline_comment_tail_stripped():
    assert_ok("feat: 标题\n\n正文\n# 尾部注释行")


# ---- 书写规则用例（与注释门禁同源）----


def test_subject_bare_word_rejected():
    assert_reject("feat: 添加 login 重试")


def test_subject_gloss_allowed():
    assert_ok("feat: 登录页（URL）校验重试")


def test_subject_english_rejected():
    assert_reject("feat: add translation popup")


def test_subject_squash_suffix_allowed():
    assert_ok("feat: 中文标题 (#99999)")


def test_body_bare_identifier_rejected():
    assert_reject("feat: 标题\n\n改动见 docs/tests/bdd.md")


def test_body_backticked_identifier_allowed():
    assert_ok("feat: 标题\n\n改动见 `docs/tests/bdd.md`")


def test_body_trailer_allowed():
    assert_ok("feat: 标题\n\n正文\n\nCo-authored-by: Someone <a@example.com>")


def test_body_bullet_lines_allowed():
    assert_ok("feat: 标题\n\n- 第一点\n- 第二点")


def test_body_squash_list_entry_allowed():
    assert_ok("feat: 标题\n\n* fix(core): 修复选区读取崩溃\n* docs: 补充说明")


def test_body_squash_entry_without_bullet_allowed():
    assert_ok("feat: 标题\n\nfix(core): 修复选区读取崩溃")


def test_body_squash_entry_with_bang_allowed():
    assert_ok("feat: 标题\n\n* feat!: 破坏性变更说明")


def test_body_squash_merge_shape_allowed():
    assert_ok(
        "refactor(comments): 专有名词只进括号注记，主体全中文 (#229)\n"
        "\n"
        "* refactor(comments): 专有名词只进括号注记，主体全中文\n"
        "\n"
        "注释纪律收紧为三层，正文段照旧受检。\n"
        "\n"
        "* fix(comments): 词法边界收口\n"
        "* docs(readme): 项目描述同规注释纪律"
    )


def test_body_squash_entry_rest_still_checked():
    assert_reject("feat: 标题\n\n* fix: add english text")


def test_body_squash_entry_unknown_type_rejected():
    assert_reject("feat: 标题\n\n* update: 更新依赖")


def test_body_squash_entry_scope_prose_still_checked():
    assert_reject("feat: 标题\n\n* fix(login page): 登录页改动")
