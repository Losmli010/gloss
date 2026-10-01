#!/usr/bin/env bash
# 界面文案统一进文案表门禁：界面词条只定义在 crates/gloss-app/i18n/*.toml，
# 只经 crates/gloss-app/src/i18n.rs 装入，代码里不内嵌文案字面量。
# 本地 `just i18n`（pre-commit 的一部分）与 CI 的 I18n check job 共用此脚本，
# 保证本地与 CI 判定一致。
#
# 覆盖三条检查：
#   文案字面量拦截  —— gloss-app 与根包 src 的生产代码里，字符串 / char /
#                      原始字符串字面量出现非 ASCII 字节即违规；#[cfg(test)] 模块、
#                      注释、#[...] 属性内部（clippy reason 等元信息）不在判罚面。
#                      纯 ASCII 的英文硬编码不可由文本判定，靠人工评审。
#   装入点集中      —— include_str! / include_bytes! 引用 i18n 资源只允许出现在
#                      crates/gloss-app/src/i18n.rs。
#   词条表文件唯一  —— 声明 gloss_ 前缀键的 TOML 只允许在 crates/gloss-app/i18n/ 下。
#
# 行尾 `i18n:allow` 放行所在行（同 secrets:allow 惯例），须注明缘由。
#
# 用法：scripts/hooks/check-i18n.sh [仓库根]   # 缺省为本脚本的上一级目录
set -euo pipefail

SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${1:-$(cd "$SELF_DIR/../.." && pwd)}"

RULE="界面文案统一进文案表"
OWNER="crates/gloss-app/src/i18n.rs"
CATALOG_DIR="crates/gloss-app/i18n"
SCAN_DIRS="crates/gloss-app/src 与根包 src"
ALLOW_MARKER="i18n:allow"

FAILED=0

fail() {
  echo "  ✗ $1" >&2
  FAILED=$((FAILED + 1))
}

rel() { printf '%s' "${1#"$ROOT"/}"; }

if ! command -v python3 >/dev/null 2>&1; then
  echo "错误：本门禁需要 python3 做字面量词法分析。" >&2
  exit 1
fi

echo "== i18n 文案表门禁（仓库根：$(rel "$ROOT")）=="

# ---- 检查一：文案字面量拦截 ----
# 词法状态机区分 代码 / 注释 / 字符串 / char / 原始字符串；#[...] 属性整体跳过
# （reason 等元信息可中文）；#[cfg(test)] 修饰的 mod 项按花括号配对整体跳过，
# 闭合后恢复扫描（测试断言按惯例可用中文钉文案）。命中行带 i18n:allow 则放行。
literal_violations="$(python3 - "$ROOT" "$ALLOW_MARKER" <<'PY'
import os
import sys

root, marker = sys.argv[1], sys.argv[2]


def rs_files(base):
    if not os.path.isdir(base):
        return
    for dirpath, dirnames, filenames in os.walk(base):
        dirnames[:] = sorted(d for d in dirnames if d != "tests")
        for name in sorted(filenames):
            if name.endswith(".rs"):
                yield os.path.join(dirpath, name)


def skip_blanks(src, i):
    n = len(src)
    while i < n:
        c = src[i]
        if c in " \t\r\n":
            i += 1
        elif c == "/" and i + 1 < n and src[i + 1] == "/":
            while i < n and src[i] != "\n":
                i += 1
        elif c == "/" and i + 1 < n and src[i + 1] == "*":
            depth = 1
            i += 2
            while i < n and depth:
                if src.startswith("/*", i):
                    depth += 1
                    i += 2
                elif src.startswith("*/", i):
                    depth -= 1
                    i += 2
                else:
                    i += 1
        else:
            break
    return i


def read_word(src, i):
    j = i
    while j < len(src) and (src[j].isalnum() or src[j] == "_"):
        j += 1
    return src[i:j], j


def attribute_span(src, i):
    """i 指向 '#'。是属性（#[..] 或 #![..]）时返回 (结束下标, 方括号内文本)，
    否则返回 (i, "")。方括号内的字符串 / char / 原始字符串按字面量整体跳过，
    其中的 ']' 不参与配对。"""
    n = len(src)
    j = i + 1
    if j < n and src[j] == "!":
        j += 1
    if j >= n or src[j] != "[":
        return i, ""
    depth = 1
    j += 1
    inner_start = j
    while j < n and depth:
        c = src[j]
        nxt = src[j + 1] if j + 1 < n else ""
        if c == '"':
            j += 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            j += 1
            continue
        if c == "r" and nxt in ('"', "#"):
            k = j + 1
            h = 0
            while k < n and src[k] == "#":
                h += 1
                k += 1
            if k < n and src[k] == '"':
                j = k + 1
                while j < n:
                    if src[j] == '"':
                        m = 0
                        while m < h and j + 1 + m < n and src[j + 1 + m] == "#":
                            m += 1
                        if m == h:
                            j += 1 + h
                            break
                    j += 1
                continue
        if c == "'":
            if nxt == "\\":
                j += 2
                while j < n and src[j] != "'":
                    j += 2 if src[j] == "\\" else 1
                j += 1
                continue
            if nxt and nxt != "'" and j + 2 < n and src[j + 2] == "'":
                j += 3
                continue
            j += 1
            continue
        if c == "[":
            depth += 1
        elif c == "]":
            depth -= 1
            if depth == 0:
                return j + 1, src[inner_start:j]
        j += 1
    return n, src[inner_start:n]


def test_mod_brace(src, i):
    """#[cfg(test)] 属性结束后，若紧跟一个带 body 的 mod 项（可带其他属性与
    pub 修饰），返回其 body '{' 的下标；否则返回 None（无 body 的 mod 声明、
    其他项均不算）。"""
    while True:
        i = skip_blanks(src, i)
        if i < len(src) and src[i] == "#":
            end, _ = attribute_span(src, i)
            if end == i:
                break
            i = end
            continue
        word, j = read_word(src, i)
        if word == "pub":
            i = j
            i = skip_blanks(src, i)
            if i < len(src) and src[i] == "(":
                depth = 1
                i += 1
                while i < len(src) and depth:
                    if src[i] == "(":
                        depth += 1
                    elif src[i] == ")":
                        depth -= 1
                    i += 1
            continue
        if word == "mod":
            i = j
            i = skip_blanks(src, i)
            _, j2 = read_word(src, i)
            if j2 == i:
                return None
            i = skip_blanks(src, j2)
            if i < len(src) and src[i] == "{":
                return i
            return None
        return None


def literal_lines(src):
    """生产代码（注释、属性、#[cfg(test)] mod 项之外）里含非 ASCII 字面量的行号。"""
    hits = set()
    i, n, line = 0, len(src), 1
    state = "code"
    hashes = 0
    block_depth = 0
    test_depth = 0
    test_pending = False
    while i < n:
        c = src[i]
        nxt = src[i + 1] if i + 1 < n else ""
        if c == "\n":
            line += 1
        if state == "line":
            if c == "\n":
                state = "code"
            i += 1
            continue
        if state == "block":
            if c == "/" and nxt == "*":
                block_depth += 1
                i += 2
                continue
            if c == "*" and nxt == "/":
                block_depth -= 1
                i += 2
                if block_depth == 0:
                    state = "code"
                continue
            i += 1
            continue
        if state in ("str", "raw"):
            if state == "str":
                if c == "\\":
                    i += 2
                    continue
                if c == '"':
                    state = "code"
                    i += 1
                    continue
            elif c == '"':
                k = 0
                while k < hashes and i + 1 + k < n and src[i + 1 + k] == "#":
                    k += 1
                if k == hashes:
                    state = "code"
                    i += 1 + hashes
                    continue
            if ord(c) > 127 and test_depth == 0 and not test_pending:
                hits.add(line)
            i += 1
            continue
        if state == "char":
            if c == "\\":
                i += 2
                continue
            if c == "'":
                state = "code"
            elif ord(c) > 127 and test_depth == 0 and not test_pending:
                hits.add(line)
            i += 1
            continue
        if c == "/" and nxt == "/":
            state = "line"
            i += 2
            continue
        if c == "/" and nxt == "*":
            state = "block"
            block_depth = 1
            i += 2
            continue
        if c == "#":
            end, inner = attribute_span(src, i)
            if end > i:
                if inner.replace(" ", "").replace("\t", "") == "cfg(test)":
                    brace = test_mod_brace(src, end)
                    if brace is not None:
                        test_pending = True
                        line += src.count("\n", i, brace)
                        i = brace
                        continue
                line += src.count("\n", i, end)
                i = end
                continue
            i += 1
            continue
        if c == "r":
            j = i + 1
            h = 0
            while j < n and src[j] == "#":
                h += 1
                j += 1
            if j < n and src[j] == '"':
                state = "raw"
                hashes = h
                i = j + 1
                continue
            i += 1
            continue
        if c == '"':
            state = "str"
            i += 1
            continue
        if c == "'":
            if nxt == "\\":
                state = "char"
                i += 1
                continue
            if nxt and nxt != "'" and i + 2 < n and src[i + 2] == "'":
                state = "char"
                i += 1
                continue
            i += 1
            continue
        if test_pending:
            test_pending = False
            if c == "{":
                test_depth = 1
                i += 1
                continue
        if test_depth > 0:
            if c == "{":
                test_depth += 1
            elif c == "}":
                test_depth -= 1
        i += 1
    return hits


for base in (
    os.path.join(root, "crates", "gloss-app", "src"),
    os.path.join(root, "src"),
):
    for path in rs_files(base):
        with open(path, encoding="utf-8", errors="replace") as fh:
            src = fh.read()
        lines = src.split("\n")
        for ln in sorted(literal_lines(src)):
            if marker in lines[ln - 1]:
                continue
            print(f"{path}:{ln}")
PY
)"

if [ -n "$literal_violations" ]; then
  while IFS= read -r hit; do
    [ -n "$hit" ] || continue
    fail "约束「${RULE}」：$(rel "${hit%:*}") 第 ${hit##*:} 行内嵌文案字面量 —— 词条进 ${CATALOG_DIR}/，经 ${OWNER} 的 Text 取用"
  done <<<"$literal_violations"
fi

# ---- 检查二：装入点集中 ----
# 文案文件是编译期资源，装入与解析只允许发生在 i18n.rs 一处；
# 别处 include 进来就绕开了类型化 Text 表与 zh/en 键一致性测试。
include_files=0
while IFS= read -r f; do
  include_files=$((include_files + 1))
  [ "$(rel "$f")" = "$OWNER" ] && continue
  hits="$(grep -nE 'include_(str|bytes)!\([^)]*i18n' "$f" 2>/dev/null || true)"
  [ -n "$hits" ] || continue
  fail "约束「${RULE}」：$(rel "$f") 在装入点之外引用了 i18n 资源 —— 只允许 ${OWNER} include"
done < <(find "$ROOT" -name '*.rs' -type f -not -path "$ROOT/target/*" -not -path "$ROOT/.git/*" 2>/dev/null)

# ---- 检查三：词条表文件唯一 ----
# 词条键统一 gloss_<模块>_<词条> 且顶层扁平；出现第二张表就会分叉出
# 不受键一致性测试管辖的文案面。
toml_files=0
while IFS= read -r f; do
  toml_files=$((toml_files + 1))
  case "$(rel "$f")" in
    "$CATALOG_DIR"/*) continue ;;
  esac
  hits="$(grep -nE '^[[:space:]]*gloss_[A-Za-z0-9_]+[[:space:]]*=' "$f" 2>/dev/null || true)"
  [ -n "$hits" ] || continue
  fail "约束「${RULE}」：$(rel "$f") 在 ${CATALOG_DIR}/ 之外声明了词条键 —— 文案表只此一处"
done < <(find "$ROOT" -name '*.toml' -type f -not -path "$ROOT/target/*" -not -path "$ROOT/.git/*" 2>/dev/null)

if [ "$FAILED" -ne 0 ]; then
  echo "" >&2
  echo "错误：有 ${FAILED} 处违反「${RULE}」（见上）。" >&2
  echo "  - 是界面文案：词条加进 ${CATALOG_DIR}/zh.toml 与 en.toml（键名 gloss_<模块>_<词条>），${OWNER} 的 Text 补同名字段，代码经 Text 取用；" >&2
  echo "  - 非文案（图标字形、自检字样、列表符号等）：所在行尾加 ${ALLOW_MARKER} 并注明缘由。" >&2
  exit 1
fi

echo "✓ ${RULE}（${SCAN_DIRS} 的生产代码字面量 + ${include_files} 个 .rs 的装入点 + ${toml_files} 个 TOML 的词条键）"
exit 0
