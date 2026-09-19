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

  printf 'info!("ready");\n' >"$dir/crates/gloss-core/src/log.rs"

  # tests/stubs/ 桩副本夹具：让「桩副本一致」检查有可比对象
  mkdir -p "$dir/crates/gloss-core/tests/stubs" \
    "$dir/crates/gloss-app/tests/stubs" "$dir/crates/gloss-platform/tests/stubs"
  printf 'engine stub\n' >"$dir/crates/gloss-core/tests/stubs/engine.rs"
  cp "$dir/crates/gloss-core/tests/stubs/engine.rs" \
    "$dir/crates/gloss-app/tests/stubs/engine.rs"
  for c in gloss-core gloss-app; do
    printf '/// 内存版配置存储桩\npub struct MemoryConfigStore;\n/// 记录每次重绑定的热键桩\npub struct RecordingHotkeyBinder;\n' \
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
echo "-- 约束「版本单点维护」（应拒绝，退出码非 0）--"
assert_case "crate 硬写 version 字面量" 1 mut_crate_version_literal "缺少 version.workspace"
assert_case "crate 硬写 edition 字面量" 1 mut_edition_literal "缺少 edition.workspace"
assert_case "根 [workspace.package] 丢了版本字面量" 1 mut_root_missing_workspace_package "没有 version 字面量"

echo ""
echo "-- 约束「依赖只开需要的特性」（应拒绝，退出码非 0）--"
assert_case "第三方依赖未关默认特性" 1 mut_dep_without_default_features "pollster 未写 default-features = false"

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
