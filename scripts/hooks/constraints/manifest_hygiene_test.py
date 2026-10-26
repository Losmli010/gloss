"""约束门禁·清单卫生域的单测：版本单点、依赖只开需要的特性与依赖无死条目
逐条覆盖——字面量、缺省特性与无证据声明必须被拒，跨行条目、放行标记与各类
使用证据形态放行。
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from constraints import fixtures, manifest_hygiene  # noqa: E402


def test_baseline_workspace_all_green(tmp_path: Path) -> None:
    fixtures.assert_pass(
        manifest_hygiene, tmp_path, needle="3 third-party dependency declarations checked"
    )


def test_default_features_on_next_line(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        fixtures.add_dep(
            root / "crates/gloss-app/Cargo.toml",
            'tokio = { version = "1", features = [\n    "sync",\n], default-features = false }',
        )
        # 跨行条目本身带使用证据：本用例只钉“特性字段检查不误报跨行”，
        # 不让依赖无死条目检查分叉结论。
        (root / "crates/gloss-app/src").mkdir(exist_ok=True)
        (root / "crates/gloss-app/src/tokio_probe.rs").write_text(
            "let _rt = tokio::runtime::Builder::new_current_thread();\n"
        )

    fixtures.assert_pass(
        manifest_hygiene, tmp_path, mutate, "4 third-party dependency declarations checked"
    )


def test_value_broken_across_lines_still_passes(tmp_path: Path) -> None:
    # 键值被换行断开的窄角落：子串判定在单行形态上做，断行不把放行翻成误报
    def mutate(root: Path) -> None:
        fixtures.add_dep(
            root / "crates/gloss-platform/Cargo.toml",
            'foo = { version = "1", default-features =\nfalse }',
        )
        (root / "crates/gloss-platform/src").mkdir(exist_ok=True)
        (root / "crates/gloss-platform/src/foo_probe.rs").write_text("let _v = foo::thing();\n")

    fixtures.assert_pass(manifest_hygiene, tmp_path, mutate, "4 third-party")


def test_dep_without_default_features(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        fixtures.add_dep(root / "crates/gloss-app/Cargo.toml", 'pollster = "1.0.1"')

    fixtures.assert_fail(
        manifest_hygiene, tmp_path, mutate, "pollster lacks default-features = false"
    )


def test_crate_version_literal(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        fixtures.replace_line(
            root / "crates/gloss-app/Cargo.toml", "version.workspace = true", 'version = "0.1.0"'
        )

    fixtures.assert_fail(manifest_hygiene, tmp_path, mutate, "misses version.workspace")


def test_edition_literal(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        fixtures.replace_line(
            root / "crates/gloss-platform/Cargo.toml",
            "edition.workspace = true",
            'edition = "2024"',
        )

    fixtures.assert_fail(manifest_hygiene, tmp_path, mutate, "misses edition.workspace")


def test_root_missing_workspace_package(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        fixtures.replace_line(root / "Cargo.toml", 'version = "0.1.0"', "# 版本字面量被挪走了")

    fixtures.assert_fail(manifest_hygiene, tmp_path, mutate, "has no version literal")


def test_unused_dep_rejected(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        fixtures.add_dep(
            root / "crates/gloss-platform/Cargo.toml",
            'serde_json = { version = "1", default-features = false }',
        )

    fixtures.assert_fail(manifest_hygiene, tmp_path, mutate, "no dead dependencies")


def test_deps_allow_marker_passes(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        fixtures.add_dep(
            root / "crates/gloss-platform/Cargo.toml",
            'serde_json = { version = "1", default-features = false } # deps:allow 自测阴性对照',
        )

    fixtures.assert_pass(manifest_hygiene, tmp_path, mutate, "no dead dependencies (")


def test_deps_allow_marker_without_reason_flagged(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        fixtures.add_dep(
            root / "crates/gloss-platform/Cargo.toml",
            'serde_json = { version = "1", default-features = false } # deps:allow',
        )

    _, fails = fixtures.run_case(manifest_hygiene, tmp_path, mutate)
    assert any("marker without a reason" in f for f in fails)
    assert not any("no dead dependencies" in f for f in fails)


def test_multiline_dep_bare_marker_mid_entry_flagged(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        fixtures.add_dep(
            root / "crates/gloss-platform/Cargo.toml",
            'serde_json = {\n    version = "1", default-features = false, # deps:allow\n}',
        )

    _, fails = fixtures.run_case(manifest_hygiene, tmp_path, mutate)
    assert any("marker without a reason" in f for f in fails)
    assert not any("no dead dependencies" in f for f in fails)
    assert not any("lacks default-features" in f for f in fails)


def test_multiline_dep_marker_with_reason_passes(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        fixtures.add_dep(
            root / "crates/gloss-platform/Cargo.toml",
            'serde_json = {\n    version = "1", default-features = false, # deps:allow 阴性对照\n}',
        )

    fixtures.assert_pass(manifest_hygiene, tmp_path, mutate, "1 deps:allow")


def test_dash_key_unused(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        fixtures.add_dep(
            root / "crates/gloss-platform/Cargo.toml",
            'crossbeam-channel = { version = "0.5", default-features = false }',
        )

    fixtures.assert_fail(manifest_hygiene, tmp_path, mutate, "no dead dependencies")


def test_dash_key_used_after_underscore(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        fixtures.add_dep(
            root / "crates/gloss-platform/Cargo.toml",
            'crossbeam-channel = { version = "0.5", default-features = false }',
        )
        (root / "crates/gloss-platform/src/queue.rs").write_text(
            "let (tx, _rx) = crossbeam_channel::unbounded();\n"
        )

    fixtures.assert_pass(manifest_hygiene, tmp_path, mutate, "0 deps:allow")


def test_dev_dep_unused_rejected(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        p = root / "crates/gloss-platform/Cargo.toml"
        with open(p, "a", encoding="utf-8") as fh:
            fh.write(
                '\n[dev-dependencies]\nserde_json = { version = "1", default-features = false }\n'
            )

    fixtures.assert_fail(manifest_hygiene, tmp_path, mutate, "no dead dependencies")


def test_path_dep_unused_not_punished(tmp_path: Path) -> None:
    fixtures.assert_pass(manifest_hygiene, tmp_path, needle="0 deps:allow")


def test_root_manifest_unused_dep_rejected(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        fixtures.add_dep(
            root / "Cargo.toml", 'serde_json = { version = "1", default-features = false }'
        )

    fixtures.assert_fail(manifest_hygiene, tmp_path, mutate, "no dead dependencies")


def test_root_manifest_src_evidence_passes(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        fixtures.add_dep(
            root / "Cargo.toml", 'serde_json = { version = "1", default-features = false }'
        )
        (root / "src").mkdir(exist_ok=True)
        (root / "src/main.rs").write_text(
            'fn main() {\n    let _: u64 = serde_json::from_str("1").unwrap();\n}\n'
        )

    fixtures.assert_pass(manifest_hygiene, tmp_path, mutate, "0 deps:allow")


def test_use_and_reexport_forms_count_as_evidence(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        fixtures.add_dep(
            root / "crates/gloss-platform/Cargo.toml",
            'serde_json = { version = "1", default-features = false }',
        )
        (root / "crates/gloss-platform/src/reexports.rs").write_text(
            "pub use serde_json;\nuse serde_json as json;\n"
        )

    fixtures.assert_pass(manifest_hygiene, tmp_path, mutate, "0 deps:allow")


def test_macro_form_counts_as_evidence(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        fixtures.add_dep(
            root / "crates/gloss-platform/Cargo.toml",
            'serde_json = { version = "1", default-features = false }',
        )
        (root / "crates/gloss-platform/src/macro_use.rs").write_text("let _v = serde_json!(1);\n")

    fixtures.assert_pass(manifest_hygiene, tmp_path, mutate, "0 deps:allow")


def test_tests_dir_evidence_counts(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        fixtures.add_dep(
            root / "crates/gloss-platform/Cargo.toml",
            'serde_json = { version = "1", default-features = false }',
        )
        (root / "crates/gloss-platform/tests").mkdir(exist_ok=True)
        (root / "crates/gloss-platform/tests/usage.rs").write_text("use serde_json;\n")

    fixtures.assert_pass(manifest_hygiene, tmp_path, mutate, "0 deps:allow")


def test_unclosed_dep_entry_at_eof_still_emitted(tmp_path: Path) -> None:
    # 依赖条目在文件尾仍待定（花括号未闭合）时脚本语言（Python）版选择补发判罚，
    # 旧壳层实现缺收尾逻辑会静默丢弃——这是有意的审慎分歧，别“对齐”回去
    def mutate(root: Path) -> None:
        with open(root / "crates/gloss-platform/Cargo.toml", "a", encoding="utf-8") as fh:
            fh.write('\n[dev-dependencies]\nserde_json = { version = "1"\n')

    fixtures.assert_fail(
        manifest_hygiene, tmp_path, mutate, "serde_json lacks default-features = false"
    )
