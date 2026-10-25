"""约束门禁·界面边界域的单测：界面库（egui）上下文统一装入点与跨语言声明
集中逐条覆盖——绕过装入点与就地声明必须被拒，放行标记、属主目录内的建点与
导出函数形式的回调定义放行。
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from constraints import fixtures, ui_boundary  # noqa: E402


def test_bare_egui_context(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        (root / "crates/gloss-app/src").mkdir(exist_ok=True)
        (root / "crates/gloss-app/src/bare.rs").write_text("let ctx = egui::Context::default();\n")

    fixtures.assert_fail(ui_boundary, tmp_path, mutate, "egui context unified load point")


def test_context_with_spaces(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        (root / "crates/gloss-app/src").mkdir(exist_ok=True)
        (root / "crates/gloss-app/src/spaced.rs").write_text("let ctx = Context :: default ();\n")

    fixtures.assert_fail(ui_boundary, tmp_path, mutate, "egui context unified load point")


def test_fonts_written_outside(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        (root / "crates/gloss-app/src").mkdir(exist_ok=True)
        (root / "crates/gloss-app/src/sneaky_fonts.rs").write_text(
            "config.set_fonts(definitions);\n"
        )

    fixtures.assert_fail(ui_boundary, tmp_path, mutate, "egui context unified load point")


def test_theme_written_outside(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        (root / "crates/gloss-app/src").mkdir(exist_ok=True)
        (root / "crates/gloss-app/src/sneaky_theme.rs").write_text("ctx.set_theme(preference);\n")

    fixtures.assert_fail(ui_boundary, tmp_path, mutate, "egui context unified load point")


def test_allow_context_marker_passes(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        (root / "crates/gloss-app/src").mkdir(exist_ok=True)
        (root / "crates/gloss-app/src/negative_control.rs").write_text(
            "let ctx = Context::default(); // constraints:allow-context 阴性对照\n"
        )

    fixtures.assert_pass(ui_boundary, tmp_path, mutate, "0 bypasses")


def test_context_inside_owner_passes(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        (root / "crates/gloss-app/src/ui").mkdir(parents=True)
        (root / "crates/gloss-app/src/ui/context.rs").write_text("let ctx = Context::default();\n")

    fixtures.assert_pass(ui_boundary, tmp_path, mutate, "0 bypasses")


def test_ffi_declared_outside(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        (root / "crates/gloss-platform/src").mkdir(exist_ok=True)
        (root / "crates/gloss-platform/src/sneaky.rs").write_text(
            '#[link(name = "CoreGraphics", kind = "framework")]\n'
            'unsafe extern "C" {\n    fn CGWarpMouseCursorPosition(p: u32) -> i32;\n}\n'
        )

    fixtures.assert_fail(ui_boundary, tmp_path, mutate, "FFI declarations centralized")


def test_ffi_declared_in_owner_passes(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        (root / "crates/gloss-platform/src/ffi").mkdir(parents=True)
        (root / "crates/gloss-platform/src/ffi/cg.rs").write_text(
            '#[link(name = "CoreGraphics", kind = "framework")]\n'
            'unsafe extern "C" {\n    pub(crate) fn CGWarpMouseCursorPosition(p: u32) -> i32;\n}\n'
        )

    fixtures.assert_pass(ui_boundary, tmp_path, mutate, "0 inline declarations")


def test_ffi_callback_definition_not_a_declaration(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        (root / "crates/gloss-platform/src").mkdir(exist_ok=True)
        (root / "crates/gloss-platform/src/callback.rs").write_text(
            'extern "C" fn fire(info: *mut c_void) {\n    let _ = info;\n}\n'
        )

    fixtures.assert_pass(ui_boundary, tmp_path, mutate, "0 inline declarations")
