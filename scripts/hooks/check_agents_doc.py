#!/usr/bin/env python3
"""指令文档引用一致性门禁：校验指令文档里提到的仓库事实确实存在。
本地提交前门禁与持续集成检查共用此脚本，保证本地与远端判定一致。

校验对象为仓库的指令文档，五类引用：
  一、全文任何任务配方的提及必须在配方清单里定义；
  二、行内反引号里的仓库路径（含分隔符）必须存在（文件或目录）；
  三、行内反引号里的裸文件名（配方清单或带常见扩展名）必须在仓库根存在；
  四、全文任何测试目标的提及必须存在对应的测试源码（根包或任一分包）；
  五、仓库各处对质量条目的指名引用必须真的在指令文档的“质量条目总表”段里。

扫描逐行进行，代码块内同样算数；路径只认行内反引号包裹的仓库相对路径。
占位符、仓库外路径、注释语法与链接不校验。路径、受检文件与测试目标都以
版本库跟踪清单为准：工作区里未跟踪的临时目录与本地记录不参与判定，本地
与持续集成才看同一批文件。

用法：以解释器运行本脚本，首参为指令文档路径，缺省为仓库根的指令文档。
"""

import os
import re
import subprocess
import sys

RECIPE_RE = re.compile(r"(?:^|[^A-Za-z0-9_-])just ([a-z][a-z0-9-]*)")
BACKTICK_RE = re.compile(r"`([^`]+)`")
CARGO_TEST_RE = re.compile(r"--test ([A-Za-z0-9_-]+)")
RULE_NAME_RE = re.compile(r"\*\*([^*]+)\*\*")
REF_LINE_RE = re.compile(r"AGENTS\.md.*质量条目“")
REF_NAME_RE = re.compile(r"质量条目“([^”]+)”")
BARE_SUFFIXES = (".md", ".toml", ".py", ".rs", ".yml", ".json")

# 本脚本与其单测按构造就含质量条目引用模式，跳过自命中
SELF_EXEMPT = {"check_agents_doc.py", "check_agents_doc_test.py"}


def scan_lines(doc_path, loader=None):
    if loader is not None:
        return loader.read_text(doc_path).split("\n")
    with open(doc_path, encoding="utf-8") as fh:
        return fh.read().split("\n")


def rule_names(lines):
    """“质量条目总表”段里以 **名字** 起头的条目名。"""
    names = []
    in_section = False
    for line in lines:
        if line.startswith("### 质量条目总表"):
            in_section = True
            continue
        if in_section and line.startswith("## "):
            break
        if in_section:
            names.extend(RULE_NAME_RE.findall(line))
    return names


def scan_targets(root, target, tracked):
    """测试目标是否有对应源码：有跟踪清单时按清单，否则按工作区。"""
    if tracked:
        leaf = ["tests", "%s.rs" % target]
        return any(
            p == "tests/%s.rs" % target or (p.startswith("crates/") and p.split("/")[2:4] == leaf)
            for p in tracked
        )
    candidates = [os.path.join(root, "tests", "%s.rs" % target)]
    crates = os.path.join(root, "crates")
    if os.path.isdir(crates):
        for crate in sorted(os.listdir(crates)):
            candidates.append(os.path.join(crates, crate, "tests", "%s.rs" % target))
    return any(os.path.isfile(c) for c in candidates)


def tracked_files(root):
    """版本库跟踪清单；不是仓库根时返回空表，调用方退回工作区遍历。"""
    try:
        top = subprocess.run(
            ["git", "-C", root, "rev-parse", "--show-toplevel"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()
        if os.path.realpath(top) != os.path.realpath(root):
            return []
        out = subprocess.run(
            ["git", "-C", root, "ls-files"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout
        return [p for p in out.splitlines() if p]
    except (OSError, subprocess.CalledProcessError):
        return []


def repo_path_exists(token, tracked):
    """路径是否在跟踪清单里：目录按前缀、文件按全名。"""
    if token.endswith("/"):
        return any(p.startswith(token) for p in tracked)
    return token in tracked or any(p.startswith(token + "/") for p in tracked)


def ref_file(p):
    """是否参与质量条目引用核对：源码、清单、脚本与工作流，另加任务配方。"""
    name = os.path.basename(p)
    return name.endswith((".rs", ".toml", ".py", ".yml")) or name == "justfile"


def scan_quality_refs(root, names, tracked, loader=None):
    """仓库各处（*.rs / *.toml / *.py / *.yml / justfile）对质量条目的指名引用核对。

    有跟踪清单时只读清单里的文件：工作区里未跟踪的临时目录与本地记录不能
    左右判定，本地与持续集成才看同一批文件；装入器在场时文件读取走共享缓
    存。返回 (violations, ref_files, ref_count)。"""
    violations = []
    ref_files = 0
    ref_count = 0
    if tracked:
        paths = [(os.path.join(root, p), p) for p in tracked if ref_file(p)]
    else:
        paths = []
        for dirpath, dirnames, filenames in os.walk(root):
            dirnames[:] = sorted(d for d in dirnames if d not in ("target", ".git"))
            for name in sorted(filenames):
                if ref_file(name):
                    path = os.path.join(dirpath, name)
                    paths.append((path, os.path.relpath(path, root)))
    for path, rel in paths:
        if os.path.basename(path) in SELF_EXEMPT:
            continue
        ref_files += 1
        if loader is not None:
            lines = loader.read_text(path, errors="replace").split("\n")
        else:
            with open(path, encoding="utf-8", errors="replace") as fh:
                lines = fh.read().split("\n")
        for line in lines:
            if not REF_LINE_RE.search(line):
                continue
            for cited in REF_NAME_RE.findall(line):
                ref_count += 1
                if cited not in names:
                    violations.append(
                        '%s: cites quality entry "%s" — no entry with this name'
                        " in the AGENTS.md quality entry table" % (rel, cited)
                    )
    return violations, ref_files, ref_count


def run(doc_path, loader=None):
    """按五类引用核对 doc_path，返回 (violations, checked, ref_files, ref_count)；
    装入器在场时清单枚举与文件读取走共享缓存，缺省自行读取。"""
    root = os.path.dirname(os.path.abspath(doc_path))
    doc_name = os.path.basename(doc_path)
    tracked = loader.tracked() if loader is not None else tracked_files(root)
    lines = scan_lines(doc_path, loader)
    justfile_lines = None
    justfile_path = os.path.join(root, "justfile")
    if os.path.isfile(justfile_path):
        if loader is not None:
            justfile_lines = loader.read_text(justfile_path).split("\n")
        else:
            with open(justfile_path, encoding="utf-8") as fh:
                justfile_lines = fh.read().split("\n")

    violations = []
    checked = 0

    for line_no, line in enumerate(lines, 1):
        # ---- 任务配方 ----
        for recipe in RECIPE_RE.findall(line):
            checked += 1
            defined = False
            for jl in justfile_lines or []:
                if re.match(r"^%s(\s[^:]+)?:(\s|$)" % recipe, jl):
                    defined = True
                    break
            if not defined:
                violations.append(
                    '%s:%d  recipe "just %s" not found in justfile' % (doc_name, line_no, recipe)
                )

        # ---- 行内反引号里的仓库路径与裸文件名 ----
        for token in BACKTICK_RE.findall(line):
            if "*" in token or "<" in token or ">" in token or "//" in token:
                continue
            if token.startswith("~"):
                continue
            if "/" in token:
                checked += 1
                exists = (
                    repo_path_exists(token, tracked)
                    if tracked
                    else os.path.exists(os.path.join(root, token))
                )
                if not exists:
                    violations.append(
                        '%s:%d  path "%s" does not exist in the repo' % (doc_name, line_no, token)
                    )
            elif token == "justfile" or token.endswith(BARE_SUFFIXES):
                checked += 1
                exists = (
                    repo_path_exists(token, tracked)
                    if tracked
                    else os.path.exists(os.path.join(root, token))
                )
                if not exists:
                    violations.append(
                        '%s:%d  file "%s" does not exist at the repo root'
                        % (doc_name, line_no, token)
                    )

        # ---- 测试目标 ----
        for target in CARGO_TEST_RE.findall(line):
            checked += 1
            if not scan_targets(root, target, tracked):
                violations.append(
                    "%s:%d  cargo test --test %s: no tests/%s.rs found"
                    % (doc_name, line_no, target, target)
                )

    # ---- 仓库各处对质量条目的指名引用必须存在 ----
    ref_violations, ref_files, ref_count = scan_quality_refs(
        root, rule_names(lines), tracked, loader
    )
    violations.extend(ref_violations)
    return violations, checked, ref_files, ref_count


def main(loader=None):
    if len(sys.argv) >= 2:
        arg = os.path.abspath(sys.argv[1])
    else:
        arg = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "AGENTS.md")
    if not os.path.isfile(arg):
        print("error: instruction file not found: %s" % arg, file=sys.stderr)
        print(
            "  the client only accepts AGENTS.md (no AGENT.md-style aliases);"
            " do not rename or move it.",
            file=sys.stderr,
        )
        return 1
    violations, checked, ref_files, ref_count = run(arg, loader)
    doc_name = os.path.basename(arg)
    if violations:
        for v in violations:
            print("  ✗ %s" % v, file=sys.stderr)
        print("", file=sys.stderr)
        print(
            "error: %s has %d references that do not match their sources (see above)."
            % (doc_name, len(violations)),
            file=sys.stderr,
        )
        print(
            "  - command or path really changed: fix the doc;"
            " do not let it describe a world that no longer exists;",
            file=sys.stderr,
        )
        print(
            "  - example or placeholder only: reword it, or wrap it in * < > ~ to skip validation;",
            file=sys.stderr,
        )
        print(
            "  - entry renamed: update every citation site"
            " (cite by name in the AGENTS.md named-entry form).",
            file=sys.stderr,
        )
        return 1
    print(
        "✓ %s references consistent (%d checked: just recipes / repo paths / test targets;"
        " plus %d named quality-entry citations across %d files)"
        % (doc_name, checked, ref_count, ref_files)
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
