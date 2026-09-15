#!/usr/bin/env bash
# AGENTS.md 引用一致性门禁：校验指令文件里提到的仓库事实确实存在。
# 本地 `just agents-doc`（pre-commit 的一部分）与 CI 的 quality job 共用此脚本，
# 保证本地与 CI 判定一致。
#
# 为什么需要它：这份文档原本叫 AGENT.md（单数），而客户端解析指令文件时只认
# AGENTS.md，于是它在从未被加载的状态下漂移出了已删除的命令（`cargo run
# -- --overlay-selftest` 随自检迁入 tests/ 后失效，无人发现）。"命令能打印出来
# 就别在文档里复述"这条原则靠人记不住，交给脚本。
#
# 校验对象以 AGENTS.md 为准，五类引用：
#   1. 全文任何 `just <recipe>` 提到的配方必须在 justfile 中定义；
#   2. 行内反引号里的仓库路径（含 `/`）必须存在（文件或目录）；
#   3. 行内反引号里的裸文件名（justfile 或带 .md/.toml/.sh/.rs/.yml/.json 扩展名）
#      必须在仓库根存在；
#   4. 全文任何 `cargo test --test <name>` 提到的测试目标必须存在对应的
#      tests/<name>.rs（根包或任一 crate）；
#   5. 仓库各处（*.rs / *.toml / *.sh / *.yml / justfile / *.md）对约束名的指名
#      引用 `约束「名字」` 必须真的在文档的「不可协商的约束」段里——条号会随重排
#      失效，所以引用一律指名，并有这条盯着。
#
# 扫描范围：`just <recipe>` 与 `cargo test --test` 逐行扫描，代码块内同样算数
# ——命令清单本来就写在代码块里，漏掉它等于放过了最该校验的部分。路径只认行内
# 反引号包裹的仓库相对路径，代码块里的树形图与行尾注释混排没法可靠切词。
# 占位符（含 `*`、`<`、`>`）、仓库外路径（含 `~`）、注释语法与 URL（含 `//`）不校验。
#
# 用法：scripts/check-agents-doc.sh [AGENTS.md 路径]   # 缺省为仓库根 AGENTS.md
set -euo pipefail

SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ "$#" -ge 1 ]; then
  ROOT="$(cd "$(dirname "$1")" && pwd)"
  DOC="$ROOT/$(basename "$1")"
else
  ROOT="$(cd "$SELF_DIR/.." && pwd)"
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
  # 不用 \b：BSD grep 对词边界转义的兼容性不稳，改用「行首或非词字符 + just 」
  # 并排除 justfile 这类无空格的词。
  while IFS= read -r recipe; do
    [ -n "$recipe" ] || continue
    CHECKED=$((CHECKED + 1))
    # 配方行形如 `name:` 或 `name arg:`；结尾要求空白或行尾，避免 `logs` 命中
    # `logs-dir:`；`[^:=]+` 则挡掉 `coverage_min := "70"` 这类变量赋值。
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

# ---- 5. 仓库各处对约束名的指名引用必须存在 ----
# 约定：引用约束写成 `AGENTS.md 约束「名字」`，不写条号——条号会随重排失效
# （上一轮重排就断掉了两处源码注释里的「第 N 条」）。改了名字忘了改引用，这里当场失败。
#
# 扫描范围：*.rs / *.toml / *.yml / justfile / *.sh 里「同一行同时出现 AGENTS.md 与
# 约束「」」的引用。不含 *.md——文档里 「」 另有用途（设计文档里的「约束」是另一套
# 语境），扫进来只会误报。本脚本自身与其单测按构造就含这个模式（正则字面量与夹具
# 数据），故跳过，见下方 case。
CONSTRAINT_NAMES="$(
  awk '/^## / { insec = ($0 == "## 不可协商的约束") } insec { print }' "$DOC" |
    grep -oE '\*\*\[[A-Z]+\] [^*]+\*\*' | sed -E 's/^\*\*\[[A-Z]+\] //; s/\*\*$//' || true
)"
REF_PATTERN='AGENTS\.md.*约束「'
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
    if ! printf '%s\n' "$CONSTRAINT_NAMES" | grep -qxF "$name"; then
      fail "${ref_file#"$ROOT"/}: 引用了约束「${name}」，但 ${DOC_NAME} 里没有这个约束名"
    fi
  done < <(grep -E "$REF_PATTERN" "$ref_file" | grep -oE '约束「[^」]+」' | sed -E 's/^约束「//; s/」$//')
done < <(find "$ROOT" -type f \
  \( -name '*.rs' -o -name '*.toml' -o -name '*.sh' -o -name '*.yml' -o -name 'justfile' \) \
  -not -path "$ROOT/target/*" -not -path "$ROOT/.git/*")

if [ "$FAILED" -ne 0 ]; then
  echo "" >&2
  echo "错误：${DOC_NAME} 有 ${FAILED} 处引用与其来源不符（见上）。" >&2
  echo "  - 命令或路径确实变了：改文档，别让它描述一个不存在的世界；" >&2
  echo "  - 只是举例或占位：换个措辞，或写成含 * < > ~ 的形式跳过校验；" >&2
  echo "  - 约束改了名字：把引用它的地方一起改（用 约束「名字」 指名，别用条号）。" >&2
  exit 1
fi

echo "✓ ${DOC_NAME} 引用一致（校验 ${CHECKED} 处：just 配方 / 仓库路径 / 测试目标；另在 ${ref_files} 个文件里核对 ${refs} 处约束名引用）"
exit 0
