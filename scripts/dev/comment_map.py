#!/usr/bin/env python3
"""注释记录命名映射：源文件与注释记录文件名之间双向映射的单一事实源，
命名助手（本脚本命令行）与记录名门禁共用同一函数。映射细则、撞名裁决与
登记表住记录文件，本文件头只留总纲：裸名默认取源文件名去扩展名，汇编模
块文件取父目录名作前缀，跨文件同名按语义家族前缀消歧并在登记表显式登记，
登记表是唯一裁决处，未登记的撞名由体检与门禁判罚；每个源文件对应决策与
测试一对记录名，测试目标与内联测试同规，另有少量自由主题记录（登记表、
豁免留痕、重构记录）不锚定单一源文件，同样在登记表之列。

用法：python3 scripts/dev/comment_map.py <path>，打印规范记录路径；
python3 scripts/dev/comment_map.py --ensure <path>，缺档时按标准前言建档；
python3 scripts/dev/comment_map.py --reverse <record>，反查源文件路径；
python3 scripts/dev/comment_map.py --doctor --fix，全量体检并删除陈旧记录。

退出码：零通过，一发现待处理项，二用法错误，三无法完成判定。
"""

from __future__ import annotations

import os
import subprocess
import sys

RECORD_DIR = os.path.join("docs", "comments")
RECORD_SUFFIX = "_comment.md"
TESTS_INFIX = "_tests"
MOD_STEM = "mod"

# 语义家族前缀登记表：源文件相对路径到记录裸名的全覆盖映射。
# 撞名组内每个成员都必须在此登记（含暂无记录的成员），未登记的撞名会被
# 体检与门禁判罚；登记即裁决，新增撞名时先补表再建记录。
FAMILY_STEMS: dict[str, str] = {
    # 根清单与各分包清单撞名：裸名（Cargo）留给根包
    "Cargo.toml": "Cargo",
    "crates/gloss-app/Cargo.toml": "gloss-app_Cargo",
    "crates/gloss-core/Cargo.toml": "gloss-core_Cargo",
    "crates/gloss-eval/Cargo.toml": "gloss-eval_Cargo",
    "crates/gloss-platform/Cargo.toml": "gloss-platform_Cargo",
    # 根自述与图标资产自述撞名
    "README.md": "README",
    "assets/icons/README.md": "icons_README",
    # 约束门禁与语言规范库的包初始化脚本撞名
    "scripts/hooks/language_spec/__init__.py": "language_spec_init",
    # 应用入口、评测命令行入口与站点脚本入口三方撞名
    "src/main.rs": "main",
    "crates/gloss-eval/src/main.rs": "eval_main",
    "web/assets/main.js": "web_main",
    # 站点样式与站点页面按家族前缀登记（应用样式目录化后无撞名）
    "web/assets/style.css": "web_style",
    "web/index.html": "web_index",
    # 应用样式色板与代码高亮色板撞名：语义色板按家族前缀登记
    "crates/gloss-app/src/ui/style/palette.rs": "style_palette",
    "crates/gloss-app/src/ui/code_hl/palette.rs": "code_hl_palette",
    # 性能脚本与评测指标模块撞名
    "scripts/perf/metrics.py": "metrics",
    "crates/gloss-eval/src/metrics.rs": "eval_metrics",
    # 核心库数据源派生模块与约束门禁语言规范域撞名：约束域带家族前缀
    "crates/gloss-core/src/pipeline/langspec.rs": "langspec",
    "scripts/hooks/constraints/langspec.py": "constraints_langspec",
    # 集成测试目标与运行时管线模块撞名：测试目标带集成前缀
    "crates/gloss-app/tests/pipeline.rs": "pipeline_integration",
    # 剪贴板家族：选区剪贴板、剪贴板子模块与浮层图像卡撞名
    "crates/gloss-platform/src/selection/clipboard.rs": "selection_clipboard",
    "crates/gloss-platform/src/clipboard/image.rs": "clipboard_image",
    "crates/gloss-platform/src/clipboard/mod.rs": "clipboard_mod",
    "crates/gloss-platform/src/clipboard/watch.rs": "clipboard_watch",
    "crates/gloss-app/src/ui/popup/image.rs": "popup_image",
    "crates/gloss-app/src/ui/popup/icon.rs": "popup_icon",
    "crates/gloss-app/src/ui/popup/note.rs": "popup_note",
    "crates/gloss-app/src/ui/popup/mod.rs": "ui_popup_mod",
    # 引擎模块与各包测试桩同名：测试桩家族共用一份记录
    "crates/gloss-core/src/pipeline/engine.rs": "engine",
    "crates/gloss-app/tests/stubs/engine.rs": "stubs_engine",
    "crates/gloss-core/tests/stubs/engine.rs": "stubs_engine",
    # 端口契约模块与各包测试桩同名
    "crates/gloss-core/src/contract/ports.rs": "ports",
    "crates/gloss-app/tests/stubs/ports.rs": "stubs_ports",
    "crates/gloss-core/tests/stubs/ports.rs": "stubs_ports",
    "crates/gloss-platform/tests/stubs/ports.rs": "stubs_ports",
    # 各包测试桩目录同名模块：三包共用一份记录
    "crates/gloss-app/tests/stubs/mod.rs": "stubs_mod",
    "crates/gloss-core/tests/stubs/mod.rs": "stubs_mod",
    "crates/gloss-platform/tests/stubs/mod.rs": "stubs_mod",
    # 修复轮共享骨架与提示词修复区撞名：提示词修复区带家族前缀
    "crates/gloss-core/src/pipeline/repair.rs": "repair",
    "crates/gloss-core/src/pipeline/prompt/repair.rs": "prompt_repair",
    # 任务模块：核心库任务与界面流任务撞名
    "crates/gloss-core/src/domain/task.rs": "task",
    "crates/gloss-app/src/flow/task.rs": "flow_task",
    # 各包库入口撞名：决策与测试记录锚定在（gloss-platform）
    "crates/gloss-platform/src/lib.rs": "lib",
    "crates/gloss-app/src/lib.rs": "app_lib",
    "crates/gloss-core/src/lib.rs": "core_lib",
    "crates/gloss-eval/src/lib.rs": "eval_lib",
}

# 自由主题记录：不锚定单一源文件的记录裸名（登记表、豁免留痕、重构记录）。
FREE_STANDING_RECORDS = frozenset(
    {
        "gate_rules",
        "waivers",
        "eval",
        "refactor-core-llm-app-layering",
        "refactor_test_coupling",
    }
)

# 承载记录的文件面：这些扩展名与特例文件可拥有记录；数据资产
# （提示词、数据集、基线）不在内，为其建档前先在此扩面。
RECORD_SUFFIXES = (".rs", ".py", ".toml", ".yml", ".yaml", ".js", ".css", ".html", ".md")
RECORD_BASENAMES = ("justfile",)
RECORD_PATH_PREFIXES = (".githooks/", ".zcode/hooks/")
EXCLUDED_PREFIXES = ("docs/comments/",)
EXCLUDED_SEGMENTS = ("prompts",)


def carries_records(rel: str) -> bool:
    """源文件是否在记录承载面：按扩展名与特例清单，排除提示词与记录目录。

    助手会话钩子目录（.zcode/hooks/）随仓库跟踪，同样承载记录；其会话
    状态与其余提示词、数据资产不在内。"""
    normal = rel.replace(os.sep, "/")
    if normal.startswith(EXCLUDED_PREFIXES):
        return False
    parts = normal.split("/")
    if any(segment in EXCLUDED_SEGMENTS for segment in parts):
        return False
    if normal.startswith(".zcode/"):
        return normal.startswith(RECORD_PATH_PREFIXES)
    name = os.path.basename(normal)
    if name.endswith(RECORD_SUFFIXES):
        return True
    return name in RECORD_BASENAMES or normal.startswith(RECORD_PATH_PREFIXES)


RECORD_PREAMBLE = (
    "> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。\n"
    "> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；"
    "文件内不留指向本文件的指针。\n"
)

EXIT_PASS = 0
EXIT_FINDINGS = 1
EXIT_USAGE = 2
EXIT_INFRA = 3


def split_record_name(file_name: str) -> str | None:
    """记录文件名到裸名：非 _comment.md 形态返回空，交由调用方判非法名。"""
    if not file_name.endswith(RECORD_SUFFIX):
        return None
    return file_name[: -len(RECORD_SUFFIX)]


def is_legal_bare_name(bare: str) -> bool:
    """裸名合法性：非空且不含点（后缀残留）。伪装后缀（主干后缀化，如源
    文件扩展名被抄进名字）由 residue_stem 对照反向索引判定，主干本身合法
    地以下划线扩展名形态结尾的源文件不受牵连。"""
    return bool(bare) and "." not in bare


# 伪装后缀对照表：下划线形态到源文件扩展名，残留在反向索引里须解析到同扩
# 展名的源文件才算（主干真以该形态结尾的合法名字不判）。
DISGUISED_SUFFIXES = {
    "_rs": ".rs",
    "_py": ".py",
    "_js": ".js",
    "_css": ".css",
    "_html": ".html",
    "_toml": ".toml",
    "_yml": ".yml",
    "_yaml": ".yaml",
    "_md": ".md",
    "_json": ".json",
}


def residue_stem(bare: str, index: dict[str, list[str]]) -> str | None:
    """伪装后缀残留的判定：裸名本身解析得到源文件即合法名字，不做剥除——
    残留只对解析不回任何源的名字有意义（同主干双扩展名源并存时，后者的规
    范名不受前者牵连）。剥掉疑似后缀后解析到同扩展名源文件才算残留，返回
    剥掉后的主干；否则视为合法名字本身。"""
    if bare in index:
        return None
    for suffix, ext in DISGUISED_SUFFIXES.items():
        if not bare.endswith(suffix):
            continue
        stem = bare[: -len(suffix)]
        if any(rel.endswith(ext) for rel in index.get(stem, ())):
            return stem
    return None


def base_stem(rel: str) -> str:
    """默认裸名：去扩展名；mod.rs 取父目录名加 _mod。"""
    stem = os.path.splitext(os.path.basename(rel))[0]
    if stem == MOD_STEM:
        parent = os.path.basename(os.path.dirname(rel))
        stem = parent + "_mod"
    return stem


def record_stem(rel: str) -> str:
    """最终裸名：家族前缀表优先，缺省走默认规则。"""
    normal = rel.replace(os.sep, "/")
    return FAMILY_STEMS.get(normal) or base_stem(normal)


def record_names(rel: str) -> tuple[str, str]:
    """源文件的一对记录文件名：(决策记录, 测试记录)。"""
    stem = record_stem(rel)
    return stem + RECORD_SUFFIX, stem + TESTS_INFIX + RECORD_SUFFIX


def tracked_files(root: str) -> list[str]:
    out = subprocess.run(
        ["git", "-C", root, "ls-files", "-z"],
        capture_output=True,
        check=True,
    ).stdout
    return [p.decode("utf-8", "replace") for p in out.split(b"\0") if p]


def build_index(tracked: list[str]) -> dict[str, list[str]]:
    """裸名到源文件路径的反向索引：一对裸名（决策与测试）覆盖同 stem 家族
    全部成员。"""
    index: dict[str, list[str]] = {}
    for rel in tracked:
        if not carries_records(rel):
            continue
        stem = record_stem(rel)
        index.setdefault(stem, []).append(rel)
        index.setdefault(stem + TESTS_INFIX, []).append(rel)
    return index


def stem_groups(tracked: list[str]) -> dict[str, list[str]]:
    """最终裸名到源文件的分组：撞名判罚看组内成员是否已登记消歧。"""
    groups: dict[str, list[str]] = {}
    for rel in tracked:
        if not carries_records(rel):
            continue
        groups.setdefault(record_stem(rel), []).append(rel)
    return groups


def unregistered_collisions(tracked: list[str]) -> list[tuple[str, list[str]]]:
    """未登记的裸名撞名组：同 stem 多成员且存在未登记成员。登记表的不变量
    是撞名组每个成员都须登记——部分登记的组里，未登记成员的规范记录名与
    已登记成员相同，映射与判罚都会落错文件，同样判罚。"""
    out = []
    for stem, members in sorted(stem_groups(tracked).items()):
        if len(members) < 2:
            continue
        if all(rel.replace(os.sep, "/") in FAMILY_STEMS for rel in members):
            continue
        out.append((stem, sorted(members)))
    return out


def free_standing_shadowing(tracked: list[str]) -> list[str]:
    """自由主题裸名与源文件裸名互相遮蔽的组：登记表成员撞上源文件 stem。"""
    stems = stem_groups(tracked)
    return [stem for stem in sorted(FREE_STANDING_RECORDS) if stem in stems]


def collision_findings(tracked: list[str]) -> list[str]:
    """撞名问题的文本清单（doctor 用）：判定住两个结构化入口。"""
    problems = [
        "stem collision on %r without family prefixes: %s" % (stem, ", ".join(members))
        for stem, members in unregistered_collisions(tracked)
    ]
    problems.extend(
        "free-standing record name %r is also a source stem; register family prefixes" % stem
        for stem in free_standing_shadowing(tracked)
    )
    return problems


def record_paths(tracked: list[str], root: str | None = None) -> list[str]:
    """记录清单：跟踪清单里的记录，加上磁盘上的记录（入库前 doctor 用）。"""
    found = {rel for rel in tracked if rel.startswith(RECORD_DIR + "/")}
    if root is not None:
        disk = os.path.join(root, RECORD_DIR)
        if os.path.isdir(disk):
            for name in sorted(os.listdir(disk)):
                if name.endswith(".md"):
                    found.add(RECORD_DIR + "/" + name)
    return sorted(found)


def doctor(tracked: list[str], root: str | None = None) -> tuple[list[str], list[str]]:
    """全量体检：返回 (非法名, 陈旧记录) 两类问题清单，撞名单列。
    非法名条目带记录路径，陈旧条目带记录路径与补救。"""
    illegal = []
    stale = []
    index = build_index(tracked)
    for rel in record_paths(tracked, root):
        bare = split_record_name(os.path.basename(rel))
        if bare is None or not is_legal_bare_name(bare) or residue_stem(bare, index):
            illegal.append(rel)
            continue
        if bare not in index and bare not in FREE_STANDING_RECORDS:
            stale.append(rel)
    return illegal, stale


def detect_root() -> str:
    """仓库根探测：取调用方工作目录的版本库顶层，脱离版本库时退回工作目录。"""
    try:
        out = subprocess.run(
            ["git", "rev-parse", "--show-toplevel"], capture_output=True, check=True
        ).stdout
        return out.decode("utf-8", "replace").strip() or os.getcwd()
    except (OSError, subprocess.CalledProcessError):
        return os.getcwd()


def main(argv: list[str]) -> int:
    root = detect_root()
    args = list(argv)
    fix = "--fix" in args
    args = [a for a in args if a != "--fix"]
    ensure = "--ensure" in args
    args = [a for a in args if a != "--ensure"]
    reverse = "--reverse" in args
    args = [a for a in args if a != "--reverse"]
    doctor_mode = "--doctor" in args
    args = [a for a in args if a != "--doctor"]

    try:
        tracked = tracked_files(root)
    except (OSError, subprocess.CalledProcessError) as exc:
        print("cannot enumerate tracked files: %s" % exc, file=sys.stderr)
        return EXIT_INFRA

    if doctor_mode:
        if args:
            print("usage: comment_map.py --doctor [--fix]", file=sys.stderr)
            return EXIT_USAGE
        return run_doctor(root, tracked, fix)
    if reverse:
        if len(args) != 1:
            print("usage: comment_map.py --reverse <记录名>", file=sys.stderr)
            return EXIT_USAGE
        return run_reverse(tracked, args[0])
    if len(args) != 1:
        print(
            "usage: comment_map.py [--ensure] <源文件路径> | --reverse <记录名> | --doctor [--fix]",
            file=sys.stderr,
        )
        return EXIT_USAGE
    return run_name(root, tracked, args[0], ensure)


def run_name(root: str, tracked: list[str], source: str, ensure: bool) -> int:
    rel = os.path.relpath(os.path.abspath(source), root).replace(os.sep, "/")
    if rel not in tracked:
        print("not a tracked source file: %s" % rel, file=sys.stderr)
        return EXIT_FINDINGS
    if not carries_records(rel):
        print("file type carries no comment records: %s" % rel, file=sys.stderr)
        return EXIT_FINDINGS
    decision, tests = record_names(rel)
    primary = tests if rel.startswith("tests/") or "/tests/" in rel else decision
    target = os.path.join(RECORD_DIR, primary)
    print(target)
    if ensure and not os.path.exists(os.path.join(root, target)):
        header = "# %s 决策记录\n\n" % record_stem(rel)
        with open(os.path.join(root, target), "w", encoding="utf-8") as fh:
            fh.write(header + RECORD_PREAMBLE)
        print("created: %s" % target)
    return EXIT_PASS


def run_reverse(tracked: list[str], record_name: str) -> int:
    bare = split_record_name(os.path.basename(record_name))
    if bare is None:
        bare = record_name
    index = build_index(tracked)
    if not is_legal_bare_name(bare) or residue_stem(bare, index):
        print("illegal record name: %s" % record_name, file=sys.stderr)
        return EXIT_FINDINGS
    sources = index.get(bare, [])
    if not sources:
        hint = " (free-standing topic record)" if bare in FREE_STANDING_RECORDS else ""
        print("no source file maps to %s%s" % (record_name, hint), file=sys.stderr)
        return EXIT_FINDINGS
    for rel in sorted(sources):
        print(rel)
    return EXIT_PASS


def run_doctor(root: str, tracked: list[str], fix: bool) -> int:
    illegal, stale = doctor(tracked, root)
    collisions = collision_findings(tracked)
    for rel in illegal:
        print("illegal record name: %s" % rel)
    for rel in stale:
        print("stale record (no source file): %s" % rel)
    for item in collisions:
        print("collision: %s" % item)
    total = len(illegal) + len(stale) + len(collisions)
    if not total:
        print("ok: record naming clean (%d records)" % len(record_paths(tracked, root)))
        return EXIT_PASS
    if fix:
        for rel in stale:
            os.remove(os.path.join(root, rel))
            print("deleted stale record: %s" % rel)
        remaining = len(illegal) + len(collisions)
        if remaining:
            print(
                "fix deleted %d stale records; %d items need manual work" % (len(stale), remaining)
            )
            return EXIT_FINDINGS
        print("ok: stale records deleted")
        return EXIT_PASS
    print("%d items found (dry run; rerun with --fix to delete stale records)" % total)
    return EXIT_FINDINGS


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
