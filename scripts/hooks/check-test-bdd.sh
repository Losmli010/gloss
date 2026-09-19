#!/usr/bin/env bash
# BDD 清单一致性门禁：docs/tests/bdd.md 与测试源码双向核对。
#   - 测试源码侧：git 跟踪 .rs 里的 #[test] / #[tokio::test] 函数，加上
#     Cargo.toml 声明的 [[test]] 目标（harness = false 自检），每一项都必须
#     登记在 bdd.md；
#   - 清单侧：bdd.md 的每个条目必须能在测试源码里找到同名测试。
# 条目的描述文字不在校验面（描述与代码冲突时以代码为准，见 AGENTS.md「测试」节）。
# 本地 `just test-bdd`（pre-commit 的一部分）与 CI 的 Test BDD check job 共用此脚本。
set -euo pipefail

SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${1:-$(cd "$SELF_DIR/../.." && pwd)}"
BDD="$ROOT/docs/tests/bdd.md"

FAILED=0

fail() {
  echo "  ✗ $1" >&2
  FAILED=$((FAILED + 1))
}

rel() { printf '%s' "${1#"$ROOT"/}"; }

if [ ! -f "$BDD" ]; then
  echo "错误：找不到 $(rel "$BDD")" >&2
  echo "  测试行为清单是 git 追踪文件，别删除或改名；目录被 gitignore 例外放行。" >&2
  exit 1
fi

# ---- 源码侧：测试函数名 ----
# #[test] / #[tokio::test] 属性行触发 pending；跳过中间的其它属性行
# （如 #[ignore = "..."]）取最近的 fn 名；属性与 fn 同行也支持。
src_names="$(
  while IFS= read -r -d '' f; do
    awk '
      /^[[:space:]]*#\[(tokio::)?test\]/ {
        if (match($0, /fn[[:space:]]+[A-Za-z0-9_]+/)) {
          line = substr($0, RSTART, RLENGTH)
          sub(/^fn[[:space:]]+/, "", line)
          print line
        } else {
          pending = 1
        }
        next
      }
      pending && /^[[:space:]]*#/ { next }
      pending && match($0, /fn[[:space:]]+[A-Za-z0-9_]+/) {
        line = substr($0, RSTART, RLENGTH)
        sub(/^fn[[:space:]]+/, "", line)
        print line
        pending = 0
      }
    ' "$ROOT/$f"
  done < <(git -C "$ROOT" ls-files -z -- '*.rs') | sort -u
)"

# [[test]] 声明的测试目标名（harness = false 自检二进制没有 #[test] 函数）
target_names="$(
  {
    grep -h -A3 -E '^\[\[test\]\]' "$ROOT/Cargo.toml" "$ROOT"/crates/*/Cargo.toml 2>/dev/null || true
  } | sed -nE 's/^[[:space:]]*name[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' | sort -u
)"

# ---- 清单侧：bdd.md 条目名 ----
# 表格取首列，人工测试取 ### 小节标题；剥掉快照重名的（popup）/（settings）
# 限定后缀；表头、分隔行与 crates/... 文件标题经标识符过滤自然排除。
bdd_names="$(
  {
    grep -E '^### ' "$BDD" | sed -E 's/^#[#]*[[:space:]]*//'
    grep -E '^\|' "$BDD" | sed -E 's/^\|[[:space:]]*//; s/[[:space:]]*\|.*$//'
  } | sed -E 's/（[^）]*）[[:space:]]*$//' \
    | grep -E '^[A-Za-z0-9_]+$' | sort -u
)"

if [ -z "$src_names" ] && [ -z "$target_names" ]; then
  echo "错误：没有从测试源码提取到任何测试（git 跟踪的 .rs 与 [[test]] 目标均为空）。" >&2
  exit 1
fi

# ---- 双向核对 ----
missing_in_bdd="$(comm -23 <(printf '%s\n' "$src_names" "$target_names" | grep -v '^$' | sort -u) \
                         <(printf '%s\n' "$bdd_names" | grep -v '^$' | sort -u) || true)"
stale_in_bdd="$(comm -13 <(printf '%s\n' "$src_names" "$target_names" | grep -v '^$' | sort -u) \
                        <(printf '%s\n' "$bdd_names" | grep -v '^$' | sort -u) || true)"

if [ -n "$missing_in_bdd" ]; then
  fail "以下测试未登记进 bdd.md："
  printf '%s\n' "$missing_in_bdd" | sed 's/^/    /' >&2
fi

if [ -n "$stale_in_bdd" ]; then
  fail "以下 bdd.md 条目在测试源码中不存在（改名或删除后未同步）："
  printf '%s\n' "$stale_in_bdd" | sed 's/^/    /' >&2
fi

if [ "$FAILED" -ne 0 ]; then
  echo "" >&2
  echo "错误：bdd.md 与测试源码不一致（见上）。" >&2
  echo "  - 新增 / 改名 / 删除测试：在同一 PR 内同步登记 bdd.md 并刷新该条目更新时间；" >&2
  echo "  - 描述与代码冲突：以代码为准，立即修正 bdd.md。" >&2
  exit 1
fi

src_total="$(printf '%s\n' "$src_names" "$target_names" | grep -cv '^$' || true)"
bdd_total="$(printf '%s\n' "$bdd_names" | grep -cv '^$' || true)"
echo "✓ bdd.md 与测试源码一致（源码 ${src_total} 项 / 清单 ${bdd_total} 项）"
exit 0
