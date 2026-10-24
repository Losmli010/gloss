#!/usr/bin/env python3
"""仓库自动化门禁：把可由工具判定的规则逐条落成检查，覆盖依赖方向、评测资产隔离、
桩副本一致、日志出口与语言、界面库（egui）上下文装入点、外部函数接口（FFI）声明集中、
版本单点、依赖特性、死条目、文件行数与残留任务标记。
本地约束配方（提交前门禁的一部分）与持续集成的同名检查作业共用此脚本，
保证本地与远端判定一致。

覆盖：
  依赖方向            —— 各包（crate）只能依赖允许的边；核心库（gloss-core）不得出现平台与渲染栈，
                         红线是窗口库（winit）、图形后端（wgpu）与平台接口（API）。
  评测资产隔离         —— 评测包（gloss-eval）不被任何清单依赖；评测资产不被生产代码的
                         内嵌宏引用。
  桩副本一致          —— 各包（crate）测试桩目录的同名桩逐字一致，防跨包漂移。
  日志统一出口        —— 除核心库（gloss-core）外不得直接依赖日志三件套（tracing）。
  日志一律英文        —— 日志宏实参里不得出现非英文字节（ASCII）。
  界面库（egui）上下文装入点 —— 除统一装入点外不得直接建立渲染上下文。
  外部函数接口（FFI）声明集中 —— 跨语言声明只允许出现在平台适配层的专用模块。
  版本单点维护        —— 子包（crate）的版本与语言版本必须继承工作区，
                         字面量只允许出现在根清单的工作区段。
  依赖只开需要的特性  —— 每个第三方依赖声明必须关闭默认特性。
  依赖无死条目        —— 声明的第三方依赖必须在源码、测试与基准里有使用证据。
  文件行数上限        —— 单个生产源文件不超过五百行；
                         文件头部的放行标记注明缘由后豁免。
  残留任务标记        —— 大写任务标记词全字匹配，命中即失败。

用法：以解释器运行本脚本，首参为仓库根，缺省为本脚本的上一级目录。
"""

import os
import re
import subprocess
import sys

LINE_CAP = 500

# 约束“依赖方向”的允许边表。新增包（crate）必须在这里登记（并与项目文档同步），
# 漏登记即失败。
ALLOWED_OWN_DEPS = {
    "gloss": "gloss-core gloss-platform gloss-app",
    "gloss-app": "gloss-core gloss-platform",
    "gloss-platform": "gloss-core",
    "gloss-core": "",
    "gloss-eval": "gloss-core",
}

# 核心库（gloss-core）的红线：平台、渲染与系统适配栈。
CORE_FORBIDDEN_EXACT = {
    "winit",
    "wgpu",
    "egui",
    "epaint",
    "objc2",
    "windows",
    "pollster",
    "rdev",
    "global-hotkey",
    "arboard",
    "font-kit",
    "directories",
    "device_query",
    "enigo",
}
CORE_FORBIDDEN_PREFIX = (
    "winit-",
    "wgpu-",
    "egui-",
    "epaint-",
    "objc2-",
    "windows-",
    "core-graphics",
    "core-text",
    "core-foundation",
    "security-framework",
    "raw-window-handle",
)

# 残留任务标记扫描的豁免面：项目文档是规则本体；本脚本与其单测的夹具含字面量
SELF_FILES = (
    "AGENTS.md",
    "scripts/hooks/check_constraints.py",
    "scripts/hooks/check_constraints_test.py",
)


def core_forbidden(key):
    return key in CORE_FORBIDDEN_EXACT or key.startswith(CORE_FORBIDDEN_PREFIX)


def is_path_dep(text):
    return "path =" in text or "path=" in text


# ---- 清单解析 ----


def manifests(root):
    out = []
    root_manifest = os.path.join(root, "Cargo.toml")
    if os.path.isfile(root_manifest):
        out.append(root_manifest)
    crates = os.path.join(root, "crates")
    if os.path.isdir(crates):
        for name in sorted(os.listdir(crates)):
            m = os.path.join(crates, name, "Cargo.toml")
            if os.path.isfile(m):
                out.append(m)
    return out


def pkg_name(manifest_path):
    """取 [package] 里的 crate 名。"""
    in_section = False
    with open(manifest_path, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            stripped = line.strip()
            if stripped.startswith("["):
                in_section = stripped == "[package]"
                continue
            if in_section and re.match(r"name\s*=", stripped):
                return stripped.split("=", 1)[1].strip().strip("\"'")
    return ""


def section_body(manifest_path, want):
    """取某个 section 的正文（到下一个 section 头为止）。"""
    out = []
    in_section = False
    with open(manifest_path, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            if line.startswith("["):
                in_section = line.rstrip() == want
                continue
            if in_section:
                out.append(line.rstrip("\n"))
    return out


def dep_entries(manifest_path):
    """抽取依赖条目，输出 (section, key, 行号, 条目全文（跨行拼接）)。

    依赖表含 [dependencies] / [dev-dependencies] / [build-dependencies]，以及
    [target.'cfg(...)'.dev-dependencies] 这类门控形式（均以 dependencies] 结尾）。"""
    entries = []
    sec = None
    in_deps = False
    pending = False
    key = start = text = None
    with open(manifest_path, encoding="utf-8", errors="replace") as fh:
        for lineno, line in enumerate(fh, 1):
            line = line.rstrip("\n")
            if line.startswith("["):
                sec = line
                in_deps = sec.endswith("dependencies]")
                continue
            if not in_deps:
                continue
            if re.match(r"[A-Za-z0-9_-]+\s*=", line):
                if pending:
                    entries.append((sec, key, start, text))
                    pending = False
                key = line.split("=", 1)[0].strip()
                start = lineno
                text = line
                depth = line.count("{") - line.count("}")
                if depth <= 0:
                    entries.append((sec, key, start, text))
                else:
                    pending = True
                continue
            if pending:
                text = text + " " + line
                depth += line.count("{") - line.count("}")
                if depth <= 0:
                    pending = False
                    entries.append((sec, key, start, text))
    if pending:
        entries.append((sec, key, start, text))
    return entries


def walk_rs(*bases):
    """列各 base 下的 .rs 文件；base 不存在则跳过。任意层级剪枝 target / .git：
    cargo 产物只应出现在仓库根，但误提交进源码树深处的同名目录同样不该进
    扫描面（bash 版只剪根级，这里是审慎的超集）。"""
    for base in bases:
        if not os.path.isdir(base):
            continue
        for dirpath, dirnames, filenames in os.walk(base):
            dirnames[:] = sorted(d for d in dirnames if d not in ("target", ".git"))
            for name in sorted(filenames):
                if name.endswith(".rs"):
                    yield os.path.join(dirpath, name)


def read_text(path):
    with open(path, encoding="utf-8", errors="replace") as fh:
        return fh.read()


def stub_section(path, pat):
    """端口桩按节比对：节以“列 0 的 ///”切分（桩的条目文档都在列 0，
    结构体字段文档有缩进，不会误切）。文件缺失按空节处理（对齐 cmp 空对空）。"""
    if not os.path.isfile(path):
        return ""
    out = []
    found = False
    for line in read_text(path).split("\n"):
        if found:
            if line.startswith("///"):
                break
            out.append(line)
        elif line.startswith("/// %s" % pat):
            found = True
            out.append(line)
    while out and not out[-1].strip():
        out.pop()
    return "\n".join(out)


def run(root):
    """逐条执行约束，返回 (oks, fails)。"""
    manifest_list = manifests(root)
    dep_dump = []  # 条目依次是属主、清单相对路径、节、键、行号与全文
    for m in manifest_list:
        name = pkg_name(m)
        if not name:
            name = os.path.basename(os.path.dirname(m))
        for sec, key, start, text in dep_entries(m):
            dep_dump.append((name, os.path.relpath(m, root), sec, key, start, text))
    return _checks(root, manifest_list, dep_dump)


def _checks(root, manifest_list, dep_dump):
    oks = []
    fails = []

    def fail(msg):
        # 违规随执行流即时输出到标准错误，列表供调用方返回
        fails.append(msg)
        print("  ✗ %s" % msg, file=sys.stderr)

    def ok(msg):
        oks.append(msg)

    def rel(path):
        # 统一归一为斜杠风格，跨平台下与源码里的常量路径可比
        return os.path.relpath(path, root).replace(os.sep, "/")

    agents_md = os.path.join(root, "AGENTS.md")

    print("== constraint checks (repo root: %s) ==" % root)

    # ---- 约束“依赖方向” ----
    edges = 0
    for owner, mrel, _sec, key, start, text in dep_dump:
        if is_path_dep(text):
            edges += 1
            allowed = ALLOWED_OWN_DEPS.get(owner, "")
            if key not in allowed.split():
                fail(
                    "constraint violation (dependency direction): %s must not depend on"
                    " %s (%s:%d); allowed edges: %s"
                    % (os.path.basename(owner), key, mrel, start, allowed or "none")
                )
        elif owner == "gloss-core" and core_forbidden(key):
            fail(
                "constraint violation (dependency direction): gloss-core has a"
                " platform/render stack dependency %s (%s:%d) — red-line, forbidden"
                % (key, mrel, start)
            )
    # 包名（crate）必须在本文件里出现过：约束表与依赖边是两处维护，靠这条挡住单边改动
    if os.path.isfile(agents_md):
        agents_text = read_text(agents_md)
        for m in manifest_list:
            name = pkg_name(m)
            if name and name not in agents_text:
                fail(
                    "constraint violation (dependency direction): crate %s missing from"
                    " the dependency direction notes in %s" % (name, rel(agents_md))
                )
    ok("ok dependency direction (%d in-repo dependency edges + crate names registered)" % edges)

    # ---- 约束“评测资产隔离” ----
    # 评测包（gloss-eval）是评测工具链，依赖方向断言已保证它只消费、不被依赖；这里再钉
    # 两条硬边界，防评测资产从旁路渗进生产：
    #   一、评测包（gloss-eval）之外的清单不得出现对它的依赖（即使边表将来被
    #      误放宽，这条仍然挡住）；
    #   二、评测包（gloss-eval）之外的源码不得用内嵌宏引用评测资产。
    eval_dep_violations = 0
    for owner, mrel, _sec, key, start, _text in dep_dump:
        if key == "gloss-eval" and owner != "gloss-eval":
            fail(
                "constraint violation (eval asset isolation): %s depends on gloss-eval"
                " (%s:%d) — the eval toolchain must stay out of the production"
                " dependency graph" % (owner, mrel, start)
            )
            eval_dep_violations += 1
    eval_asset_violations = 0
    for f in walk_rs(
        os.path.join(root, "crates"),
        os.path.join(root, "src"),
        os.path.join(root, "tests"),
        os.path.join(root, "benches"),
    ):
        if rel(f).startswith("crates/gloss-eval/"):
            continue
        hits = [
            line
            for line in read_text(f).split("\n")
            if re.search(r"include_str!\(.*gloss-eval|include_bytes!\(.*gloss-eval", line)
        ]
        if not hits:
            continue
        eval_asset_violations += 1
        fail(
            "constraint violation (eval asset isolation): %s embeds a gloss-eval asset"
            " into production code (%s)" % (rel(f), hits[0][:60])
        )
    if eval_dep_violations == 0 and eval_asset_violations == 0:
        ok("ok eval asset isolation (0 dependency refs + 0 include refs)")

    # ---- 测试桩副本一致性：跨包（crate）复用的同名桩逐字一致 ----
    # 桩按包（crate）自持（各包的测试桩目录），一份漂移会让两个包的测试在
    # 语义不同的假实现上各自通过。引擎桩全文比对；端口桩按节比对。
    core_stubs = os.path.join(root, "crates", "gloss-core", "tests", "stubs")
    if os.path.isfile(os.path.join(core_stubs, "engine.rs")):
        drifts = 0

        def assert_same(desc, text_a, text_b):
            nonlocal drifts
            if text_a != text_b:
                fail(
                    "stub copy drift: %s — same-name stubs must be identical;"
                    " sync across crates when changing injected semantics" % desc
                )
                drifts += 1

        def text_of(path):
            return read_text(path) if os.path.isfile(path) else ""

        assert_same(
            "engine.rs (core <-> app)",
            text_of(os.path.join(core_stubs, "engine.rs")),
            text_of(os.path.join(root, "crates", "gloss-app", "tests", "stubs", "engine.rs")),
        )
        for crate in ("gloss-app", "gloss-platform"):
            assert_same(
                "memory config store stub (core <-> %s)" % crate,
                stub_section(os.path.join(core_stubs, "ports.rs"), "内存版配置存储桩"),
                stub_section(
                    os.path.join(root, "crates", crate, "tests", "stubs", "ports.rs"),
                    "内存版配置存储桩",
                ),
            )
        if drifts == 0:
            ok("ok stub copies identical (engine.rs full text + shared port stubs per section)")
    else:
        ok("ok stub copies identical (no tests/stubs/, skipped)")

    # ---- 约束“日志统一出口” ----
    for owner, mrel, _sec, key, start, _text in dep_dump:
        if key in ("tracing", "tracing-appender", "tracing-subscriber") and owner != "gloss-core":
            fail(
                "constraint violation (unified log exit): %s directly depends on %s"
                " (%s:%d) — use gloss_core::log only" % (os.path.basename(owner), key, mrel, start)
            )
    log_owners = len({owner for owner, _, _, _, _, _ in dep_dump})
    ok("ok unified log exit (log dependency ownership across %d crates)" % log_owners)

    # ---- 约束“日志一律英文” ----
    # 把源文件折成一行再抓日志宏的实参（到第一个右括号为止），实参里出现非英文字节即违规；行内反引号与注释里的中文不受影响（不在宏实参里）。
    rs_files = 0
    log_violations = 0
    log_macro_re = re.compile(
        r"(info|warn|error|debug|trace|event|info_span|warn_span|error_span"
        r"|debug_span|trace_span)!\(([^)]*)\)"
    )
    for f in walk_rs(root):
        rs_files += 1
        flat = read_text(f).replace("\n", " ")
        hits = [m.group(0) for m in log_macro_re.finditer(flat) if re.search(r"[^ -~]", m.group(0))]
        if hits:
            log_violations += 1
            fail(
                "constraint violation (log messages English-only): %s has a log macro"
                " argument with non-ASCII — %s" % (rel(f), hits[0][:60])
            )
    ok(
        "ok log messages English-only (%d .rs files scanned, %d violations)"
        % (rs_files, log_violations)
    )

    # ---- 约束“界面库（egui）上下文统一装入点” ----
    # 浮层与设置窗各持一个独立的上下文，字体与主题都要逐个上下文装上；
    # 这两件事只允许经统一装入点发生——别处直接建上下文、或直接设置字体与
    # 主题，都会得到一个少装了东西的上下文。
    # 匹配前去掉空白，带空格的写法同样命中；确需绕过的行注明放行标记并写明
    # 缘由（同密钥扫描的豁免惯例）。
    context_owner = "crates/gloss-app/src/ui/context.rs"
    ctx_re = re.compile(r"(?:^|[^A-Za-z0-9_])Context::default\(\)|\.set_fonts\(|\.set_theme\(")
    ctx_files = 0
    ctx_violations = 0
    for f in walk_rs(root):
        ctx_files += 1
        if rel(f) == context_owner:
            continue
        hits = [
            line
            for line in read_text(f).split("\n")
            if "constraints:allow-context" not in line
            and ctx_re.search(line.replace(" ", "").replace("\t", ""))
        ]
        if not hits:
            continue
        ctx_violations += 1
        fail(
            "constraint violation (egui context unified load point): %s bypasses the"
            " unified load point — go through new_context / reapply in ui/context.rs"
            " (%s)" % (rel(f), hits[0][:40])
        )
    ok(
        "ok egui context unified load point (%d .rs files scanned, %d bypasses)"
        % (ctx_files, ctx_violations)
    )

    # ---- 约束“外部函数接口（FFI）声明集中” ----
    # 平台跨语言声明只允许出现在平台适配层的专用模块（签名与不变量依据一处审计）；
    # 业务模块从那里引用，就地再声明一份会与真签名漂移。语言（C）声明只能写在跨语言
    # 块里，故按块匹配（匹配前去掉空白）；导出函数形式的回调定义不在其列：
    # 那是我们导出的函数，随各自的业务模块走。
    ffi_owner_dir = "crates/gloss-platform/src/ffi/"
    ffi_files = 0
    ffi_violations = 0
    for f in walk_rs(root):
        ffi_files += 1
        if rel(f).startswith(ffi_owner_dir):
            continue
        hits = [
            line
            for line in read_text(f).split("\n")
            if 'extern"C"{' in line.replace(" ", "").replace("\t", "")
        ]
        if not hits:
            continue
        ffi_violations += 1
        fail(
            "constraint violation (FFI declarations centralized): %s declares a C"
            " interface inline — move it into the ffi module of gloss-platform (%s)"
            % (rel(f), hits[0][:40])
        )
    ok(
        "ok FFI declarations centralized (%d .rs files scanned, %d inline declarations)"
        % (ffi_files, ffi_violations)
    )

    # ---- 约束“版本单点维护” ----
    root_manifest = os.path.join(root, "Cargo.toml")
    ws_body = (
        section_body(root_manifest, "[workspace.package]") if os.path.isfile(root_manifest) else []
    )
    for field in ("version", "edition"):
        if not any(re.match(r"^%s\s*=\s*\"" % field, line) for line in ws_body):
            fail(
                "constraint violation (single version source): %s [workspace.package]"
                " has no %s literal (the single source lives here)" % (rel(root_manifest), field)
            )
    for m in manifest_list:
        pkg = section_body(m, "[package]")
        for field in ("version", "edition"):
            if not any(re.match(r"^%s\.workspace\s*=\s*true" % field, line) for line in pkg):
                fail(
                    "constraint violation (single version source): %s [package] misses"
                    " %s.workspace = true (do not hardcode the literal)" % (rel(m), field)
                )
    ok("ok single version source (root [workspace.package] + per-crate *.workspace = true)")

    # ---- 约束“依赖只开需要的特性” ----
    deps_checked = 0
    for _owner, mrel, _sec, key, start, text in dep_dump:
        # 本仓库路径依赖（不带版本）与工作区继承项不适用这条
        if is_path_dep(text) and "version" not in text:
            continue
        if "workspace = true" in text:
            continue
        deps_checked += 1
        if "default-features = false" not in text:
            fail(
                "constraint violation (default features off): %s:%d %s lacks"
                " default-features = false" % (mrel, start, key)
            )
    ok("ok default features off (%d third-party dependency declarations checked)" % deps_checked)

    # ---- 约束“依赖无死条目” ----
    # 声明的每个第三方依赖（非本仓路径、非工作区继承）必须能在所属包（crate）的
    # 源码面（源码、测试、基准目录）找到使用证据：路径引用（含属性宏与派生宏
    # 内的路径）、宏调用，或导入语句（重导出与别名导入同算）。依赖键连字符按
    # 本语言（Rust）惯例转下划线后匹配；证据按文本匹配、不区分注释与代码（宽松判定防
    # 误报优先）。条目行尾的放行标记豁免该条（同密钥扫描惯例，须注明缘由）。
    # 本仓路径依赖由“依赖方向”管辖，不在此判罚；本仓无构建脚本，若未来引入
    # 需把其纳入扫描面。
    dead_count = 0
    dead_scanned = 0
    dead_allowed = 0
    for owner, mrel, _sec, key, start, text in dep_dump:
        if is_path_dep(text) or "workspace = true" in text:
            continue
        dead_scanned += 1
        if "deps:allow" in text:
            dead_allowed += 1
            continue
        crate_dir = root if mrel == "Cargo.toml" else os.path.join(root, os.path.dirname(mrel))
        import_name = key.replace("-", "_")
        evidence = re.compile(
            r"(?:^|[^A-Za-z0-9_])%s(?:::|!)|^\s*(?:pub\s+)?use\s+%s\s*(?:;|as)"
            % (import_name, import_name)
        )
        found = False
        for base in ("src", "tests", "benches"):
            base_dir = os.path.join(crate_dir, base)
            if not os.path.isdir(base_dir):
                continue
            for dirpath, _, filenames in os.walk(base_dir):
                for name in filenames:
                    path = os.path.join(dirpath, name)
                    for line in read_text(path).split("\n"):
                        if evidence.search(line):
                            found = True
                            break
                    if found:
                        break
                if found:
                    break
            if found:
                break
        if found:
            continue
        dead_count += 1
        fail(
            "constraint violation (no dead dependencies): %s declares %s (%s:%d) with no"
            " usage evidence in src/tests/benches — use it, drop it, or add a deps:allow"
            " marker with a reason at the end of the entry line"
            % (os.path.basename(owner), key, mrel, start)
        )
    if dead_count == 0:
        ok(
            "ok no dead dependencies (%d third-party declarations scanned, %d deps:allow)"
            % (dead_scanned, dead_allowed)
        )

    # ---- 约束“文件行数上限” ----
    # 单个生产源文件不超过五百行，超限即失败；扫描面是根源码目录与各包（crate）的
    # 源码目录，测试目标与基准不在其列。生产行数按首个列零的测试开关属性行
    # 截断（测试模块按仓库惯例置于文件尾），故文件尾的内联测试模块不计入。
    # 存量确需缓冲的，文件前五行写放行标记并注明缘由（同密钥扫描惯例）——
    # 豁免是债，新代码一律按职责拆分，不加豁免。
    big_files = 0
    big_allowed = 0
    src_bases = [os.path.join(root, "src")]
    crates_dir = os.path.join(root, "crates")
    if os.path.isdir(crates_dir):
        for name in sorted(os.listdir(crates_dir)):
            src_bases.append(os.path.join(crates_dir, name, "src"))
    for f in walk_rs(*src_bases):
        lines = read_text(f).split("\n")
        production = 0
        for line in lines:
            if line.startswith("#[cfg(test)]"):
                break
            production += 1
        if production <= LINE_CAP:
            continue
        if any("lines:allow" in line for line in lines[:5]):
            big_allowed += 1
            continue
        big_files += 1
        fail(
            "constraint violation (line cap): %s has %d production lines, over the %d-line"
            " cap — split by responsibility; if a buffer is truly needed, add lines:allow"
            " with a reason in the first 5 lines" % (rel(f), production, LINE_CAP)
        )
    if big_files == 0:
        ok(
            "ok line cap (production code <=%d lines; %d lines:allow exemptions)"
            % (LINE_CAP, big_allowed)
        )

    # ---- 残留任务标记扫描 ----
    # 大写任务标记词全字匹配，扫全部仓库跟踪文件。
    # 项目文档（规则本体）与本脚本及其自测（夹具含字面量）豁免，避免自命中。
    # 非版本库环境（门禁自测夹具）无跟踪面可扫，整段跳过。
    if (
        subprocess.run(
            ["git", "-C", root, "rev-parse", "--is-inside-work-tree"], capture_output=True
        ).returncode
        == 0
    ):
        # 正则按英文词界（ASCII）的字节语义对齐命令行全字匹配：汉字不算词字符，
        # 任务标记紧贴中文的形态同样命中
        marker_re = re.compile(r"\b(?:TODO|FIXME|HACK|TBD)\b", re.ASCII)
        out = subprocess.run(
            ["git", "-C", root, "ls-files", "-z"], capture_output=True, check=True
        ).stdout
        for raw in out.split(b"\0"):
            if not raw:
                continue
            f = raw.decode("utf-8", "replace")
            if f in SELF_FILES:
                continue
            path = os.path.join(root, f)
            try:
                with open(path, "rb") as fh:
                    data = fh.read()
            except OSError:
                continue
            if b"\0" in data:
                continue
            hits = [
                "%d:%s" % (no, line)
                for no, line in enumerate(data.decode("utf-8", "replace").split("\n"), 1)
                if marker_re.search(line)
            ]
            if hits:
                fail("leftover task marker: %s — %s" % (f, hits[0][:60]))
        ok("ok no leftover task markers (whole-word scan of git-tracked files)")

    return oks, fails


def main():
    root = (
        os.path.abspath(sys.argv[1])
        if len(sys.argv) > 1
        else os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    )
    oks, fails = run(root)
    # 违规已在检查过程内即时输出到标准错误，这里只补汇总与结论
    for line in oks:
        print("  ✓ %s" % line)
    if fails:
        print("", file=sys.stderr)
        print(
            "error: %d checks failed the automated gates (see above)." % len(fails), file=sys.stderr
        )
        print("  - rule stays: fix the code;", file=sys.stderr)
        print(
            "  - rule really changed: update AGENTS.md and related configs first, then"
            " this script; never just one side.",
            file=sys.stderr,
        )
        return 1
    print("✓ constraint checks passed (%d checks)" % len(oks))
    return 0


if __name__ == "__main__":
    sys.exit(main())
