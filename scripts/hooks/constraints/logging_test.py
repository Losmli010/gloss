"""约束门禁·日志域的单测：统一出口与一律英文逐条覆盖——核心库持有日志三件
套是唯一合法形态，日志宏实参出现非英文字节即违规，中文注释与英文实参共存
的形态不误报。
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from constraints import fixtures, logging  # noqa: E402


def test_log_arguments_english_with_chinese_comments(tmp_path):
    fixtures.assert_pass(logging, tmp_path, needle="0 violations")


def test_core_tracing_is_the_only_log_exit(tmp_path):
    fixtures.assert_pass(logging, tmp_path)


def test_app_depends_on_tracing(tmp_path):
    def mutate(root):
        fixtures.add_dep(
            root / "crates/gloss-app/Cargo.toml",
            'tracing = { version = "0.1", default-features = false }',
        )

    fixtures.assert_fail(logging, tmp_path, mutate, "use gloss_core::log only")


def test_platform_depends_on_tracing_subscriber(tmp_path):
    def mutate(root):
        fixtures.add_dep(
            root / "crates/gloss-platform/Cargo.toml",
            'tracing-subscriber = { version = "0.3", default-features = false }',
        )

    fixtures.assert_fail(logging, tmp_path, mutate, "use gloss_core::log only")


def test_log_message_in_chinese(tmp_path):
    def mutate(root):
        (root / "crates/gloss-core/src/log.rs").write_text('info!("找不到配置文件");\n')

    fixtures.assert_fail(logging, tmp_path, mutate, "log macro argument with non-ASCII")


def test_log_message_multiline_chinese(tmp_path):
    def mutate(root):
        (root / "crates/gloss-core/src/log.rs").write_text(
            'info!(\n    gen = 1,\n    "读取失败"\n);\n'
        )

    fixtures.assert_fail(logging, tmp_path, mutate, "log macro argument with non-ASCII")
