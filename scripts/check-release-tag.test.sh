#!/usr/bin/env bash
# check-release-tag.sh 的单元测试（纯 bash 轻量断言，零依赖）
# 覆盖：一致通过、不一致拒绝、tag 格式（缺 v / 段数不对）、
# [workspace.package] 段定位（version.workspace 与依赖段版本不得误读）、
# 段缺失报错、参数缺失用法提示。
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CHECKER="$SCRIPT_DIR/check-release-tag.sh"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

PASS=0
FAIL=0

# 建一个最小仓库夹具：把被测脚本拷进去（脚本按自身位置定位仓库根），
# Cargo.toml 内容用例定制。
new_fixture() {
  local dir="$1" manifest="$2"
  mkdir -p "$dir/scripts"
  cp "$CHECKER" "$dir/scripts/"
  printf '%s\n' "$manifest" >"$dir/Cargo.toml"
}

# 断言助手：$1=描述  $2=期望退出码(0=通过,非0=拒绝)  $3=tag  $4=Cargo.toml 内容
# 可选 $5：输出里必须出现的字符串——只看退出码不够：bash 3.2 的变量名吞字节
# 这类 bug 也以非零退出收场，会把「脚本报错」误判成「校验拒绝」。
expect() {
  local desc="$1" expected="$2" tag="$3" manifest="$4" needle="${5:-}"
  local fixture out rc
  fixture="$TMP/case-$PASS-$FAIL"
  new_fixture "$fixture" "$manifest"
  out="$(cd "$fixture" && bash scripts/check-release-tag.sh "$tag" 2>&1)"
  rc=$?
  if [ "$rc" -ne "$expected" ]; then
    FAIL=$((FAIL + 1))
    echo "FAIL - ${desc}（期望退出码 ${expected}，实际 ${rc}）" >&2
    echo "------- 输出 -------" >&2
    printf '%s\n' "$out" >&2
    return 0
  fi
  if [ -n "$needle" ] && ! printf '%s' "$out" | grep -q "$needle"; then
    FAIL=$((FAIL + 1))
    echo "FAIL - ${desc}（输出缺「${needle}」，可能被脚本报错顶替）" >&2
    echo "------- 输出 -------" >&2
    printf '%s\n' "$out" >&2
    return 0
  fi
  PASS=$((PASS + 1))
  echo "ok  - $desc"
}

MANIFEST_OK='[package]
name = "gloss"
version.workspace = true

[workspace.package]
version = "0.1.0"
edition = "2024"'

expect "tag 与版本一致 → 通过" 0 "v0.1.0" "$MANIFEST_OK" "✓ 版本号一致"
expect "多位版本号一致 → 通过" 0 "v10.20.30" '[workspace.package]
version = "10.20.30"' "✓ 版本号一致"
expect "tag 与版本不一致 → 拒绝" 1 "v0.2.0" "$MANIFEST_OK" "版本号不一致"
expect "tag 缺 v 前缀 → 拒绝" 1 "0.1.0" "$MANIFEST_OK" "缺 v 前缀"
expect "tag 只有两段 → 拒绝" 1 "v0.1" "$MANIFEST_OK" "不符合"
expect "tag 四段 → 拒绝" 1 "v0.1.0.1" "$MANIFEST_OK" "不符合"

# version.workspace = true（根包继承行）与依赖段的 version 都不得被当成单点版本：
# 单点行故意放在 [workspace.package]，值与继承行、依赖行都不同。
MANIFEST_TRAPS='[package]
name = "gloss"
version.workspace = true

[dependencies]
serde = { version = "9.9.9", default-features = false }

[workspace.package]
version = "1.2.3"'

expect "继承行与依赖段版本不误读，只认单点段" 0 "v1.2.3" "$MANIFEST_TRAPS" "✓ 版本号一致"

# 单点被挪走（段里没有 version）必须报错，而不是静默放行
expect "[workspace.package] 缺 version → 拒绝" 1 "v0.1.0" '[workspace.package]
edition = "2024"' "没能从 Cargo.toml"

# 参数缺失 → 用法提示（退出码 2）
out="$(bash "$CHECKER" 2>&1)"
rc=$?
if [ "$rc" -eq 2 ]; then
  PASS=$((PASS + 1))
  echo "ok  - 缺参数 → 用法提示退出码 2"
else
  FAIL=$((FAIL + 1))
  echo "FAIL - 缺参数 → 用法提示退出码 2(实际 ${rc})" >&2
fi

echo "----------------------------------------"
echo "通过 $PASS / $((PASS + FAIL))"
if [ "$FAIL" -ne 0 ]; then
  exit 1
fi
