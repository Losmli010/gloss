"""约束门禁的单测：最小工作区夹具加变异函数，逐条覆盖各约束。

被测入口是门禁的运行函数，返回通过与失败两个列表；夹具建在测试框架（pytest）
的临时目录，残留标记用例另建一次性版本库；用法见持续集成的钩子工作流。
"""

import shutil
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import check_constraints  # noqa: E402

CHECKER = Path(__file__).parent / "check_constraints.py"


def insert_after_section(path, section, text):
    lines = path.read_text(encoding="utf-8").split("\n")
    out = []
    for line in lines:
        out.append(line)
        if line == section:
            out.append(text)
    path.write_text("\n".join(out), encoding="utf-8")


def replace_line(path, old, new):
    lines = path.read_text(encoding="utf-8").split("\n")
    lines = [new if line == old else line for line in lines]
    path.write_text("\n".join(lines), encoding="utf-8")


def write_manifest(path, name, deps=""):
    content = (
        '[package]\nname = "%s"\nversion.workspace = true\n'
        "edition.workspace = true\n\n[dependencies]\n\n[lints]\nworkspace = true" % name
    )
    path.write_text(content, encoding="utf-8")
    if deps:
        insert_after_section(path, "[dependencies]", deps)


def new_fixture(root, agents_body=None):
    (root / "crates/gloss-core/src").mkdir(parents=True)
    (root / "crates/gloss-platform").mkdir()
    (root / "crates/gloss-app").mkdir()

    (root / "Cargo.toml").write_text(
        '[package]\nname = "gloss"\nversion.workspace = true\nedition.workspace = true\n'
        "\n[dependencies]\n\n[workspace]\n"
        'members = ["crates/gloss-core", "crates/gloss-platform", "crates/gloss-app"]\n'
        '\n[workspace.package]\nversion = "0.1.0"\nedition = "2024"',
        encoding="utf-8",
    )
    insert_after_section(
        root / "Cargo.toml", "[dependencies]", 'gloss-core = { path = "crates/gloss-core" }'
    )

    write_manifest(
        root / "crates/gloss-core/Cargo.toml",
        "gloss-core",
        'serde = { version = "1", default-features = false }\n'
        'tracing = { version = "0.1", default-features = false }',
    )
    write_manifest(
        root / "crates/gloss-platform/Cargo.toml",
        "gloss-platform",
        'gloss-core = { path = "../gloss-core" }\n'
        'toml = { version = "0.8", default-features = false }',
    )
    write_manifest(
        root / "crates/gloss-app/Cargo.toml",
        "gloss-app",
        'gloss-core = { path = "../gloss-core" }\ngloss-platform = { path = "../gloss-platform" }',
    )

    (root / "crates/gloss-core/src/log.rs").write_text('tracing::info!("ready");\n')
    # 第三方依赖的使用证据（依赖无死条目检查的基线）：
    # 核心库（gloss-core）的序列化库（serde）经派生宏使用，平台适配层（gloss-platform）的配置解析经完整路径使用。
    (root / "crates/gloss-core/src/lib.rs").write_text(
        "#[derive(serde::Deserialize)]\npub struct Config;\n"
    )
    (root / "crates/gloss-platform/src").mkdir()
    (root / "crates/gloss-platform/src/lib.rs").write_text(
        'let _v: String = toml::from_str("k = 1").unwrap();\n'
    )

    # 桩副本目录夹具：让“桩副本一致”检查有可比对象
    for crate in ("gloss-core", "gloss-app", "gloss-platform"):
        (root / "crates" / crate / "tests/stubs").mkdir(parents=True)
    engine = root / "crates/gloss-core/tests/stubs/engine.rs"
    engine.write_text("engine stub\n")
    shutil.copyfile(engine, root / "crates/gloss-app/tests/stubs/engine.rs")
    for crate in ("gloss-core", "gloss-app"):
        (root / "crates" / crate / "tests/stubs/ports.rs").write_text(
            "/// 内存版配置存储桩\npub struct MemoryConfigStore;\n"
        )
    (root / "crates/gloss-platform/tests/stubs/ports.rs").write_text(
        "/// 内存版配置存储桩\npub struct MemoryConfigStore;\n"
    )

    if agents_body is not None:
        (root / "AGENTS.md").write_text(agents_body + "\n", encoding="utf-8")
    else:
        (root / "AGENTS.md").write_text(
            "gloss、gloss-core、gloss-platform、gloss-app 的依赖方向见下。\n", encoding="utf-8"
        )


def run_case(tmp_path, mutate=None):
    new_fixture(tmp_path)
    if mutate is not None:
        mutate(tmp_path)
    return check_constraints.run(str(tmp_path))


def assert_pass(tmp_path, mutate=None, needle=None):
    oks, fails = run_case(tmp_path, mutate)
    assert fails == []
    if needle is not None:
        assert any(needle in o for o in oks)


def assert_fail(tmp_path, mutate=None, needle=None):
    oks, fails = run_case(tmp_path, mutate)
    assert fails
    if needle is not None:
        out = "\n".join(fails)
        assert needle in out


def add_dep(path, text):
    insert_after_section(path, "[dependencies]", text)


# ---- 合法用例（基线与易误报形态）----


def test_baseline_workspace_all_green(tmp_path):
    assert_pass(tmp_path, needle="3 third-party dependency declarations checked")


def test_default_features_on_next_line(tmp_path):
    def mutate(root):
        add_dep(
            root / "crates/gloss-app/Cargo.toml",
            'tokio = { version = "1", features = [\n    "sync",\n], default-features = false }',
        )
        # 跨行条目本身带使用证据：本用例只钉“特性字段检查不误报跨行”，
        # 不让依赖无死条目检查分叉结论。
        (root / "crates/gloss-app/src").mkdir(exist_ok=True)
        (root / "crates/gloss-app/src/tokio_probe.rs").write_text(
            "let _rt = tokio::runtime::Builder::new_current_thread();\n"
        )

    assert_pass(tmp_path, mutate, "4 third-party dependency declarations checked")


def test_log_arguments_english_with_chinese_comments(tmp_path):
    assert_pass(tmp_path, needle="0 violations")


def test_core_tracing_is_the_only_log_exit(tmp_path):
    assert_pass(tmp_path)


# ---- 约束“依赖方向”----


def test_core_depends_on_platform_rejected(tmp_path):
    def mutate(root):
        add_dep(
            root / "crates/gloss-core/Cargo.toml", 'gloss-platform = { path = "../gloss-platform" }'
        )

    assert_fail(tmp_path, mutate, "must not depend on gloss-platform")


def test_core_depends_on_winit_redline(tmp_path):
    def mutate(root):
        add_dep(
            root / "crates/gloss-core/Cargo.toml",
            'winit = { version = "0.30", default-features = false }',
        )

    assert_fail(tmp_path, mutate, "red-line, forbidden")


def test_core_depends_on_wgpu_core_prefix_match(tmp_path):
    def mutate(root):
        add_dep(
            root / "crates/gloss-core/Cargo.toml",
            'wgpu-core = { version = "30", default-features = false }',
        )

    assert_fail(tmp_path, mutate, "red-line, forbidden")


def test_platform_depends_on_app_rejected(tmp_path):
    def mutate(root):
        add_dep(root / "crates/gloss-platform/Cargo.toml", 'gloss-app = { path = "../gloss-app" }')

    assert_fail(tmp_path, mutate, "must not depend on gloss-app")


def test_depends_on_unregistered_crate(tmp_path):
    def mutate(root):
        (root / "crates/gloss-util").mkdir()
        write_manifest(root / "crates/gloss-util/Cargo.toml", "gloss-util")
        add_dep(root / "crates/gloss-app/Cargo.toml", 'gloss-util = { path = "../gloss-util" }')

    assert_fail(tmp_path, mutate, "must not depend on gloss-util")


def test_crate_missing_from_agents_md(tmp_path):
    def mutate(root):
        (root / "AGENTS.md").write_text("gloss 与 gloss-core 的依赖方向见下。\n", encoding="utf-8")

    assert_fail(tmp_path, mutate, "missing from")


def test_engine_stub_drift(tmp_path):
    def mutate(root):
        with open(root / "crates/gloss-app/tests/stubs/engine.rs", "a") as fh:
            fh.write("\n// drift\n")

    assert_fail(tmp_path, mutate, "stub copy drift")


def test_ports_stub_section_drift(tmp_path):
    def mutate(root):
        replace_line(
            root / "crates/gloss-app/tests/stubs/ports.rs",
            "pub struct MemoryConfigStore;",
            "pub struct MemoryConfigStore drift;",
        )

    assert_fail(tmp_path, mutate, "stub copy drift")


# ---- 约束“日志统一出口”----


def test_app_depends_on_tracing(tmp_path):
    def mutate(root):
        add_dep(
            root / "crates/gloss-app/Cargo.toml",
            'tracing = { version = "0.1", default-features = false }',
        )

    assert_fail(tmp_path, mutate, "use gloss_core::log only")


def test_platform_depends_on_tracing_subscriber(tmp_path):
    def mutate(root):
        add_dep(
            root / "crates/gloss-platform/Cargo.toml",
            'tracing-subscriber = { version = "0.3", default-features = false }',
        )

    assert_fail(tmp_path, mutate, "use gloss_core::log only")


# ---- 约束“日志一律英文”----


def test_log_message_in_chinese(tmp_path):
    def mutate(root):
        (root / "crates/gloss-core/src/log.rs").write_text('info!("找不到配置文件");\n')

    assert_fail(tmp_path, mutate, "log macro argument with non-ASCII")


def test_log_message_multiline_chinese(tmp_path):
    def mutate(root):
        (root / "crates/gloss-core/src/log.rs").write_text(
            'info!(\n    gen = 1,\n    "读取失败"\n);\n'
        )

    assert_fail(tmp_path, mutate, "log macro argument with non-ASCII")


# ---- 约束“界面库（egui）上下文统一装入点”----


def test_bare_egui_context(tmp_path):
    def mutate(root):
        (root / "crates/gloss-app/src").mkdir(exist_ok=True)
        (root / "crates/gloss-app/src/bare.rs").write_text("let ctx = egui::Context::default();\n")

    assert_fail(tmp_path, mutate, "egui context unified load point")


def test_context_with_spaces(tmp_path):
    def mutate(root):
        (root / "crates/gloss-app/src").mkdir(exist_ok=True)
        (root / "crates/gloss-app/src/spaced.rs").write_text("let ctx = Context :: default ();\n")

    assert_fail(tmp_path, mutate, "egui context unified load point")


def test_fonts_written_outside(tmp_path):
    def mutate(root):
        (root / "crates/gloss-app/src").mkdir(exist_ok=True)
        (root / "crates/gloss-app/src/sneaky_fonts.rs").write_text(
            "config.set_fonts(definitions);\n"
        )

    assert_fail(tmp_path, mutate, "egui context unified load point")


def test_theme_written_outside(tmp_path):
    def mutate(root):
        (root / "crates/gloss-app/src").mkdir(exist_ok=True)
        (root / "crates/gloss-app/src/sneaky_theme.rs").write_text("ctx.set_theme(preference);\n")

    assert_fail(tmp_path, mutate, "egui context unified load point")


def test_allow_context_marker_passes(tmp_path):
    def mutate(root):
        (root / "crates/gloss-app/src").mkdir(exist_ok=True)
        (root / "crates/gloss-app/src/negative_control.rs").write_text(
            "let ctx = Context::default(); // constraints:allow-context 阴性对照\n"
        )

    assert_pass(tmp_path, mutate, "0 bypasses")


def test_context_inside_owner_passes(tmp_path):
    def mutate(root):
        (root / "crates/gloss-app/src/ui").mkdir(parents=True)
        (root / "crates/gloss-app/src/ui/context.rs").write_text("let ctx = Context::default();\n")

    assert_pass(tmp_path, mutate, "0 bypasses")


# ---- 约束“外部函数接口（FFI）声明集中”----


def test_ffi_declared_outside(tmp_path):
    def mutate(root):
        (root / "crates/gloss-platform/src").mkdir(exist_ok=True)
        (root / "crates/gloss-platform/src/sneaky.rs").write_text(
            '#[link(name = "CoreGraphics", kind = "framework")]\n'
            'unsafe extern "C" {\n    fn CGWarpMouseCursorPosition(p: u32) -> i32;\n}\n'
        )

    assert_fail(tmp_path, mutate, "FFI declarations centralized")


def test_ffi_declared_in_owner_passes(tmp_path):
    def mutate(root):
        (root / "crates/gloss-platform/src/ffi").mkdir(parents=True)
        (root / "crates/gloss-platform/src/ffi/cg.rs").write_text(
            '#[link(name = "CoreGraphics", kind = "framework")]\n'
            'unsafe extern "C" {\n    pub(crate) fn CGWarpMouseCursorPosition(p: u32) -> i32;\n}\n'
        )

    assert_pass(tmp_path, mutate, "0 inline declarations")


def test_ffi_callback_definition_not_a_declaration(tmp_path):
    def mutate(root):
        (root / "crates/gloss-platform/src").mkdir(exist_ok=True)
        (root / "crates/gloss-platform/src/callback.rs").write_text(
            'extern "C" fn fire(info: *mut c_void) {\n    let _ = info;\n}\n'
        )

    assert_pass(tmp_path, mutate, "0 inline declarations")


# ---- 约束“版本单点维护”----


def test_crate_version_literal(tmp_path):
    def mutate(root):
        replace_line(
            root / "crates/gloss-app/Cargo.toml", "version.workspace = true", 'version = "0.1.0"'
        )

    assert_fail(tmp_path, mutate, "misses version.workspace")


def test_edition_literal(tmp_path):
    def mutate(root):
        replace_line(
            root / "crates/gloss-platform/Cargo.toml",
            "edition.workspace = true",
            'edition = "2024"',
        )

    assert_fail(tmp_path, mutate, "misses edition.workspace")


def test_root_missing_workspace_package(tmp_path):
    def mutate(root):
        replace_line(root / "Cargo.toml", 'version = "0.1.0"', "# 版本字面量被挪走了")

    assert_fail(tmp_path, mutate, "has no version literal")


# ---- 约束“依赖只开需要的特性”----


def test_dep_without_default_features(tmp_path):
    def mutate(root):
        add_dep(root / "crates/gloss-app/Cargo.toml", 'pollster = "1.0.1"')

    assert_fail(tmp_path, mutate, "pollster lacks default-features = false")


# ---- 约束“依赖无死条目”----


def test_unused_dep_rejected(tmp_path):
    def mutate(root):
        add_dep(
            root / "crates/gloss-platform/Cargo.toml",
            'serde_json = { version = "1", default-features = false }',
        )

    assert_fail(tmp_path, mutate, "no dead dependencies")


def test_deps_allow_marker_passes(tmp_path):
    def mutate(root):
        add_dep(
            root / "crates/gloss-platform/Cargo.toml",
            'serde_json = { version = "1", default-features = false } # deps:allow 自测阴性对照',
        )

    assert_pass(tmp_path, mutate, "no dead dependencies (")


def test_dash_key_unused(tmp_path):
    def mutate(root):
        add_dep(
            root / "crates/gloss-platform/Cargo.toml",
            'crossbeam-channel = { version = "0.5", default-features = false }',
        )

    assert_fail(tmp_path, mutate, "no dead dependencies")


def test_dash_key_used_after_underscore(tmp_path):
    def mutate(root):
        add_dep(
            root / "crates/gloss-platform/Cargo.toml",
            'crossbeam-channel = { version = "0.5", default-features = false }',
        )
        (root / "crates/gloss-platform/src/queue.rs").write_text(
            "let (tx, _rx) = crossbeam_channel::unbounded();\n"
        )

    assert_pass(tmp_path, mutate, "0 deps:allow")


def test_dev_dep_unused_rejected(tmp_path):
    def mutate(root):
        p = root / "crates/gloss-platform/Cargo.toml"
        with open(p, "a", encoding="utf-8") as fh:
            fh.write(
                '\n[dev-dependencies]\nserde_json = { version = "1", default-features = false }\n'
            )

    assert_fail(tmp_path, mutate, "no dead dependencies")


def test_path_dep_unused_not_punished(tmp_path):
    assert_pass(tmp_path, needle="0 deps:allow")


def test_root_manifest_unused_dep_rejected(tmp_path):
    def mutate(root):
        add_dep(root / "Cargo.toml", 'serde_json = { version = "1", default-features = false }')

    assert_fail(tmp_path, mutate, "no dead dependencies")


def test_root_manifest_src_evidence_passes(tmp_path):
    def mutate(root):
        add_dep(root / "Cargo.toml", 'serde_json = { version = "1", default-features = false }')
        (root / "src").mkdir(exist_ok=True)
        (root / "src/main.rs").write_text(
            'fn main() {\n    let _: u64 = serde_json::from_str("1").unwrap();\n}\n'
        )

    assert_pass(tmp_path, mutate, "0 deps:allow")


def test_use_and_reexport_forms_count_as_evidence(tmp_path):
    def mutate(root):
        add_dep(
            root / "crates/gloss-platform/Cargo.toml",
            'serde_json = { version = "1", default-features = false }',
        )
        (root / "crates/gloss-platform/src/reexports.rs").write_text(
            "pub use serde_json;\nuse serde_json as json;\n"
        )

    assert_pass(tmp_path, mutate, "0 deps:allow")


def test_macro_form_counts_as_evidence(tmp_path):
    def mutate(root):
        add_dep(
            root / "crates/gloss-platform/Cargo.toml",
            'serde_json = { version = "1", default-features = false }',
        )
        (root / "crates/gloss-platform/src/macro_use.rs").write_text("let _v = serde_json!(1);\n")

    assert_pass(tmp_path, mutate, "0 deps:allow")


def test_tests_dir_evidence_counts(tmp_path):
    def mutate(root):
        add_dep(
            root / "crates/gloss-platform/Cargo.toml",
            'serde_json = { version = "1", default-features = false }',
        )
        (root / "crates/gloss-platform/tests").mkdir(exist_ok=True)
        (root / "crates/gloss-platform/tests/usage.rs").write_text("use serde_json;\n")

    assert_pass(tmp_path, mutate, "0 deps:allow")


# ---- 约束“文件行数上限”----


def test_overlong_rs_file_rejected(tmp_path):
    def mutate(root):
        (root / "crates/gloss-app/src").mkdir(exist_ok=True)
        (root / "crates/gloss-app/src/big.rs").write_text("let _x = 1;\n" * 520)

    assert_fail(tmp_path, mutate, "line cap")


def test_lines_allow_in_first_five_lines_passes(tmp_path):
    def mutate(root):
        (root / "crates/gloss-app/src").mkdir(exist_ok=True)
        (root / "crates/gloss-app/src/big.rs").write_text(
            "// lines:allow 自测阴性对照\n" + "let _x = 1;\n" * 520
        )

    assert_pass(tmp_path, mutate, "1 lines:allow exemptions")


def test_overlong_test_file_not_punished(tmp_path):
    def mutate(root):
        (root / "crates/gloss-app/tests").mkdir(exist_ok=True)
        (root / "crates/gloss-app/tests/big.rs").write_text("let _x = 1;\n" * 520)

    assert_pass(tmp_path, mutate, "0 lines:allow exemptions")


def test_inline_tests_excluded_from_production_lines(tmp_path):
    def mutate(root):
        (root / "crates/gloss-app/src").mkdir(exist_ok=True)
        (root / "crates/gloss-app/src/mixed.rs").write_text(
            "let _x = 1;\n" * 20 + "#[cfg(test)]\nmod tests {\n" + "let _y = 1;\n" * 520 + "}\n"
        )

    assert_pass(tmp_path, mutate, "0 lines:allow exemptions")


def test_lines_allow_past_header_rejected(tmp_path):
    def mutate(root):
        (root / "crates/gloss-app/src").mkdir(exist_ok=True)
        (root / "crates/gloss-app/src/late_allow.rs").write_text(
            "let _x = 1;\n" * 5
            + "// lines:allow 写在文件头 5 行之外不算豁免\n"
            + "let _x = 1;\n" * 520
        )

    assert_fail(tmp_path, mutate, "line cap")


def test_indented_cfg_test_not_truncated(tmp_path):
    def mutate(root):
        (root / "crates/gloss-app/src").mkdir(exist_ok=True)
        (root / "crates/gloss-app/src/indented_cfg.rs").write_text(
            "let _x = 1;\n" * 19 + "    #[cfg(test)]\n" + "let _y = 1;\n" * 520
        )

    assert_fail(tmp_path, mutate, "line cap")


# ---- 评测资产隔离 ----


def test_production_crate_depends_on_eval_rejected(tmp_path):
    def mutate(root):
        add_dep(root / "crates/gloss-app/Cargo.toml", 'gloss-eval = { path = "../gloss-eval" }')

    # 行号必须来自评测包（gloss-eval）条目自身（插在依赖段标题后的第 7 行），不是上游循环遗留的陈旧值
    _, fails = run_case(tmp_path, mutate)
    assert any(
        "must stay out of the production dependency graph" in f
        and "crates/gloss-app/Cargo.toml:7" in f
        for f in fails
    )


def test_production_code_includes_eval_asset_rejected(tmp_path):
    def mutate(root):
        (root / "crates/gloss-core/src/lib.rs").write_text(
            'const X: &str = include_str!("../../gloss-eval/datasets/classify.jsonl");\n'
        )

    assert_fail(tmp_path, mutate, "embeds a gloss-eval asset into production code")


# ---- 残留任务标记扫描（版本库夹具；上方非版本库夹具用例覆盖自动跳过路径）----
# 约束脚本自身豁免：脚本副本（含各类任务标记字面量）被版本库跟踪，
# “干净版本库通过”用例正常退出即证明其未被自命中。


def test_clean_git_repo_passes(tmp_path):
    new_fixture(tmp_path)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    (tmp_path / "scripts/hooks").mkdir(parents=True)
    shutil.copyfile(CHECKER, tmp_path / "scripts/hooks/check_constraints.py")
    (tmp_path / "src").mkdir()
    (tmp_path / "src/clean.rs").write_text("let x = compute(input);\n")
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    oks, fails = check_constraints.run(str(tmp_path))
    assert fails == []


def test_todo_marker_hit(tmp_path):
    new_fixture(tmp_path)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    (tmp_path / "scripts/hooks").mkdir(parents=True)
    shutil.copyfile(CHECKER, tmp_path / "scripts/hooks/check_constraints.py")
    (tmp_path / "src").mkdir()
    (tmp_path / "src/has_todo.rs").write_text("// TODO: 待办\n")
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    _, fails = check_constraints.run(str(tmp_path))
    assert any("leftover task marker:" in f for f in fails)


def test_fixme_marker_hit(tmp_path):
    new_fixture(tmp_path)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    (tmp_path / "scripts/hooks").mkdir(parents=True)
    shutil.copyfile(CHECKER, tmp_path / "scripts/hooks/check_constraints.py")
    (tmp_path / "src").mkdir()
    (tmp_path / "src/has_fixme.rs").write_text("fixme_marker(); // FIXME\n")
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    _, fails = check_constraints.run(str(tmp_path))
    assert any("leftover task marker:" in f for f in fails)


def test_todo_adjacent_to_cjk_hit(tmp_path):
    # 正则按英文词界（ASCII）语义对齐全词匹配：标记紧贴中文（汉字不算词字符）同样命中
    new_fixture(tmp_path)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    (tmp_path / "scripts/hooks").mkdir(parents=True)
    shutil.copyfile(CHECKER, tmp_path / "scripts/hooks/check_constraints.py")
    (tmp_path / "src").mkdir()
    (tmp_path / "src/cjk_todo.rs").write_text("// TODO记录：待办\n")
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    _, fails = check_constraints.run(str(tmp_path))
    assert any("leftover task marker:" in f for f in fails)


def test_unclosed_dep_entry_at_eof_still_emitted(tmp_path):
    # 依赖条目在文件尾仍待定（花括号未闭合）时脚本语言（Python）版选择补发判罚，
    # 旧壳层实现缺收尾逻辑会静默丢弃——这是有意的审慎分歧，别“对齐”回去
    def mutate(root):
        with open(root / "crates/gloss-platform/Cargo.toml", "a", encoding="utf-8") as fh:
            fh.write('\n[dev-dependencies]\nserde_json = { version = "1"\n')

    assert_fail(tmp_path, mutate, "serde_json lacks default-features = false")


def test_agents_md_uppercase_todo_exempt(tmp_path):
    new_fixture(tmp_path)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    (tmp_path / "scripts/hooks").mkdir(parents=True)
    shutil.copyfile(CHECKER, tmp_path / "scripts/hooks/check_constraints.py")
    (tmp_path / "AGENTS.md").write_text(
        "gloss、gloss-core、gloss-platform、gloss-app 的依赖方向见下。代码不留 TODO 残留标记。\n",
        encoding="utf-8",
    )
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    _, fails = check_constraints.run(str(tmp_path))
    assert fails == []


def test_lowercase_and_word_boundary_no_false_positive(tmp_path):
    new_fixture(tmp_path)
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True, capture_output=True)
    (tmp_path / "scripts/hooks").mkdir(parents=True)
    shutil.copyfile(CHECKER, tmp_path / "scripts/hooks/check_constraints.py")
    (tmp_path / "src").mkdir()
    (tmp_path / "src/boundary.rs").write_text('let todo = "TODOS"; let _x = MY_TODO;\n')
    subprocess.run(["git", "-C", str(tmp_path), "add", "-A"], check=True, capture_output=True)
    _, fails = check_constraints.run(str(tmp_path))
    assert fails == []
