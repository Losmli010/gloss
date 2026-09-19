#!/usr/bin/env bash
# AGENTS.md 引用一致性门禁：校验指令文件里提到的仓库事实确实存在。
# 本地 `just agents-doc`（pre-commit 的一部分）与 CI 的 Docs check job 共用此脚本，
# 保证本地与 CI 判定一致。
#
# 校验对象以 AGENTS.md 为准，五类引用：
#   1. 全文任何 `just <recipe>` 提到的配方必须在 justfile 中定义；
#   2. 行内反引号里的仓库路径（含 `/`）必须存在（文件或目录）；
#   3. 行内反引号里的裸文件名（justfile 或带 .md/.toml/.sh/.rs/.yml/.json 扩展名）
#      必须在仓库根存在；
#   4. 全文任何 `cargo test --test <name>` 提到的测试目标必须存在对应的
#      tests/<name>.rs（根包或任一 crate）；
#   5. 仓库各处（*.rs / *.toml / *.sh / *.yml / justfile / *.md）对质量条目的指名
#      引用 `质量条目「名字」` 必须真的在文档的「质量条目总表」段里（带 **名字** 的条目）。
#
# 扫描范围：`just <recipe>` 与 `cargo test --test` 逐行扫描，代码块内同样算数。
# 路径只认行内反引号包裹的仓库相对路径。
# 占位符（含 `*`、`<`、`>`）、仓库外路径（含 `~`）、注释语法与 URL（含 `//`）不校验。
#
# 用法：scripts/hooks/check-agents-doc.sh [AGENTS.md 路径]   # 缺省为仓库根 AGENTS.md
set -euo pipefail

SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ "$#" -ge 1 ]; then
  ROOT="$(cd "$(dirname "$1")" && pwd)"
  DOC="$ROOT/$(basename "$1")"
else
  ROOT="$(cd "$SELF_DIR/../.." && pwd)"
  DOC="$ROOT/AGENTS.md"
fi

DOC_NAME="${DOC#"$ROOT"/}"
JUSTFILE="$ROOT/justfile"

if [ ! -f "$DOC" ]; then
  echo "错误：找不到指令文件 $DOC" >&2
  echo "  客户端只认 AGENTS.md（没有 AGENT.md 之类的别名），别改名或挪位。" >&2
  exit 1
fi

FAILED=0
CHECKED=0

fail() {
  echo "  ✗ $1" >&2
  FAILED=$((FAILED + 1))
}

LINE_NO=0
while IFS= read -r line; do
  LINE_NO=$((LINE_NO + 1))

  # ---- 1. just 配方 ----
  while IFS= read -r recipe; do
    [ -n "$recipe" ] || continue
    CHECKED=$((CHECKED + 1))
    if [ ! -f "$JUSTFILE" ] ||
      ! grep -qE "^${recipe}([[:space:]][^:=]+)?:([[:space:]]|\$)" "$JUSTFILE"; then
      fail "${DOC_NAME}:${LINE_NO}  「just ${recipe}」在 justfile 中找不到同名配方"
    fi
  done < <(printf '%s\n' "$line" | grep -oE '(^|[^A-Za-z0-9_-])just [a-z][a-z0-9-]*' | sed -E 's/.*just //' || true)

  # ---- 2/3. 行内反引号里的仓库路径与裸文件名 ----
  while IFS= read -r token; do
    [ -n "$token" ] || continue
    case "$token" in
      *'*'* | *'<'* | *'>'* | '~'* | *'//'*) continue ;;
    esac
    case "$token" in
      */*)
        CHECKED=$((CHECKED + 1))
        [ -e "$ROOT/$token" ] || fail "${DOC_NAME}:${LINE_NO}  「${token}」在仓库里不存在"
        ;;
      justfile | *.md | *.toml | *.sh | *.rs | *.yml | *.json)
        CHECKED=$((CHECKED + 1))
        [ -e "$ROOT/$token" ] || fail "${DOC_NAME}:${LINE_NO}  「${token}」在仓库根不存在"
        ;;
    esac
  done < <(printf '%s\n' "$line" | grep -oE '`[^`]+`' | tr -d '`' || true)

  # ---- 4. cargo test 目标 ----
  while IFS= read -r target; do
    [ -n "$target" ] || continue
    CHECKED=$((CHECKED + 1))
    found=0
    for candidate in "$ROOT/tests/${target}.rs" "$ROOT"/crates/*/tests/"${target}".rs; do
      if [ -f "$candidate" ]; then
        found=1
      fi
    done
    [ "$found" -eq 1 ] ||
      fail "${DOC_NAME}:${LINE_NO}  「cargo test --test ${target}」找不到 tests/${target}.rs"
  done < <(printf '%s\n' "$line" | grep -oE '\-\-test [A-Za-z0-9_-]+' | sed -E 's/^--test //' || true)

done < "$DOC"

# ---- 5. 仓库各处对质量条目的指名引用必须存在 ----
# 约定：引用写成 `AGENTS.md 质量条目「名字」`；被指名的条目在总表里以 **名字** 起头。
#
# 扫描范围：*.rs / *.toml / *.yml / justfile / *.sh 里「同一行同时出现 AGENTS.md 与
# 质量条目「」」的引用。本脚本自身与其单测按构造就含这个模式，故跳过，见下方 case。
RULE_NAMES="$(
  awk '
    /^### 质量条目总表/ { insec = 1; next }
    insec && /^## / { insec = 0 }
    insec { print }
  ' "$DOC" |
    grep -oE '\*\*[^*]+\*\*' | sed -E 's/^\*\*//; s/\*\*$//' || true
)"
REF_PATTERN='AGENTS\.md.*质量条目「'
ref_files=0
refs=0
while IFS= read -r ref_file; do
  case "$(basename "$ref_file")" in
    check-agents-doc.sh | check-agents-doc.test.sh) continue ;;
  esac
  ref_files=$((ref_files + 1))
  while IFS= read -r name; do
    [ -n "$name" ] || continue
    refs=$((refs + 1))
    if ! printf '%s\n' "$RULE_NAMES" | grep -qxF "$name"; then
      fail "${ref_file#"$ROOT"/}: 引用了质量条目「${name}」，但 ${DOC_NAME} 的质量条目总表里没有这个条目名"
    fi
  done < <(grep -E "$REF_PATTERN" "$ref_file" | grep -oE '质量条目「[^」]+」' | sed -E 's/^质量条目「//; s/」$//')
done < <(find "$ROOT" -type f \
  \( -name '*.rs' -o -name '*.toml' -o -name '*.sh' -o -name '*.yml' -o -name 'justfile' \) \
  -not -path "$ROOT/target/*" -not -path "$ROOT/.git/*")

if [ "$FAILED" -ne 0 ]; then
  echo "" >&2
  echo "错误：${DOC_NAME} 有 ${FAILED} 处引用与其来源不符（见上）。" >&2
  echo "  - 命令或路径确实变了：改文档，别让它描述一个不存在的世界；" >&2
  echo "  - 只是举例或占位：换个措辞，或写成含 * < > ~ 的形式跳过校验；" >&2
  echo "  - 条目改了名字：把引用它的地方一起改（用 质量条目「名字」 指名）。" >&2
  exit 1
fi

echo "✓ ${DOC_NAME} 引用一致（校验 ${CHECKED} 处：just 配方 / 仓库路径 / 测试目标；另在 ${ref_files} 个文件里核对 ${refs} 处质量条目名引用）"
exit 0
