"""约束门禁·架构域的单测：依赖方向允许边、核心库红线、包名登记与评测资产
隔离逐条覆盖——违规必须被拒，合法形态必须放行，判罚行号必须来自条目自身。
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from constraints import architecture, fixtures  # noqa: E402


def test_core_depends_on_platform_rejected(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        fixtures.add_dep(
            root / "crates/gloss-core/Cargo.toml", 'gloss-platform = { path = "../gloss-platform" }'
        )

    fixtures.assert_fail(architecture, tmp_path, mutate, "must not depend on gloss-platform")


def test_core_depends_on_winit_redline(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        fixtures.add_dep(
            root / "crates/gloss-core/Cargo.toml",
            'winit = { version = "0.30", default-features = false }',
        )

    fixtures.assert_fail(architecture, tmp_path, mutate, "red-line, forbidden")


def test_core_depends_on_wgpu_core_prefix_match(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        fixtures.add_dep(
            root / "crates/gloss-core/Cargo.toml",
            'wgpu-core = { version = "30", default-features = false }',
        )

    fixtures.assert_fail(architecture, tmp_path, mutate, "red-line, forbidden")


def test_platform_depends_on_app_rejected(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        fixtures.add_dep(
            root / "crates/gloss-platform/Cargo.toml", 'gloss-app = { path = "../gloss-app" }'
        )

    fixtures.assert_fail(architecture, tmp_path, mutate, "must not depend on gloss-app")


def test_depends_on_unregistered_crate(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        (root / "crates/gloss-util").mkdir()
        fixtures.write_manifest(root / "crates/gloss-util/Cargo.toml", "gloss-util")
        fixtures.add_dep(
            root / "crates/gloss-app/Cargo.toml", 'gloss-util = { path = "../gloss-util" }'
        )

    fixtures.assert_fail(architecture, tmp_path, mutate, "must not depend on gloss-util")


def test_crate_missing_from_agents_md(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        (root / "AGENTS.md").write_text("gloss 与 gloss-core 的依赖方向见下。\n", encoding="utf-8")

    fixtures.assert_fail(architecture, tmp_path, mutate, "missing from")


def test_production_crate_depends_on_eval_rejected(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        fixtures.add_dep(
            root / "crates/gloss-app/Cargo.toml", 'gloss-eval = { path = "../gloss-eval" }'
        )

    # 行号必须来自评测包（gloss-eval）条目自身（插在依赖段标题后的第 7 行），不是上游循环遗留的陈旧值
    _, fails = fixtures.run_case(architecture, tmp_path, mutate)
    assert any(
        "must stay out of the production dependency graph" in f
        and "crates/gloss-app/Cargo.toml:7" in f
        for f in fails
    )


def test_production_code_includes_eval_asset_rejected(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        (root / "crates/gloss-core/src/lib.rs").write_text(
            'const X: &str = include_str!("../../gloss-eval/datasets/classify.jsonl");\n'
        )

    fixtures.assert_fail(
        architecture, tmp_path, mutate, "embeds a gloss-eval asset into production code"
    )
