#!/usr/bin/env bash
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CHECKER="$SCRIPT_DIR/check-test-bdd.sh"

PASS=0
FAIL=0
CASE_NO=0

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
git init -q "$TMP"
mkdir -p "$TMP/scripts/hooks" "$TMP/src" "$TMP/docs/tests" "$TMP/tests"
cp "$CHECKER" "$TMP/scripts/hooks/"
chmod +x "$TMP/scripts/hooks/check-test-bdd.sh"

# 基线夹具：两个 Cargo.toml（含 [[test]] 目标）、一个源码测试文件、一份一致的 bdd.md
build_fixture() {
  cat >"$TMP/Cargo.toml" <<'EOF'
[package]
name = "gloss"
version.workspace = true
edition = "2024"

[[test]]
name = "overlay_selftest"
harness = false
EOF
  cat >"$TMP/src/lib.rs" <<'EOF'
#[test]
fn registered_plain() {}

#[tokio::test]
async fn registered_tokio() {}

#[test]
#[ignore = "needs auth"]
fn registered_ignored() {}

#[test] fn same_line_registered() {}

#[test]
fn snapshots_match_baseline() {}
EOF
  cat >"$TMP/docs/tests/bdd.md" <<'EOF'
# 测试行为清单（BDD）

## 总览
| 类别 | 数量 | 运行 |
| --- | --- | --- |
| 单元测试 | 5 | `just test` |

### crates/demo.rs
| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| registered_plain | 目标 | 给定…当…则… | 2026-09-19 |
| registered_tokio | 目标 | 给定…当…则… | 2026-09-19 |
| registered_ignored | 目标 | 给定…当…则… | 2026-09-19 |
| same_line_registered | 目标 | 给定…当…则… | 2026-09-19 |
| snapshots_match_baseline（popup） | 目标 | 给定…当…则… | 2026-09-19 |

## 性能测试
| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| overlay_selftest | 目标 | 给定…当…则… | 2026-09-19 |
EOF
  git -C "$TMP" add -A >/dev/null 2>&1
}

assert_case() {
  local desc="$1" expected="$2" mutate="${3-}" needle="${4-}"

  CASE_NO=$((CASE_NO + 1))
  build_fixture
  if [ -n "$mutate" ]; then "$mutate"; fi

  local out actual reason=""
  out="$(bash "$TMP/scripts/hooks/check-test-bdd.sh" 2>&1)"
  actual=$?

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
    echo "    输出: $(printf '%s' "${out}" | tail -n 4)"
    FAIL=$((FAIL + 1))
  fi
}

mut_unregistered_test() {
  printf '\n#[test]\nfn unregistered_thing() {}\n' >>"$TMP/src/lib.rs"
}
mut_stale_bdd_entry() {
  printf '| ghost_entry | 目标 | 场景 | 2026-09-19 |\n' >>"$TMP/docs/tests/bdd.md"
}
mut_drop_bdd_file() {
  rm -f "$TMP/docs/tests/bdd.md"
}
mut_rename_in_code_only() {
  # BSD/GNU sed 的 -i 语法不兼容，沿用 awk 改写（与 check-constraints.test.sh 同模式）
  awk '{ if ($0 == "fn registered_plain() {}") print "fn registered_plain_renamed() {}"; else print }' \
    "$TMP/src/lib.rs" >"$TMP/src/lib.rs.tmp" && mv "$TMP/src/lib.rs.tmp" "$TMP/src/lib.rs"
}
mut_file_heading_only_change() {
  # 同上：awk 改写保证 BSD/GNU 兼容
  awk '{ if ($0 == "### crates/demo.rs") print "### crates/renamed.rs"; else print }' \
    "$TMP/docs/tests/bdd.md" >"$TMP/docs/tests/bdd.md.tmp" &&
    mv "$TMP/docs/tests/bdd.md.tmp" "$TMP/docs/tests/bdd.md"
}

echo "== 测试 check-test-bdd.sh =="
echo ""
echo "-- 一致（应通过，退出码 0）--"
assert_case "全量登记一致（fn/tokio/ignore/同行属性/重名后缀剥除/[[test]] 目标）" 0
assert_case "bdd 文件小节标题（crates/…）变化不影响核对" 0 mut_file_heading_only_change
assert_case "表头与分隔行不误判为条目" 0

echo ""
echo "-- 不一致（应拒绝，退出码非 0）--"
assert_case "源码测试未登记进 bdd.md" 1 mut_unregistered_test "unregistered_thing"
assert_case "bdd.md 条目在源码中不存在" 1 mut_stale_bdd_entry "ghost_entry"
assert_case "源码改名后清单未同步" 1 mut_rename_in_code_only "registered_plain_renamed"
assert_case "bdd.md 缺失" 1 mut_drop_bdd_file "找不到"

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
