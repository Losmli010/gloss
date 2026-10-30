"""约束门禁·语言规范域的单测：字符事实单源逐条覆盖——判罚树里的数据源字
符面字面量即违规，注释与文档字符串、派生模块与测试文件、放行标记三路豁免
不误报，数据源缺失与损坏失败关闭，判罚模式随数据源走。
"""

from __future__ import annotations

import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).parent.parent))

from constraints import fixtures, langspec  # noqa: E402

# 带顿号与浪纹线的判罚数据面：覆盖基线夹具面外的成员，锁定模式随数据源走
DATA_TOML = (
    '[zh]\npunct = "，、～"\n\n[en]\npunct = "."\n\n[shared]\nmath = "="\n'
    'attached_after_digit = "%"\nattached_before_digit = "￥"\n'
    'attached_between_digits = "."\ndash_ellipsis = "…"\ncurly_quotes = "“”"\n'
    'sentence_finals = "。!"\nemoji_bare = "✅"\nemoji_variants = "\\uFE0F"\n'
)


def seed_data(root: Path, toml: str = DATA_TOML) -> None:
    base = root / "crates/gloss-core/langspec"
    base.mkdir(parents=True, exist_ok=True)
    (base / "zh_en.toml").write_text(toml, encoding="utf-8")


def write_gate(root: Path, body: str) -> None:
    gate = root / "crates/gloss-core/src/pipeline/lang_gate.rs"
    gate.parent.mkdir(parents=True, exist_ok=True)
    gate.write_text(body, encoding="utf-8")


def write_scripts(root: Path, **files: str) -> None:
    scripts = root / "scripts/hooks/language_spec"
    scripts.mkdir(parents=True)
    for name, body in files.items():
        (scripts / name).write_text(body, encoding="utf-8")


def test_judgment_files_without_face_literals_pass(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        write_gate(
            root,
            "//! 正文判定：字符事实自数据源派生。\n"
            "pub fn judge(c: char, punct: &[char]) -> bool {\n    punct.contains(&c)\n}\n",
        )
        write_scripts(
            root,
            **{
                "chinese.py": 'PUNCT = "，。"\n',
                "chinese_test.py": 'assert "，" in PUNCT\n',
                "english.py": "def check(text):\n    return text\n",
            },
        )

    fixtures.assert_pass(langspec, tmp_path, mutate, needle="language spec literals single-sourced")


def test_face_literal_in_production_judgment_flags(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        write_gate(root, "pub fn judge(c: char) -> bool {\n    c == '，'\n}\n")

    fixtures.assert_fail(
        langspec, tmp_path, mutate, "language character literal outside the data source"
    )


def test_face_literal_in_dev_judgment_flags(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        write_scripts(root, **{"chinese.py": 'PUNCT = "，。"\n', "english.py": 'BAD = "。"\n'})

    oks, fails = fixtures.run_case(langspec, tmp_path, mutate)
    assert fails
    joined = "\n".join(fails)
    assert "scripts/hooks/language_spec/english.py" in joined
    assert "chinese.py" not in joined


def test_comments_docstrings_and_test_modules_stay_out(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        write_gate(
            root,
            "//! 正文判定：注释里的，句号不判。\n"
            "#[cfg(test)]\nmod tests {\n    #[test]\n"
            '    fn sample_text_keeps_its_faces() {\n        assert_eq!("，。", "，。");\n    }\n}\n',
        )
        write_scripts(
            root,
            **{
                "coderef.py": '"""文件头：文档字符串里的，句号不判。"""\n\n\n'
                "def strip(text):\n    return text  # 行注释里的、顿号不判\n"
            },
        )

    fixtures.assert_pass(langspec, tmp_path, mutate)


def test_allow_marker_with_reason_skips_the_line(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        seed_data(root)
        write_scripts(root, **{"coderef.py": 'LIST = r"\\d+[.、]"\nOTHER = "。"\n'})
        scripts = root / "scripts/hooks/language_spec/coderef.py"
        lines = scripts.read_text(encoding="utf-8").split("\n")
        lines[0] += "  # langspec:allow 列表记号正则仍住代码"
        scripts.write_text("\n".join(lines), encoding="utf-8")

    _oks, fails = fixtures.run_case(langspec, tmp_path, mutate)
    assert len(fails) == 1
    assert "language character literal" in fails[0]


def test_allow_marker_without_reason_flags(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        seed_data(root)
        write_scripts(root, **{"coderef.py": 'LIST = "、"\n'})
        scripts = root / "scripts/hooks/language_spec/coderef.py"
        lines = scripts.read_text(encoding="utf-8").split("\n")
        lines[0] += "  # langspec:allow"
        scripts.write_text("\n".join(lines), encoding="utf-8")

    fixtures.assert_fail(langspec, tmp_path, mutate, "langspec:allow marker without a reason")


def test_missing_data_source_fails_closed(tmp_path: Path) -> None:
    with pytest.raises(SystemExit) as exc:
        langspec.run(str(tmp_path))
    assert exc.value.code == 3


def test_broken_data_source_fails_closed(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        seed_data(root, toml="not [valid toml")

    with pytest.raises(SystemExit) as exc:
        fixtures.run_case(langspec, tmp_path, mutate)
    assert exc.value.code == 3


def test_face_pattern_follows_the_data_source(tmp_path: Path) -> None:
    seed_data(tmp_path)
    faces = langspec.face_chars(str(tmp_path))
    assert "～" in faces and "、" in faces and "＝" not in faces
