"""约束门禁·界面样式域的单测：四条边界逐条覆盖——语义色内联构造、明暗态
直读、字体族内联构造与数值设计常数必须被拒，属主文件、带缘由的放行标记与
内联测试模块放行，缺缘由的放行标记判罚。
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from constraints import fixtures, ui_style  # noqa: E402

UI = "crates/gloss-app/src/ui"


def test_inline_color_outside_the_palette(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        (root / UI).mkdir(parents=True)
        (root / f"{UI}/rogue.rs").write_text("let color = egui::Color32::from_rgb(1, 2, 3);\n")

    fixtures.assert_fail(ui_style, tmp_path, mutate, "constructs a semantic color inline")


def test_palette_owner_and_marked_lines_pass(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        (root / f"{UI}/style").mkdir(parents=True)
        (root / f"{UI}/style/palette.rs").write_text(
            "pub const C: Color32 = Color32::from_rgb(1, 2, 3);\n"
        )
        (root / f"{UI}/draw.rs").write_text(
            "let c = Color32::from_gray(7); // style:allow 阴性对照\n"
        )

    fixtures.assert_pass(ui_style, tmp_path, mutate, "0 inline colors")


def test_dark_mode_outside_the_resolution_point(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        (root / UI).mkdir(parents=True)
        (root / f"{UI}/view.rs").write_text("let dark = ui.visuals().dark_mode;\n")

    fixtures.assert_fail(ui_style, tmp_path, mutate, "reads the appearance mode directly")


def test_dark_mode_owner_passes(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        (root / f"{UI}/style").mkdir(parents=True)
        (root / f"{UI}/style/mod.rs").write_text(
            "fn palette(ui: &Ui) -> &Palette {\n    if ui.visuals().dark_mode {\n"
        )

    fixtures.assert_pass(ui_style, tmp_path, mutate, "0 direct reads")


def test_font_construction_outside_the_owners(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        (root / UI).mkdir(parents=True)
        (root / f"{UI}/badge.rs").write_text(
            "let id = egui::FontId::new(12.0, egui::FontFamily::Proportional);\n"
        )

    fixtures.assert_fail(ui_style, tmp_path, mutate, "constructs a font family inline")


def test_font_owners_and_test_modules_pass(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        (root / f"{UI}/style").mkdir(parents=True)
        (root / f"{UI}/fonts").mkdir(parents=True)
        (root / f"{UI}/style/typography.rs").write_text(
            "pub fn serif(px: f32) -> FontId {\n    FontId::new(px, family())\n}\n"
        )
        (root / f"{UI}/fonts/imp.rs").write_text(
            "let chain = vec![FontFamily::Name(name.into())];\n"
        )
        (root / f"{UI}/view.rs").write_text(
            "#[cfg(test)]\nmod t {\n    let id = egui::FontId::monospace(12.0);\n}\n"
        )

    fixtures.assert_pass(ui_style, tmp_path, mutate, "0 inline builds")


def test_numeric_constant_outside_style(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        (root / UI).mkdir(parents=True)
        (root / f"{UI}/view.rs").write_text("const GAP: f32 = 8.0;\n")

    fixtures.assert_fail(ui_style, tmp_path, mutate, "declares a constant with a numeric literal")


def test_token_arithmetic_and_owner_constants_pass(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        (root / f"{UI}/style").mkdir(parents=True)
        (root / f"{UI}/style/geometry.rs").write_text("pub const SEAL_SIZE: f32 = rem(1.75);\n")
        (root / f"{UI}/view.rs").write_text(
            "const RESERVE: f32 = geometry::overlay::FOOTER_HEIGHT + space::PARAGRAPH;\n"
            "const KNOB: Color32 = Color32::WHITE;\n"
        )

    fixtures.assert_pass(ui_style, tmp_path, mutate, "0 strays")


def test_whitelisted_non_design_files_pass(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        (root / f"{UI}/popup").mkdir(parents=True)
        (root / f"{UI}/code_hl").mkdir(parents=True)
        (root / f"{UI}/popup/icon.rs").write_text("const ICON_MARGIN: u32 = 100;\n")
        (root / f"{UI}/code_hl/mod.rs").write_text('const TABLE: &str = "x1";\n')

    fixtures.assert_pass(ui_style, tmp_path, mutate, "0 strays")


def test_allow_marker_without_reason_flagged(tmp_path: Path) -> None:
    def mutate(root: Path) -> None:
        (root / UI).mkdir(parents=True)
        (root / f"{UI}/view.rs").write_text("const GAP: f32 = 8.0; // style:allow\n")

    fixtures.assert_fail(ui_style, tmp_path, mutate, "style:allow marker without a reason")
