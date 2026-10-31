"""约束门禁单测的共享夹具：最小工作区基线、清单与文本变异助手、按领域模块
驱动的用例助手；各领域单测经用例助手建夹具、变异后运行被测模块，断言通过
与失败两个清单。夹具建在测试框架（pytest）的临时目录，残留标记用例另建一
次性版本库。
"""

from __future__ import annotations

import shutil
from collections.abc import Callable
from pathlib import Path
from types import ModuleType


def insert_after_section(path: Path, section: str, text: str) -> None:
    lines = path.read_text(encoding="utf-8").split("\n")
    out = []
    for line in lines:
        out.append(line)
        if line == section:
            out.append(text)
    path.write_text("\n".join(out), encoding="utf-8")


def replace_line(path: Path, old: str, new: str) -> None:
    lines = path.read_text(encoding="utf-8").split("\n")
    lines = [new if line == old else line for line in lines]
    path.write_text("\n".join(lines), encoding="utf-8")


def write_manifest(path: Path, name: str, deps: str = "") -> None:
    content = (
        '[package]\nname = "%s"\nversion.workspace = true\n'
        "edition.workspace = true\n\n[dependencies]\n\n[lints]\nworkspace = true" % name
    )
    path.write_text(content, encoding="utf-8")
    if deps:
        insert_after_section(path, "[dependencies]", deps)


def new_fixture(root: Path, agents_body: str | None = None) -> None:
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
    # 语言规范判罚树的夹具：语言规范域按判罚树扫描，判罚面为空按失败关闭退出，
    # 基线夹具须携带最小判罚文件（无字符面字面量，字符事实自数据面派生）
    judge_file = root / "crates/gloss-core/src/pipeline/lang_gate.rs"
    judge_file.parent.mkdir(parents=True)
    judge_file.write_text(
        "//! 正文判定桩：字符事实自数据源派生。\npub fn judge(c: char) -> bool {\n"
        "    c.is_alphabetic()\n}\n",
        encoding="utf-8",
    )
    # 语言规范数据源夹具：语言规范域的判罚模式自数据面动态构建，数据源缺席
    # 即失败关闭，基线夹具须携带最小数据面（面成员只作扫描面，不判语言）
    langspec_dir = root / "crates/gloss-core/langspec"
    langspec_dir.mkdir(parents=True)
    (langspec_dir / "zh_en.toml").write_text(
        '[zh]\npunct = "，。"\nattached_after_digit = "℃"\nattached_before_digit = "￥"\n\n'
        '[en]\npunct = "."\n\n[shared]\nmath = "="\n'
        'attached_after_digit = "%"\nattached_before_digit = "¥"\n'
        'attached_between_digits = "."\ndash_ellipsis = "…"\ncurly_quotes = "“”"\n'
        'sentence_finals = "。!"\nemoji_bare = "✅"\nemoji_variants = "\\uFE0F"\n',
        encoding="utf-8",
    )
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


def add_dep(path: Path, text: str) -> None:
    insert_after_section(path, "[dependencies]", text)


def run_case(
    module: ModuleType, tmp_path: Path, mutate: Callable[[Path], None] | None = None
) -> tuple[list[str], list[str]]:
    new_fixture(tmp_path)
    if mutate is not None:
        mutate(tmp_path)
    return module.run(str(tmp_path))


def assert_pass(
    module: ModuleType,
    tmp_path: Path,
    mutate: Callable[[Path], None] | None = None,
    needle: str | None = None,
) -> None:
    oks, fails = run_case(module, tmp_path, mutate)
    assert fails == []
    if needle is not None:
        assert any(needle in o for o in oks)


def assert_fail(
    module: ModuleType,
    tmp_path: Path,
    mutate: Callable[[Path], None] | None = None,
    needle: str | None = None,
) -> None:
    oks, fails = run_case(module, tmp_path, mutate)
    assert fails
    if needle is not None:
        assert needle in "\n".join(fails)
