#!/usr/bin/env python3
# 仓库自动化门禁：把可由工具判定的规则逐条落成检查
# （依赖方向 / eval 资产隔离 / 桩副本一致 / 日志出口与语言 / egui 上下文装入点 /
#  FFI 声明集中 / 版本单点 / 依赖特性 / 死条目 / 文件行数 / 残留任务标记）。
# 本地 `just constraints`（pre-commit 的一部分）与 CI 的 Constraints check job 共用此脚本，
# 保证本地与 CI 判定一致。
#
# 覆盖：
#   依赖方向            —— 各 crate 只能依赖允许的边；gloss-core 不得出现平台/渲染栈
#                          （红线：winit / wgpu / 平台 API）。
#   eval 资产隔离       —— gloss-eval 不被任何 manifest 依赖；eval 资产不被生产
#                          代码 include_str!/include_bytes! 引用。
#   桩副本一致          —— 各 crate tests/stubs/ 的同名桩逐字一致，防跨 crate 漂移。
#   日志统一出口        —— 除 gloss-core 外不得直接依赖 tracing 三件套。
#   日志一律英文        —— 日志宏实参里不得出现非 ASCII 字节。
#   egui 上下文装入点   —— 除 ui/context.rs 外不得直接建立 egui::Context。
#   FFI 声明集中        —— extern "C" 声明只允许出现在 gloss-platform 的 ffi 模块。
#   版本单点维护        —— 子 crate 的 version / edition 必须 *.workspace = true，
#                          字面量只允许出现在根 [workspace.package]。
#   依赖只开需要的特性  —— 每个第三方依赖声明必须带 default-features = false。
#   依赖无死条目        —— 声明的第三方依赖必须在 src/tests/benches 有使用证据。
#   文件行数上限        —— 单个生产代码 .rs 文件（根 src/ 与 crates/*/src/）≤500 行；
#                          文件头部 lines:allow 注明缘由放行。
#   残留任务标记        —— TODO / FIXME / HACK / TBD 大写词全字匹配，命中即失败。
#
# 用法：scripts/hooks/check_constraints.py [仓库根]   # 缺省为本脚本的上一级目录

import os
import re
import subprocess
import sys

LINE_CAP = 500

# 约束「依赖方向」的允许边表。新增 crate 必须在这里登记（并与 AGENTS.md 同步），
# 漏登记即失败。
ALLOWED_OWN_DEPS = {
    "gloss": "gloss-core gloss-platform gloss-app",
    "gloss-app": "gloss-core gloss-platform",
    "gloss-platform": "gloss-core",
    "gloss-core": "",
    "gloss-eval": "gloss-core",
}

# gloss-core 的红线：平台 / 渲染 / 系统适配栈。
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

# 残留任务标记扫描的豁免面：AGENTS.md 是规则本体；本脚本与其单测的夹具含字面量
SELF_FILES = (
    "AGENTS.md",
    "scripts/hooks/check_constraints.py",
    "scripts/hooks/check_constraints_test.py",
)


def core_forbidden(key):
    return key in CORE_FORBIDDEN_EXACT or key.startswith(CORE_FORBIDDEN_PREFIX)


def is_path_dep(text):
    return "path =" in text or "path=" in text


# ---- manifest 解析 ----


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
            if in_section and line.split()[:1] == ["name"] and "=" in line:
                return line.split("=", 1)[1].strip().strip("\"'")
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
    """列各 base 下的 .rs 文件（跳过 target / .git）；base 不存在则跳过。"""
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
    """端口桩按节比对：节以「列 0 的 ///」切分（桩的条目文档都在列 0，
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
    dep_dump = []  # (owner, manifest 相对路径, section, key, 行号, 条目全文)
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
        fails.append(msg)

    def ok(msg):
        oks.append(msg)

    def rel(path):
        return os.path.relpath(path, root)

    agents_md = os.path.join(root, "AGENTS.md")

    print("== 约束检查（仓库根：%s）==" % root)

    # ---- 约束「依赖方向」 ----
    edges = 0
    for owner, mrel, _sec, key, start, text in dep_dump:
        if is_path_dep(text):
            edges += 1
            allowed = ALLOWED_OWN_DEPS.get(owner, "")
            if key not in allowed.split():
                fail(
                    "约束「依赖方向」：%s 不得依赖 %s（%s:%d）；允许的边：%s"
                    % (os.path.basename(owner), key, mrel, start, allowed or "无")
                )
        elif owner == "gloss-core" and core_forbidden(key):
            fail(
                "约束「依赖方向」：gloss-core 出现平台/渲染栈依赖 %s（%s:%d）——红线禁入"
                % (key, mrel, start)
            )
    # crate 名必须在本文件里出现过：约束表与依赖边是两处维护，靠这条挡住单边改动
    if os.path.isfile(agents_md):
        agents_text = read_text(agents_md)
        for m in manifest_list:
            name = pkg_name(m)
            if name and name not in agents_text:
                fail(
                    "约束「依赖方向」：crate %s 未出现在 %s 的依赖方向说明里"
                    % (name, rel(agents_md))
                )
    ok("依赖方向（%d 条本仓库依赖边 + 各 crate 的 crate 名登记）" % edges)

    # ---- 约束「eval 资产隔离」 ----
    # gloss-eval 是评测工具链，依赖方向断言已保证它只消费、不被依赖；这里再钉
    # 两条硬边界，防评测资产从旁路渗进生产：
    #   1. 任何 gloss-eval 之外的 manifest 不得出现对它的依赖（即使边表将来被
    #      误放宽，这条仍然挡住）；
    #   2. gloss-eval 之外的 .rs 不得 include_str!/include_bytes! 引用 eval
    #      资产（datasets/fixtures/prompts）。
    eval_dep_violations = 0
    for owner, mrel, _sec, key, _start, _text in dep_dump:
        if key == "gloss-eval" and owner != "gloss-eval":
            fail(
                "约束「eval 资产隔离」：%s 依赖了 gloss-eval（%s:%d）"
                "——评测工具链不得进生产依赖图" % (owner, mrel, start)
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
            "约束「eval 资产隔离」：%s 把 gloss-eval 资产嵌进了生产代码（%s）"
            % (rel(f), hits[0][:60])
        )
    if eval_dep_violations == 0 and eval_asset_violations == 0:
        ok("eval 资产隔离（依赖 0 处 + include 0 处）")

    # ---- 测试桩副本一致性（AGENTS.md「测试」节：跨 crate 复用的同名桩逐字一致）----
    # 桩按 crate 自持（tests/stubs/），一份漂移会让两个 crate 的测试在语义不同的
    # 假实现上各自通过。engine.rs 全文比对；端口桩按节比对。
    core_stubs = os.path.join(root, "crates", "gloss-core", "tests", "stubs")
    if os.path.isfile(os.path.join(core_stubs, "engine.rs")):
        drifts = 0

        def assert_same(desc, text_a, text_b):
            nonlocal drifts
            if text_a != text_b:
                fail("桩副本漂移：%s —— 同名桩必须逐字一致，改注入语义时跨 crate 同步" % desc)
                drifts += 1

        def text_of(path):
            return read_text(path) if os.path.isfile(path) else ""

        assert_same(
            "engine.rs（core ↔ app）",
            text_of(os.path.join(core_stubs, "engine.rs")),
            text_of(os.path.join(root, "crates", "gloss-app", "tests", "stubs", "engine.rs")),
        )
        for crate in ("gloss-app", "gloss-platform"):
            assert_same(
                "内存版配置存储桩（core ↔ %s）" % crate,
                stub_section(os.path.join(core_stubs, "ports.rs"), "内存版配置存储桩"),
                stub_section(
                    os.path.join(root, "crates", crate, "tests", "stubs", "ports.rs"),
                    "内存版配置存储桩",
                ),
            )
        if drifts == 0:
            ok("桩副本逐字一致（engine.rs 全文 + 共享端口桩逐节比对）")
    else:
        ok("桩副本逐字一致（无 tests/stubs/，跳过）")

    # ---- 约束「日志统一出口」 ----
    for owner, mrel, _sec, key, start, _text in dep_dump:
        if key in ("tracing", "tracing-appender", "tracing-subscriber") and owner != "gloss-core":
            fail(
                "约束「日志统一出口」：%s 直接依赖了 %s（%s:%d）——只经 gloss_core::log 使用"
                % (os.path.basename(owner), key, mrel, start)
            )
    log_owners = len({owner for owner, _, _, _, _, _ in dep_dump})
    ok("日志统一出口（%d 个 crate 的日志依赖归属）" % log_owners)

    # ---- 约束「日志一律英文」 ----
    # 把源文件折成一行再抓日志宏的实参（到第一个右括号为止），实参里出现非 ASCII
    # 字节即违规；行内反引号/注释里的中文不受影响（不在宏实参里）。
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
            fail("约束「日志一律英文」：%s 的日志实参含非 ASCII —— %s" % (rel(f), hits[0][:60]))
    ok("日志一律英文（扫 %d 个 .rs 文件，%d 处违规）" % (rs_files, log_violations))

    # ---- 约束「egui 上下文统一装入点」 ----
    # 浮层与设置窗各持一个独立的 egui::Context，字体与主题都要逐个上下文装上；
    # 这两件事只允许经 ui/context.rs（new_context / reapply）发生——别处直接建上下文、
    # 或直接调用 set_fonts / set_theme，都会得到一个少装了东西的上下文。
    # 匹配前去掉空白，`Context :: default ()` 这类写法同样命中；确需绕过的行
    # 注明 `constraints:allow-context` 并写明缘由（同 secrets:allow 惯例）。
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
            "约束「egui 上下文统一装入点」：%s 绕过统一装入点 —— 走 ui/context.rs 的 "
            "new_context / reapply（%s）" % (rel(f), hits[0][:40])
        )
    ok("egui 上下文统一装入点（扫 %d 个 .rs 文件，%d 处绕过）" % (ctx_files, ctx_violations))

    # ---- 约束「FFI 声明集中」 ----
    # 平台 FFI 的 extern 声明只允许出现在 gloss-platform 的 ffi 模块（签名与 SAFETY
    # 依据一处审计）；业务模块从那里引用，就地再声明一份会与真签名漂移。C 声明只能
    # 写在 extern 块里，故按块匹配（匹配前去掉空白）；`extern "C" fn name(...)` 形式
    # 的回调定义不在其列：那是我们导出的函数，随各自的业务模块走。
    ffi_owner_dir = "crates/gloss-platform/src/ffi/"
    ffi_files = 0
    ffi_violations = 0
    for f in walk_rs(root):
        ffi_files += 1
        if rel(f).replace(os.sep, "/").startswith(ffi_owner_dir):
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
            "约束「FFI 声明集中」：%s 就地声明了 C 接口 —— 收进 gloss-platform 的 ffi 模块（%s）"
            % (rel(f), hits[0][:40])
        )
    ok("FFI 声明集中（扫 %d 个 .rs 文件，%d 处就地声明）" % (ffi_files, ffi_violations))

    # ---- 约束「版本单点维护」 ----
    root_manifest = os.path.join(root, "Cargo.toml")
    ws_body = (
        section_body(root_manifest, "[workspace.package]") if os.path.isfile(root_manifest) else []
    )
    for field in ("version", "edition"):
        if not any(re.match(r"^%s\s*=\s*\"" % field, line) for line in ws_body):
            fail(
                "约束「版本单点维护」：%s 的 [workspace.package] 没有 %s 字面量"
                "（版本单点就在这里）" % (rel(root_manifest), field)
            )
    for m in manifest_list:
        pkg = section_body(m, "[package]")
        for field in ("version", "edition"):
            if not any(re.match(r"^%s\.workspace\s*=\s*true" % field, line) for line in pkg):
                fail(
                    "约束「版本单点维护」：%s 的 [package] 缺少 %s.workspace = true"
                    "（不要硬写字面量）" % (rel(m), field)
                )
    ok("版本单点维护（根 [workspace.package] + 各 crate 的 *.workspace = true）")

    # ---- 约束「依赖只开需要的特性」 ----
    deps_checked = 0
    for _owner, mrel, _sec, key, start, text in dep_dump:
        # 本仓库 path 依赖（不带 version）与 workspace 继承项不适用这条
        if is_path_dep(text) and "version" not in text:
            continue
        if "workspace = true" in text:
            continue
        deps_checked += 1
        if "default-features = false" not in text:
            fail(
                "约束「依赖只开需要的特性」：%s:%d 的 %s 未写 default-features = false"
                % (mrel, start, key)
            )
    ok("依赖只开需要的特性（检查 %d 条第三方依赖声明）" % deps_checked)

    # ---- 约束「依赖无死条目」 ----
    # 声明的每个第三方依赖（非本仓 path、非 workspace 继承）必须能在所属 crate 的
    # 源码面（src / tests / benches）找到使用证据：`名::` 路径（含属性宏与 derive
    # 内的路径，如 #[tokio::test]、#[derive(serde::Deserialize)]）、`名!` 宏调用，
    # 或 `use 名;`（`pub use`、`use 名 as 别名` 同算）。依赖键连字符按 Rust 惯例
    # 转下划线后匹配；证据按文本匹配、不区分注释与代码（宽松判定防误报优先）。
    # 条目行尾 `deps:allow` 放行该条（同 secrets:allow 惯例，须注明缘由）。
    # 本仓 path 依赖由「依赖方向」管辖，不在此判罚；本仓无 build.rs，若未来引入
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
            "约束「依赖无死条目」：%s 声明了 %s（%s:%d）但 src/tests/benches 无使用证据"
            " —— 用起来、删掉，或条目行尾 deps:allow 注明缘由"
            % (os.path.basename(owner), key, mrel, start)
        )
    if dead_count == 0:
        ok("依赖无死条目（扫 %d 条第三方声明，deps:allow %d 条）" % (dead_scanned, dead_allowed))

    # ---- 约束「文件行数上限」 ----
    # 单个生产代码 .rs 文件 ≤500 行，超限即失败；扫描面是根 src/ 与 crates/*/src/，
    # 测试目标（tests/、benches/）不在其列。生产行数按首个列 0 的 `#[cfg(test)]`
    # 截断（测试模块按仓库惯例置于文件尾），故文件尾的内联测试模块不计入。
    # 存量确需缓冲的，文件前 5 行写 `lines:allow` 并注明缘由放行（同 secrets:allow
    # 惯例）——豁免是债，新代码一律按职责拆分，不加豁免。
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
            "约束「文件行数上限」：%s 生产代码 %d 行，超 %d 行上限 —— 按职责拆分；"
            "确需缓冲在文件前 5 行写 lines:allow 注明缘由" % (rel(f), production, LINE_CAP)
        )
    if big_files == 0:
        ok("文件行数上限（生产代码 ≤%d 行；lines:allow 豁免 %d 个）" % (LINE_CAP, big_allowed))

    # ---- 残留任务标记扫描 ----
    # 大写词全字匹配 TODO / FIXME / HACK / TBD，扫全部 git 跟踪文件。
    # AGENTS.md（规则本体）与本脚本及其自测（夹具含字面量）豁免，避免自命中。
    # 非 git 仓库（门禁自测夹具）无跟踪面可扫，整段跳过。
    if (
        subprocess.run(
            ["git", "-C", root, "rev-parse", "--is-inside-work-tree"], capture_output=True
        ).returncode
        == 0
    ):
        marker_re = re.compile(r"\b(?:TODO|FIXME|HACK|TBD)\b")
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
                fail("残留任务标记：%s —— %s" % (f, hits[0][:60]))
        ok("无残留任务标记（git 跟踪文件全字扫描）")

    return oks, fails


def main():
    root = (
        os.path.abspath(sys.argv[1])
        if len(sys.argv) > 1
        else os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    )
    oks, fails = run(root)
    for line in oks:
        print("  ✓ %s" % line)
    if fails:
        for line in fails:
            print("  ✗ %s" % line, file=sys.stderr)
        print("", file=sys.stderr)
        print("错误：有 %d 处未通过自动化门禁（见上）。" % len(fails), file=sys.stderr)
        print("  - 规则本身不打算改：改代码；", file=sys.stderr)
        print(
            "  - 规则确实要改：先改 AGENTS.md 与相关配置，再改本脚本，别只改一处。", file=sys.stderr
        )
        return 1
    print("✓ 约束检查通过（%d 项）" % len(oks))
    return 0


if __name__ == "__main__":
    sys.exit(main())
