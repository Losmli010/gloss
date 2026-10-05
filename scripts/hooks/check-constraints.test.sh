#!/usr/bin/env bash
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CHECKER="$SCRIPT_DIR/check-constraints.sh"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

PASS=0
FAIL=0
CASE_NO=0
FIX=""


insert_after_section() {
  local file="$1" section="$2" text="$3"
  printf '%s\n' "$text" >"$WORK/repl.txt"
  awk -v sec="$section" '
    NR == FNR { repl = repl $0 "\n"; next }
    { print }
    $0 == sec { printf "%s", repl }
  ' "$WORK/repl.txt" "$file" >"$file.tmp" && mv "$file.tmp" "$file"
}

replace_line() {
  local file="$1" from="$2" to="$3"
  awk -v f="$from" -v t="$to" '{ if ($0 == f) print t; else print }' "$file" >"$file.tmp" &&
    mv "$file.tmp" "$file"
}

write_manifest() {
  local path="$1" name="$2" deps="${3-}"
  cat >"$path" <<EOF
[package]
name = "$name"
version.workspace = true
edition.workspace = true

[dependencies]

[lints]
workspace = true
EOF
  if [ -n "$deps" ]; then
    insert_after_section "$path" "[dependencies]" "$deps"
  fi
}

new_fixture() {
  local dir="$1" agents_body="${2-}"
  mkdir -p "$dir/crates/gloss-core/src" "$dir/crates/gloss-platform" "$dir/crates/gloss-app"

  cat >"$dir/Cargo.toml" <<'EOF'
[package]
name = "gloss"
version.workspace = true
edition.workspace = true

[dependencies]

[workspace]
members = ["crates/gloss-core", "crates/gloss-platform", "crates/gloss-app"]

[workspace.package]
version = "0.1.0"
edition = "2024"
EOF
  insert_after_section "$dir/Cargo.toml" "[dependencies]" 'gloss-core = { path = "crates/gloss-core" }'

  write_manifest "$dir/crates/gloss-core/Cargo.toml" "gloss-core" \
    'serde = { version = "1", default-features = false }
tracing = { version = "0.1", default-features = false }'
  write_manifest "$dir/crates/gloss-platform/Cargo.toml" "gloss-platform" \
    'gloss-core = { path = "../gloss-core" }
toml = { version = "0.8", default-features = false }'
  write_manifest "$dir/crates/gloss-app/Cargo.toml" "gloss-app" \
    'gloss-core = { path = "../gloss-core" }
gloss-platform = { path = "../gloss-platform" }'

  printf 'tracing::info!("ready");\n' >"$dir/crates/gloss-core/src/log.rs"
  # 第三方依赖的使用证据（依赖无死条目检查的基线）：
  # core 的 serde 经 derive 路径使用，platform 的 toml 经完整路径使用。
  printf '#[derive(serde::Deserialize)]\npub struct Config;\n' \
    >"$dir/crates/gloss-core/src/lib.rs"
  mkdir -p "$dir/crates/gloss-platform/src"
  printf 'let _v: String = toml::from_str("k = 1").unwrap();\n' \
    >"$dir/crates/gloss-platform/src/lib.rs"

  # tests/stubs/ 桩副本夹具：让「桩副本一致」检查有可比对象
  mkdir -p "$dir/crates/gloss-core/tests/stubs" \
    "$dir/crates/gloss-app/tests/stubs" "$dir/crates/gloss-platform/tests/stubs"
  printf 'engine stub\n' >"$dir/crates/gloss-core/tests/stubs/engine.rs"
  cp "$dir/crates/gloss-core/tests/stubs/engine.rs" \
    "$dir/crates/gloss-app/tests/stubs/engine.rs"
  for c in gloss-core gloss-app; do
    printf '/// 内存版配置存储桩\npub struct MemoryConfigStore;\n' \
      >"$dir/crates/$c/tests/stubs/ports.rs"
  done
  printf '/// 内存版配置存储桩\npub struct MemoryConfigStore;\n' \
    >"$dir/crates/gloss-platform/tests/stubs/ports.rs"

  if [ -n "$agents_body" ]; then
    printf '%s\n' "$agents_body" >"$dir/AGENTS.md"
  else
    printf 'gloss、gloss-core、gloss-platform、gloss-app 的依赖方向见下。\n' >"$dir/AGENTS.md"
  fi
}

assert_case() {
  local desc="$1" expected="$2" mutate="${3-}" needle="${4-}"

  CASE_NO=$((CASE_NO + 1))
  FIX="$WORK/case${CASE_NO}"
  new_fixture "$FIX"
  if [ -n "$mutate" ]; then "$mutate"; fi

  local out
  out="$(bash "$CHECKER" "$FIX" 2>&1)"
  local actual=$?
  local reason=""

  if [ "$actual" -ne "$expected" ]; then
    reason="期望退出码 ${expected}，实际 ${actual}"
  elif [ -n "$needle" ]; then
    case "$needle" in
      !*)
        if printf '%s' "$out" | grep -qF "${needle#!}"; then
          reason="输出不应包含「${needle#!}」"
        fi
        ;;
      *)
        if ! printf '%s' "$out" | grep -qF "$needle"; then
          reason="输出缺少「${needle}」"
        fi
        ;;
    esac
  fi

  if [ -z "$reason" ]; then
    echo "  ✓ $desc"
    PASS=$((PASS + 1))
  else
    echo "  ✗ $desc  (${reason})"
    echo "    输出: $(printf '%s' "${out}" | tail -n 3)"
    FAIL=$((FAIL + 1))
  fi
}


mut_app_engine_stub_drift() {
  printf '\n// drift\n' >>"$FIX/crates/gloss-app/tests/stubs/engine.rs"
}

mut_app_ports_stub_drift() {
  replace_line "$FIX/crates/gloss-app/tests/stubs/ports.rs" \
    'pub struct MemoryConfigStore;' \
    'pub struct MemoryConfigStore drift;'
}

mut_core_depends_on_platform() {
  insert_after_section "$FIX/crates/gloss-core/Cargo.toml" "[dependencies]" \
    'gloss-platform = { path = "../gloss-platform" }'
}
mut_core_depends_on_winit() {
  insert_after_section "$FIX/crates/gloss-core/Cargo.toml" "[dependencies]" \
    'winit = { version = "0.30", default-features = false }'
}
mut_core_depends_on_wgpu_core() {
  insert_after_section "$FIX/crates/gloss-core/Cargo.toml" "[dependencies]" \
    'wgpu-core = { version = "30", default-features = false }'
}
mut_platform_depends_on_app() {
  insert_after_section "$FIX/crates/gloss-platform/Cargo.toml" "[dependencies]" \
    'gloss-app = { path = "../gloss-app" }'
}
mut_app_depends_on_unregistered_crate() {
  mkdir -p "$FIX/crates/gloss-util"
  write_manifest "$FIX/crates/gloss-util/Cargo.toml" "gloss-util" ""
  insert_after_section "$FIX/crates/gloss-app/Cargo.toml" "[dependencies]" \
    'gloss-util = { path = "../gloss-util" }'
}
mut_crate_missing_from_agents_md() {
  printf 'gloss 与 gloss-core 的依赖方向见下。\n' >"$FIX/AGENTS.md"
}
mut_app_depends_on_tracing() {
  insert_after_section "$FIX/crates/gloss-app/Cargo.toml" "[dependencies]" \
    'tracing = { version = "0.1", default-features = false }'
}
mut_platform_depends_on_tracing_subscriber() {
  insert_after_section "$FIX/crates/gloss-platform/Cargo.toml" "[dependencies]" \
    'tracing-subscriber = { version = "0.3", default-features = false }'
}
mut_log_message_in_chinese() {
  printf 'info!("找不到配置文件");\n' >"$FIX/crates/gloss-core/src/log.rs"
}
mut_log_message_multiline_chinese() {
  printf 'info!(\n    gen = 1,\n    "读取失败"\n);\n' >"$FIX/crates/gloss-core/src/log.rs"
}
mut_bare_egui_context() {
  mkdir -p "$FIX/crates/gloss-app/src"
  printf 'let ctx = egui::Context::default();\n' >"$FIX/crates/gloss-app/src/bare.rs"
}
mut_context_with_spaces() {
  mkdir -p "$FIX/crates/gloss-app/src"
  printf 'let ctx = Context :: default ();\n' >"$FIX/crates/gloss-app/src/spaced.rs"
}
mut_fonts_written_outside() {
  mkdir -p "$FIX/crates/gloss-app/src"
  printf 'config.set_fonts(definitions);\n' >"$FIX/crates/gloss-app/src/sneaky_fonts.rs"
}
mut_theme_written_outside() {
  mkdir -p "$FIX/crates/gloss-app/src"
  printf 'ctx.set_theme(preference);\n' >"$FIX/crates/gloss-app/src/sneaky_theme.rs"
}
mut_context_allowed() {
  mkdir -p "$FIX/crates/gloss-app/src"
  printf 'let ctx = Context::default(); // constraints:allow-context 阴性对照\n' \
    >"$FIX/crates/gloss-app/src/negative_control.rs"
}
mut_installed_egui_context() {
  mkdir -p "$FIX/crates/gloss-app/src/ui"
  printf 'let ctx = Context::default();\n' >"$FIX/crates/gloss-app/src/ui/context.rs"
}
mut_ffi_declared_outside() {
  mkdir -p "$FIX/crates/gloss-platform/src"
  printf '#[link(name = "CoreGraphics", kind = "framework")]\nunsafe extern "C" {\n    fn CGWarpMouseCursorPosition(p: u32) -> i32;\n}\n' \
    >"$FIX/crates/gloss-platform/src/sneaky.rs"
}
mut_ffi_declared_in_owner() {
  mkdir -p "$FIX/crates/gloss-platform/src/ffi"
  printf '#[link(name = "CoreGraphics", kind = "framework")]\nunsafe extern "C" {\n    pub(crate) fn CGWarpMouseCursorPosition(p: u32) -> i32;\n}\n' \
    >"$FIX/crates/gloss-platform/src/ffi/cg.rs"
}
mut_ffi_callback_definition() {
  mkdir -p "$FIX/crates/gloss-platform/src"
  printf 'extern "C" fn fire(info: *mut c_void) {\n    let _ = info;\n}\n' \
    >"$FIX/crates/gloss-platform/src/callback.rs"
}
mut_crate_version_literal() {
  replace_line "$FIX/crates/gloss-app/Cargo.toml" "version.workspace = true" 'version = "0.1.0"'
}
mut_edition_literal() {
  replace_line "$FIX/crates/gloss-platform/Cargo.toml" "edition.workspace = true" 'edition = "2024"'
}
mut_root_missing_workspace_package() {
  replace_line "$FIX/Cargo.toml" 'version = "0.1.0"' "# 版本字面量被挪走了"
}
mut_dep_without_default_features() {
  insert_after_section "$FIX/crates/gloss-app/Cargo.toml" "[dependencies]" \
    'pollster = "1.0.1"'
}
mut_dep_default_features_on_next_line() {
  insert_after_section "$FIX/crates/gloss-app/Cargo.toml" "[dependencies]" \
    'tokio = { version = "1", features = [
    "sync",
], default-features = false }'
  # 跨行条目本身带使用证据：本用例只钉「features 检查不误报跨行」，
  # 不让依赖无死条目检查分叉结论。
  mkdir -p "$FIX/crates/gloss-app/src"
  printf 'let _rt = tokio::runtime::Builder::new_current_thread();\n' \
    >"$FIX/crates/gloss-app/src/tokio_probe.rs"
}
mut_platform_unused_dep() {
  insert_after_section "$FIX/crates/gloss-platform/Cargo.toml" "[dependencies]" \
    'serde_json = { version = "1", default-features = false }'
}
mut_platform_unused_dep_allowed() {
  insert_after_section "$FIX/crates/gloss-platform/Cargo.toml" "[dependencies]" \
    'serde_json = { version = "1", default-features = false } # deps:allow 自测阴性对照'
}
mut_platform_dash_key_unused() {
  insert_after_section "$FIX/crates/gloss-platform/Cargo.toml" "[dependencies]" \
    'crossbeam-channel = { version = "0.5", default-features = false }'
}
mut_platform_dash_key_used() {
  insert_after_section "$FIX/crates/gloss-platform/Cargo.toml" "[dependencies]" \
    'crossbeam-channel = { version = "0.5", default-features = false }'
  printf 'let (tx, _rx) = crossbeam_channel::unbounded();\n' \
    >"$FIX/crates/gloss-platform/src/queue.rs"
}
mut_platform_dev_dep_unused() {
  printf '\n[dev-dependencies]\nserde_json = { version = "1", default-features = false }\n' \
    >>"$FIX/crates/gloss-platform/Cargo.toml"
}
mut_root_dep_unused() {
  insert_after_section "$FIX/Cargo.toml" "[dependencies]" \
    'serde_json = { version = "1", default-features = false }'
}
mut_root_dep_used() {
  insert_after_section "$FIX/Cargo.toml" "[dependencies]" \
    'serde_json = { version = "1", default-features = false }'
  mkdir -p "$FIX/src"
  printf 'fn main() {\n    let _: u64 = serde_json::from_str("1").unwrap();\n}\n' \
    >"$FIX/src/main.rs"
}
mut_platform_use_bare_forms() {
  insert_after_section "$FIX/crates/gloss-platform/Cargo.toml" "[dependencies]" \
    'serde_json = { version = "1", default-features = false }'
  printf 'pub use serde_json;\nuse serde_json as json;\n' \
    >"$FIX/crates/gloss-platform/src/reexports.rs"
}
mut_platform_macro_form() {
  insert_after_section "$FIX/crates/gloss-platform/Cargo.toml" "[dependencies]" \
    'serde_json = { version = "1", default-features = false }'
  printf 'let _v = serde_json!(1);\n' >"$FIX/crates/gloss-platform/src/macro_use.rs"
}
mut_platform_tests_only_evidence() {
  insert_after_section "$FIX/crates/gloss-platform/Cargo.toml" "[dependencies]" \
    'serde_json = { version = "1", default-features = false }'
  printf 'use serde_json;\n' >"$FIX/crates/gloss-platform/tests/usage.rs"
}

echo "== 测试 check-constraints.sh =="
echo ""
echo "-- 合法用例（应通过，退出码 0）--"
assert_case "基线工作区全绿" 0 "" "检查 3 条第三方依赖声明"
assert_case "跨行内联表里写了 default-features（不应误报）" 0 mut_dep_default_features_on_next_line "检查 4 条第三方依赖声明"
assert_case "日志实参为英文（含中文注释）" 0 "" "0 处违规"
assert_case "core 依赖 tracing 合法（唯一日志出口）" 0 "" "约束检查通过"

echo ""
echo "-- 约束「依赖方向」（应拒绝，退出码非 0）--"
assert_case "core 依赖 platform（边表外）" 1 mut_core_depends_on_platform "不得依赖 gloss-platform"
assert_case "core 依赖 winit（红线）" 1 mut_core_depends_on_winit "红线禁入"
assert_case "core 依赖 wgpu-core（红线，靠前缀匹配）" 1 mut_core_depends_on_wgpu_core "红线禁入"
assert_case "platform 反向依赖 app" 1 mut_platform_depends_on_app "不得依赖 gloss-app"
assert_case "依赖未登记的 crate" 1 mut_app_depends_on_unregistered_crate "不得依赖 gloss-util"
assert_case "crate 名未写进 AGENTS.md" 1 mut_crate_missing_from_agents_md "未出现在"
assert_case "app 的 engine.rs 桩副本漂移" 1 mut_app_engine_stub_drift "桩副本漂移"
assert_case "app 的端口桩节漂移" 1 mut_app_ports_stub_drift "桩副本漂移"

echo ""
echo "-- 约束「日志统一出口」（应拒绝，退出码非 0）--"
assert_case "gloss-app 直接依赖 tracing" 1 mut_app_depends_on_tracing "只经 gloss_core::log"
assert_case "gloss-platform 直接依赖 tracing-subscriber" 1 mut_platform_depends_on_tracing_subscriber "只经 gloss_core::log"

echo ""
echo "-- 约束「日志一律英文」（应拒绝，退出码非 0）--"
assert_case "日志实参含中文" 1 mut_log_message_in_chinese "日志实参含非 ASCII"
assert_case "日志实参跨行且含中文" 1 mut_log_message_multiline_chinese "日志实参含非 ASCII"

echo ""
echo "-- 约束「egui 上下文统一装入点」（应拒绝，退出码非 0）--"
assert_case "统一装入点之外直接建 egui 上下文" 1 mut_bare_egui_context "egui 上下文统一装入点"
assert_case "写成 Context :: default () 也不漏网" 1 mut_context_with_spaces "egui 上下文统一装入点"
assert_case "统一装入点之外调用 set_fonts" 1 mut_fonts_written_outside "egui 上下文统一装入点"
assert_case "统一装入点之外调用 set_theme" 1 mut_theme_written_outside "egui 上下文统一装入点"
assert_case "注了 constraints:allow-context 的阴性对照放过" 0 mut_context_allowed "0 处绕过"
assert_case "统一装入点内建立上下文（不应误报）" 0 mut_installed_egui_context "0 处绕过"

echo ""
echo "-- 约束「FFI 声明集中」（应拒绝，退出码非 0）--"
assert_case "业务模块就地声明 C 接口" 1 mut_ffi_declared_outside "FFI 声明集中"
assert_case "ffi 模块内声明 C 接口（不应误报）" 0 mut_ffi_declared_in_owner "0 处就地声明"
assert_case "导出的 C 回调定义不算就地声明" 0 mut_ffi_callback_definition "0 处就地声明"

echo ""
echo "-- 约束「版本单点维护」（应拒绝，退出码非 0）--"
assert_case "crate 硬写 version 字面量" 1 mut_crate_version_literal "缺少 version.workspace"
assert_case "crate 硬写 edition 字面量" 1 mut_edition_literal "缺少 edition.workspace"
assert_case "根 [workspace.package] 丢了版本字面量" 1 mut_root_missing_workspace_package "没有 version 字面量"

echo ""
echo "-- 约束「依赖只开需要的特性」（应拒绝，退出码非 0）--"
assert_case "第三方依赖未关默认特性" 1 mut_dep_without_default_features "pollster 未写 default-features = false"

echo ""
echo "-- 约束「依赖无死条目」（应拒绝，退出码非 0）--"
assert_case "声明了无使用的第三方依赖" 1 mut_platform_unused_dep "依赖无死条目"
assert_case "deps:allow 放行所在条目" 0 mut_platform_unused_dep_allowed "依赖无死条目（扫"
assert_case "连字符键名无使用（转下划线后仍无证据）" 1 mut_platform_dash_key_unused "依赖无死条目"
assert_case "连字符键名按下划线使用不算死条目" 0 mut_platform_dash_key_used "deps:allow 0 条"
assert_case "dev-dependencies 死条目同样判罚" 1 mut_platform_dev_dep_unused "依赖无死条目"
assert_case "path 依赖无使用不判罚（归依赖方向管）" 0 "" "deps:allow 0 条"
assert_case "根 manifest 死条目同样判罚" 1 mut_root_dep_unused "依赖无死条目"
assert_case "根 manifest：src 里的证据放行" 0 mut_root_dep_used "deps:allow 0 条"
assert_case "pub use / use as 形态算证据" 0 mut_platform_use_bare_forms "deps:allow 0 条"
assert_case "宏调用形态算证据" 0 mut_platform_macro_form "deps:allow 0 条"
assert_case "tests/ 目录里的证据算数" 0 mut_platform_tests_only_evidence "deps:allow 0 条"

echo ""
echo "-- 残留任务标记扫描（git 夹具；上方非 git 夹具用例覆盖自动跳过路径）--"
MARK="$WORK/markcase"
new_fixture "$MARK"
git -C "$MARK" init -q
mkdir -p "$MARK/scripts/hooks"
cp "$CHECKER" "$MARK/scripts/hooks/"

assert_mark() {
  local desc="$1" expected="$2" rel="$3" content="$4" needle="${5-}"
  mkdir -p "$MARK/$(dirname "$rel")"
  printf '%s\n' "$content" >"$MARK/$rel"
  git -C "$MARK" add -A >/dev/null 2>&1
  local out actual reason=""
  out="$(bash "$MARK/scripts/hooks/check-constraints.sh" "$MARK" 2>&1)"
  actual=$?
  if [ "$actual" -ne "$expected" ]; then
    reason="期望退出码 ${expected}，实际 ${actual}"
  elif [ -n "$needle" ] && ! printf '%s' "$out" | grep -qF "$needle"; then
    reason="输出缺少「${needle}」"
  elif [ -z "$needle" ] && printf '%s' "$out" | grep -qF "残留任务标记："; then
    reason="不应出现残留标记命中"
  fi
  if [ -z "$reason" ]; then
    echo "  ✓ $desc"
    PASS=$((PASS + 1))
  else
    echo "  ✗ $desc  (${reason})"
    echo "    输出: $(printf '%s' "${out}" | tail -n 3)"
    FAIL=$((FAIL + 1))
  fi
  git -C "$MARK" rm -rq --cached "$rel" 2>/dev/null || true
  rm -f "$MARK/$rel"
}

assert_mark "干净 git 仓库通过" 0 "src/clean.rs" 'let x = compute(input);'
assert_mark "源文件含 TODO 命中" 1 "src/has_todo.rs" '// TODO: 待办' "残留任务标记："
assert_mark "FIXME 命中" 1 "src/has_fixme.rs" 'fixme_marker(); // FIXME' "残留任务标记："
assert_mark "AGENTS.md 含大写 TODO 豁免（规则本体）" 0 "AGENTS.md" \
  'gloss、gloss-core、gloss-platform、gloss-app 的依赖方向见下。代码不留 TODO 残留标记。'
assert_mark "小写 todo 与词边界（TODOs / MY_TODO）不误报" 0 "src/boundary.rs" \
  'let todo = "TODOS"; let _x = MY_TODO;'
# checker 自身豁免：脚本副本（内含 TODO|FIXME|HACK|TBD 字面量）已被 git add -A 跟踪，
# ---- eval 资产隔离 ----

mut_eval_dep() {
  insert_after_section "$FIX/crates/gloss-app/Cargo.toml" "[dependencies]" \
    'gloss-eval = { path = "../gloss-eval" }'
}

assert_case "生产 crate 依赖 gloss-eval 被拒" 1 mut_eval_dep "不得进生产依赖图"

mut_eval_include() {
  printf 'const X: &str = include_str!("../../gloss-eval/datasets/classify.jsonl");\n' \
    >"$FIX/crates/gloss-core/src/lib.rs"
}

assert_case "生产代码 include eval 资产被拒" 1 mut_eval_include "eval 资产嵌进了生产代码"

# 「干净 git 仓库通过」用例退出 0 即证明其未被自命中。

echo ""
echo "== 测试结果 =="
echo "  通过: $PASS"
echo "  失败: $FAIL"

if [ "$FAIL" -gt 0 ]; then
  echo ""
  echo "✗ 有 $FAIL 个测试失败"
  exit 1
fi

echo "✓ 全部 $PASS 个测试通过"
exit 0
