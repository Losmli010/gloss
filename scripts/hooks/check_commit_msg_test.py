"""check_commit_msg.py 的单测：逐条覆盖 Conventional Commits 判定与边界。

被测函数是 check_commit_msg.run(text)（返回错误串或 None），不落进程。
用法：python3 -m pytest scripts/hooks/check_commit_msg_test.py
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
    assert_ok("chore(deps): 升级 egui 到 0.31")


def test_revert_type():
    assert_ok("revert: 回滚上一次提交")


def test_breaking_mark():
    assert_ok("feat!: 破坏性 API 变更")


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
    assert_ok("feat: " + "a" * 75)


def test_header_over_81_bytes():
    assert_reject("feat: " + "a" * 76)


def test_body_without_blank_line():
    assert_reject("feat: 标题\n直接正文无空行")


def test_single_char_subject():
    assert_ok("feat: a")


def test_two_char_subject():
    assert_ok("fix: ab")


def test_cjk_subject():
    assert_ok("feat: 标题")


def test_empty_subject():
    assert_reject("feat: ")


def test_single_line_hash_not_stripped():
    # 拉取请求（PR）标题场景：单行井号开头不按注释剥离，按格式错误拒绝
    assert_reject("#97: 新增钩子")


def test_multiline_comment_tail_stripped():
    assert_ok("feat: 标题\n\n正文\n# 尾部注释行")
