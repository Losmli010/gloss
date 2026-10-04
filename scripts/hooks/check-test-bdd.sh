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
# 「## 发版人工步骤」节登记无自动化测试源码的发版链路运维步骤，整节豁免双向核对。
bdd_names="$(
  awk '
    /^## /  { exempt = ($0 ~ /^## 发版人工步骤/) ? 1 : 0 }
    /^### / { if (!exempt) { sub(/^#[#]*[[:space:]]*/, ""); print } }
    /^\|/   { if (!exempt) { sub(/^\|[[:space:]]*/, ""); sub(/[[:space:]]*\|.*$/, ""); print } }
  ' "$BDD" \
    | sed -E 's/（[^）]*）[[:space:]]*$//' \
    | grep -E '^[A-Za-z0-9_]+$' | sort -u
)"

if [ -z "$src_names" ] && [ -z "$target_names" ]; then
  echo "错误：没有从测试源码提取到任何测试（git 跟踪的 .rs 与 [[test]] 目标均为空）。" >&2
  exit 1
fi

# ---- 双向核对 ----
# 两侧各自已去重，但 fn 名与 [[test]] 目标名可能同名（如 overlay 自检），
# 合并后须再去重，否则 comm 把多出的那份误报成「未登记」。
src_all_names="$(printf '%s\n' "$src_names" "$target_names" | grep -v '^$' | sort -u)"
missing_in_bdd="$(comm -23 <(printf '%s\n' "$src_all_names") \
                         <(printf '%s\n' "$bdd_names" | grep -v '^$' | sort -u) || true)"
stale_in_bdd="$(comm -13 <(printf '%s\n' "$src_all_names") \
                        <(printf '%s\n' "$bdd_names" | grep -v '^$' | sort -u) || true)"
# 重复登记：整行精确重复（同名同目标同场景同时间）＝改名后旧行未删的
# 真实特征。同名不同描述的沿革行（历史重录记录）是有意保留，不拦。
duplicate_rows="$(awk '
    /^## /  { exempt = ($0 ~ /^## 发版人工步骤/) ? 1 : 0 }
    /^\|/   {
      if (exempt) next
      if ($0 ~ /^[[:space:]]*\|[[:space:]]*测试名称/) next
      if ($0 ~ /^[[:space:]]*\|[[:space:]]*---/) next
      print
    }
  ' "$BDD" | sort | uniq -d)"

if [ -n "$stale_in_bdd" ]; then
  fail "以下 bdd.md 条目在测试源码中不存在（改名或删除后未同步）："
  printf '%s\n' "$stale_in_bdd" | sed 's/^/    /' >&2
fi

if [ -n "$missing_in_bdd" ]; then
  fail "以下测试未登记进 bdd.md："
  printf '%s\n' "$missing_in_bdd" | sed 's/^/    /' >&2
fi
duplicate_entries="$duplicate_rows"
if [ -n "$duplicate_entries" ]; then
  fail "以下 bdd.md 条目重复登记（同名条目数超过源码同名测试数，改名后旧行未删？）："
  printf '%s\n' "$duplicate_entries" | sed 's/^/    /' >&2
fi

# 章节唯一性：同名 ## 章节出现多次＝整段重复拼接的事故痕迹。
dup_chapters="$(grep '^## ' "$BDD" | sort | uniq -d)"
if [ -n "$dup_chapters" ]; then
  fail "以下 ## 章节标题重复出现（整段重复拼接的痕迹）："
  printf '%s\n' "$dup_chapters" | sed 's/^/    /' >&2
fi

# 块级重复：同一 ## 章节内重复的 ### 小节标题，或同一小节内逐字重复的
# 「- 」条目行——人工测试条目整段重复拼接的痕迹（表格行的重复由上面的
# 整行精确重复检查覆盖）。
duplicate_blocks="$(awk '
    /^## /  {
      chapter = $0
      exempt = ($0 ~ /^## 发版人工步骤/) ? 1 : 0
      section = exempt ? "" : $0
      split("", h3)
      split("", bullets)
      next
    }
    /^### / {
      if (exempt || chapter == "") next
      if (h3[$0]++) print $0
      section = $0
      split("", bullets)
      next
    }
    /^- / {
      if (exempt || section == "") next
      if (bullets[$0]++) print $0
      next
    }
  ' "$BDD" | sort -u)"
if [ -n "$duplicate_blocks" ]; then
  fail "以下小节标题或条目行在同一章节/小节内重复出现（整段重复拼接的痕迹）："
  printf '%s\n' "$duplicate_blocks" | sed 's/^/    /' >&2
fi

# 结构校验：非豁免小节里的每个表格必须是良构 GFM 表——表行连成一个块，
# 且块的第二行是分隔行。空行或正文混在表头与表体之间会把表体从表头上
# 切断（渲染退化为纯文本），一律按缺表头列出。
table_broken="$(awk '
  function end_run() {
    if (in_run && !run_ok && !exempt && section != "") broken[section] = 1
    in_run = 0
  }
  /^## /  {
    end_run()
    exempt = ($0 ~ /^## 发版人工步骤/) ? 1 : 0
    section = exempt ? "" : $0
    next
  }
  /^### / {
    if (exempt) next
    end_run()
    section = $0
    next
  }
  /^\|/ {
    if (exempt || section == "") next
    if (!in_run) { in_run = 1; run_len = 0; run_ok = 0 }
    run_len++
    if (run_len == 2 && $0 ~ /^[[:space:]]*\|[[:space:]]*---/) run_ok = 1
    next
  }
  { end_run() }
  END {
    end_run()
    for (s in broken) print s
  }
' "$BDD")"
if [ -n "$table_broken" ]; then
  fail "以下小节的表格缺表头（或表体被空行/正文从表头切断）："
  printf '%s\n' "$table_broken" | sed 's/^/    /' >&2
fi

if [ "$FAILED" -ne 0 ]; then
  echo "" >&2
  echo "错误：bdd.md 与测试源码不一致（见上）。" >&2
  echo "  - 新增 / 改名 / 删除测试：在同一 PR 内同步登记 bdd.md 并刷新该条目更新时间；" >&2
  echo "  - 描述与代码冲突：以代码为准，立即修正 bdd.md。" >&2
  exit 1
fi

src_total="$(printf '%s\n' "$src_all_names" | grep -cv '^$' || true)"
bdd_total="$(printf '%s\n' "$bdd_names" | grep -cv '^$' || true)"
echo "✓ bdd.md 与测试源码一致（源码 ${src_total} 项 / 清单 ${bdd_total} 项）"
exit 0
