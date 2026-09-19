#!/usr/bin/env bash
# 「不可协商的约束」机械门禁：把 AGENTS.md 里已被工具判定得了的约束逐条落成检查。
# 本地 `just constraints`（pre-commit 的一部分）与 CI 的 quality job 共用此脚本，
# 保证本地与 CI 判定一致。
#
# 覆盖（按 AGENTS.md 约束的名字对应）：
#   依赖方向            —— 各 crate 只能依赖允许的边；gloss-core 不得出现平台/渲染栈
#                          （红线：winit / wgpu / 平台 API）。
#   日志统一出口        —— 除 gloss-core 外不得直接依赖 tracing 三件套。
#   日志一律英文        —— 日志宏实参里不得出现非 ASCII 字节。
#   版本单点维护        —— 子 crate 的 version / edition 必须 *.workspace = true，
#                          字面量只允许出现在根 [workspace.package]。
#   依赖只开需要的特性  —— 每个第三方依赖声明必须带 default-features = false。
#
# 用法：scripts/hooks/check-constraints.sh [仓库根]   # 缺省为本脚本的上一级目录
set -euo pipefail

SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${1:-$(cd "$SELF_DIR/../.." && pwd)}"
AGENTS_MD="$ROOT/AGENTS.md"

FAILED=0
CHECKS=0

fail() {
  echo "  ✗ $1" >&2
  FAILED=$((FAILED + 1))
}

ok() {
  CHECKS=$((CHECKS + 1))
  echo "  ✓ $1"
}

rel() { printf '%s' "${1#"$ROOT"/}"; }

# ---- manifest 解析助手 ----

# 工作区内的 crate manifest：根包 + crates/*/
manifests() {
  [ -f "$ROOT/Cargo.toml" ] && printf '%s\n' "$ROOT/Cargo.toml"
  for m in "$ROOT"/crates/*/Cargo.toml; do
    if [ -f "$m" ]; then printf '%s\n' "$m"; fi
  done
}

# 取 [package] 里的 crate 名
pkg_name() {
  awk '
    /^\[/ { insec = ($0 == "[package]") }
    insec && /^name[[:space:]]*=/ {
      sub(/^name[[:space:]]*=[[:space:]]*/, "")
      gsub(/["'"'"']/, "")
      print
      exit
    }
  ' "$1"
}

# 取某个 section 的正文
section_body() {
  awk -v want="$2" '/^\[/ { insec = ($0 == want) } insec { print }' "$1"
}

# 抽取依赖条目，输出 "section<TAB>key<TAB>行号<TAB>条目全文（跨行拼接）"。
# 依赖表含 [dependencies] / [dev-dependencies] / [build-dependencies]，以及
# [target.'cfg(...)'.dev-dependencies] 这类门控形式（均以 dependencies] 结尾）。
dep_entries() {
  awk '
    function braces(s,   i, c, d) {
      d = 0
      for (i = 1; i <= length(s); i++) {
        c = substr(s, i, 1)
        if (c == "{") d++
        else if (c == "}") d--
      }
      return d
    }
    function emit() { print sec "\t" key "\t" start "\t" text }
    /^\[/ {
      sec = $0
      in_deps = (sec ~ /dependencies\]$/)
      next
    }
    !in_deps { next }
    /^[A-Za-z0-9_-]+[[:space:]]*=/ {
      if (pending) { emit(); pending = 0 }
      key = $0
      sub(/[[:space:]]*=.*/, "", key)
      start = NR
      text = $0
      depth = braces($0)
      if (depth <= 0) { emit() } else { pending = 1 }
      next
    }
    pending {
      text = text " " $0
      depth += braces($0)
      if (depth <= 0) { pending = 0; emit() }
    }
  ' "$1"
}

# 条目的依赖键（本仓库内依赖一律用 path 指定）
is_path_dep() {
  case "$1" in
    *"path ="* | *"path="*) return 0 ;;
    *) return 1 ;;
  esac
}

# 约束「依赖方向」的允许边表。新增 crate 必须在这里登记（并与 AGENTS.md 同步），
# 漏登记即失败。
allowed_own_deps() {
  case "$1" in
    gloss) printf 'gloss-core gloss-platform gloss-app' ;;
    gloss-app) printf 'gloss-core gloss-platform' ;;
    gloss-platform) printf 'gloss-core' ;;
    gloss-core) printf '' ;;
    *) printf '' ;;
  esac
}

# gloss-core 的红线：平台 / 渲染 / 系统适配栈。
core_forbidden() {
  case "$1" in
    winit | winit-* | wgpu | wgpu-* | egui | egui-* | epaint | epaint-* | \
      objc2 | objc2-* | windows | windows-* | core-graphics* | core-text* | \
      core-foundation* | security-framework* | raw-window-handle* | pollster | \
      rdev | global-hotkey | arboard | font-kit | directories | device_query | enigo)
      return 0
      ;;
    *) return 1 ;;
  esac
}

# ---- 逐 manifest 收集依赖边 ----

MANIFEST_LIST="$(manifests)"
DEP_DUMP=""
for m in $MANIFEST_LIST; do
  name="$(pkg_name "$m")"
  [ -n "$name" ] || name="$(basename "$(dirname "$m")")"
  while IFS=$'\t' read -r sec key start text; do
    [ -n "$key" ] || continue
    DEP_DUMP="${DEP_DUMP}${name}|$(rel "$m")|${sec}|${key}|${start}|${text}"$'\n'
  done < <(dep_entries "$m")
done

echo "== 约束检查（仓库根：$(rel "$ROOT")）=="

# ---- 约束「依赖方向」 ----
edges=0
while IFS='|' read -r owner manifest sec key start text; do
  [ -n "$owner" ] || continue
  if is_path_dep "$text"; then
    edges=$((edges + 1))
    allowed=" $(allowed_own_deps "$owner") "
    case "$allowed" in
      *" $key "*) ;;
      *) fail "约束「依赖方向」：$(basename "$owner") 不得依赖 ${key}（$(rel "$manifest"):${start}）；允许的边：$(allowed_own_deps "$owner" | sed 's/^$/无/')" ;;
    esac
  elif [ "$owner" = "gloss-core" ] && core_forbidden "$key"; then
    fail "约束「依赖方向」：gloss-core 出现平台/渲染栈依赖 ${key}（$(rel "$manifest"):${start}）——红线禁入"
  fi
done <<<"$DEP_DUMP"

# crate 名必须在本文件里出现过：约束表与依赖边是两处维护，靠这条挡住单边改动
if [ -f "$AGENTS_MD" ]; then
  for m in $MANIFEST_LIST; do
    name="$(pkg_name "$m")"
    [ -n "$name" ] || continue
    if ! grep -qF "$name" "$AGENTS_MD"; then
      fail "约束「依赖方向」：crate ${name} 未出现在 $(rel "$AGENTS_MD") 的依赖方向说明里"
    fi
  done
fi
ok "依赖方向（${edges} 条本仓库依赖边 + 各 crate 的 crate 名登记）"

# ---- 约束「依赖方向」附加红线：测试桩不得进生产构建 ----
stub_deps=0
while IFS='|' read -r owner manifest sec key start text; do
  [ -n "$owner" ] || continue
  [ "$key" = "gloss-core" ] || continue
  case "$sec" in
    *dev-dependencies*) continue ;;
  esac
  case "$text" in
    *test-util*)
      fail "约束「依赖方向」：${owner} 的非 dev 依赖启用了 gloss-core/test-util（$(rel "$manifest"):${start}）——测试桩会进生产构建，只允许写在 dev-dependencies 里"
      ;;
    *) stub_deps=$((stub_deps + 1)) ;;
  esac
done <<<"$DEP_DUMP"
ok "测试桩不进生产构建（核对 ${stub_deps} 条非 dev 的本仓库依赖）"

# ---- 约束「日志统一出口」 ----
log_owners=""
while IFS='|' read -r owner manifest sec key start text; do
  case "$key" in
    tracing | tracing-appender | tracing-subscriber)
      [ "$owner" = "gloss-core" ] ||
        fail "约束「日志统一出口」：$(basename "$owner") 直接依赖了 ${key}（$(rel "$manifest"):${start}）——只经 gloss_core::log 使用"
      ;;
  esac
done <<<"$DEP_DUMP"
log_owners="$(printf '%s' "$DEP_DUMP" | cut -d'|' -f1 | sort -u | wc -l | tr -d ' ')"
ok "日志统一出口（${log_owners} 个 crate 的日志依赖归属）"

# ---- 约束「日志一律英文」 ----
# 把源文件折成一行再抓日志宏的实参（到第一个右括号为止），实参里出现非 ASCII
# 字节即违规；行内反引号/注释里的中文不受影响（不在宏实参里）。
rs_files=0
log_violations=0
while IFS= read -r f; do
  rs_files=$((rs_files + 1))
  if hits="$(tr '\n' ' ' <"$f" |
    LC_ALL=C grep -oE '(info|warn|error|debug|trace|event|info_span|warn_span|error_span|debug_span|trace_span)!\(([^)]*)\)' |
    LC_ALL=C grep '[^ -~]')"; then
    log_violations=$((log_violations + 1))
    fail "约束「日志一律英文」：$(rel "$f") 的日志实参含非 ASCII —— $(printf '%s' "$hits" | head -n 1 | cut -c1-60)"
  fi
done < <(find "$ROOT" -name '*.rs' -type f -not -path "$ROOT/target/*" -not -path "$ROOT/.git/*")
ok "日志一律英文（扫 ${rs_files} 个 .rs 文件，${log_violations} 处违规）"

# ---- 约束「版本单点维护」 ----
root_ws=""
if [ -f "$ROOT/Cargo.toml" ]; then
  root_ws="$(section_body "$ROOT/Cargo.toml" "[workspace.package]")"
fi
if ! printf '%s' "$root_ws" | grep -qE '^version[[:space:]]*=[[:space:]]*"'; then
  fail "约束「版本单点维护」：根 $(rel "$ROOT/Cargo.toml") 的 [workspace.package] 没有 version 字面量（版本单点就在这里）"
fi
if ! printf '%s' "$root_ws" | grep -qE '^edition[[:space:]]*=[[:space:]]*"'; then
  fail "约束「版本单点维护」：根 $(rel "$ROOT/Cargo.toml") 的 [workspace.package] 没有 edition 字面量"
fi
for m in $MANIFEST_LIST; do
  pkg="$(section_body "$m" "[package]")"
  for field in version edition; do
    if ! printf '%s' "$pkg" | grep -qE "^${field}\.workspace[[:space:]]*=[[:space:]]*true"; then
      fail "约束「版本单点维护」：$(rel "$m") 的 [package] 缺少 ${field}.workspace = true（不要硬写字面量）"
    fi
  done
done
ok "版本单点维护（根 [workspace.package] + 各 crate 的 *.workspace = true）"

# ---- 约束「依赖只开需要的特性」 ----
deps_checked=0
while IFS='|' read -r owner manifest sec key start text; do
  [ -n "$key" ] || continue
  # 本仓库 path 依赖（不带 version）与 workspace 继承项不适用这条
  if is_path_dep "$text"; then
    case "$text" in
      *version*) ;;
      *) continue ;;
    esac
  fi
  case "$text" in *"workspace = true"*) continue ;; esac
  deps_checked=$((deps_checked + 1))
  case "$text" in
    *"default-features = false"*) ;;
    *) fail "约束「依赖只开需要的特性」：$(rel "$manifest"):${start} 的 ${key} 未写 default-features = false" ;;
  esac
done <<<"$DEP_DUMP"
ok "依赖只开需要的特性（检查 ${deps_checked} 条第三方依赖声明）"

if [ "$FAILED" -ne 0 ]; then
  echo "" >&2
  echo "错误：有 ${FAILED} 处违反「不可协商的约束」（见上）。" >&2
  echo "  - 约束本身不打算改：改代码；" >&2
  echo "  - 约束确实要改：先改 $(rel "$AGENTS_MD")，再改本脚本与 deny.toml，别只改一处。" >&2
  exit 1
fi

echo "✓ 约束检查通过（${CHECKS} 项）"
exit 0
