#!/usr/bin/env bash
# gen-manifest.sh 的单元测试（纯 bash 轻量断言，零依赖）
# 覆盖：正常生成（version/url/size/sha256 实测）、缺 zip / 缺 dmg 拒绝、
# tag 与 Cargo.toml 版本不一致拒绝（版本单点复用）、tag 格式拒绝、缺参数用法提示。
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GENERATOR="$SCRIPT_DIR/gen-manifest.sh"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

PASS=0
FAIL=0

# 建一个最小仓库夹具：被测脚本与版本单点校验脚本都拷进去（前者按自身位置调用后者），
# Cargo.toml 内容用例定制，artifacts 目录放已知内容的产物文件。
new_fixture() {
  local dir="$1" manifest="$2"
  mkdir -p "$dir/scripts/release" "$dir/artifacts"
  cp "$GENERATOR" "$dir/scripts/release/"
  cp "$SCRIPT_DIR/check-release-tag.sh" "$dir/scripts/release/"
  printf '%s\n' "$manifest" >"$dir/Cargo.toml"
  printf 'arm64-zip-payload' >"$dir/artifacts/gloss-0.1.0-aarch64-apple-darwin.zip"
  printf 'arm64-dmg-payload' >"$dir/artifacts/gloss-0.1.0-aarch64-apple-darwin.dmg"
  printf 'x64-zip-payload' >"$dir/artifacts/gloss-0.1.0-x86_64-apple-darwin.zip"
  printf 'x64-dmg-payload' >"$dir/artifacts/gloss-0.1.0-x86_64-apple-darwin.dmg"
}

# 断言助手：$1=描述  $2=期望退出码(0=通过,非0=拒绝)  $3=tag  $4=Cargo.toml 内容
# 可选 $5：输出里必须出现的字符串——不只看退出码，防「脚本报错」被误判成「校验拒绝」。
expect() {
  local desc="$1" expected="$2" tag="$3" manifest="$4" needle="${5:-}"
  local fixture out rc
  fixture="$TMP/case-$PASS-$FAIL"
  new_fixture "$fixture" "$manifest"
  out="$(cd "$fixture" && bash scripts/release/gen-manifest.sh "$tag" artifacts 2>/dev/null)"
  rc=$?
  if [ "$rc" -ne "$expected" ]; then
    FAIL=$((FAIL + 1))
    echo "FAIL - ${desc}（期望退出码 ${expected}，实际 ${rc}）" >&2
    return 0
  fi
  if [ -n "$needle" ] && ! printf '%s' "$out" | grep -q "$needle"; then
    FAIL=$((FAIL + 1))
    echo "FAIL - ${desc}（输出缺「${needle}」）" >&2
    echo "------- 输出 -------" >&2
    printf '%s\n' "$out" >&2
    return 0
  fi
  PASS=$((PASS + 1))
  echo "ok  - $desc"
}

MANIFEST_OK='[workspace.package]
version = "0.1.0"
edition = "2024"'

# 正常生成：版本、双架构条目、直链、实测 size 与 sha256
expect "版本与双架构条目生成 → 通过" 0 "v0.1.0" "$MANIFEST_OK" '"version": "0.1.0"'
expect "aarch64 zip 直链与 sha256 → 通过" 0 "v0.1.0" "$MANIFEST_OK" \
  '"url": "https://losmli010.github.io/gloss/latest/gloss-0.1.0-aarch64-apple-darwin.zip"'
expect "x86_64 dmg 直链 → 通过" 0 "v0.1.0" "$MANIFEST_OK" \
  '"dmg_url": "https://losmli010.github.io/gloss/latest/gloss-0.1.0-x86_64-apple-darwin.dmg"'

# size/sha256 必须是产物文件实测值（拿夹具真实算一遍对照）
FIXTURE="$TMP/case-measure"
new_fixture "$FIXTURE" "$MANIFEST_OK"
EXPECT_SHA="$(shasum -a 256 "$FIXTURE/artifacts/gloss-0.1.0-aarch64-apple-darwin.zip" | awk '{print $1}')"
EXPECT_SIZE="$(wc -c < "$FIXTURE/artifacts/gloss-0.1.0-aarch64-apple-darwin.zip" | tr -d '[:space:]')"
out="$(cd "$FIXTURE" && bash scripts/release/gen-manifest.sh v0.1.0 artifacts)"
if printf '%s' "$out" | grep -q "\"size\": ${EXPECT_SIZE}" \
  && printf '%s' "$out" | grep -q "\"sha256\": \"${EXPECT_SHA}\""; then
  PASS=$((PASS + 1))
  echo "ok  - size 与 sha256 为产物实测值"
else
  FAIL=$((FAIL + 1))
  echo "FAIL - size 与 sha256 为产物实测值" >&2
  printf '%s\n' "$out" >&2
fi

# 缺产物文件 → 拒绝（残缺清单不得发出）：expect 的夹具总是齐备的，
# 缺文件用例单独建夹具删文件后再跑，并断言错误信息是「缺产物文件」。
expect_missing() {
  local desc="$1" victim="$2" fixture out rc
  fixture="$TMP/case-missing-$PASS-$FAIL"
  new_fixture "$fixture" "$MANIFEST_OK"
  rm -f "$fixture/artifacts/$victim"
  out="$(cd "$fixture" && bash scripts/release/gen-manifest.sh v0.1.0 artifacts 2>&1)"
  rc=$?
  if [ "$rc" -ne 0 ] && printf '%s' "$out" | grep -q "缺产物文件"; then
    PASS=$((PASS + 1))
    echo "ok  - $desc"
  elif [ "$rc" -eq 0 ]; then
    FAIL=$((FAIL + 1))
    echo "FAIL - ${desc} → 应拒绝，实际通过" >&2
    printf '%s\n' "$out" >&2
  else
    FAIL=$((FAIL + 1))
    echo "FAIL - ${desc} → 拒绝但错误信息不是「缺产物文件」" >&2
    printf '%s\n' "$out" >&2
  fi
}

expect_missing "缺 zip → 拒绝" "gloss-0.1.0-aarch64-apple-darwin.zip"
expect_missing "缺 dmg → 拒绝" "gloss-0.1.0-x86_64-apple-darwin.dmg"

# 版本单点复用：tag 与 Cargo.toml 不一致、tag 格式坏 → 一并拒绝
expect "tag 与 Cargo.toml 版本不一致 → 拒绝" 1 "v0.2.0" "$MANIFEST_OK"
expect "tag 缺 v 前缀 → 拒绝" 1 "0.1.0" "$MANIFEST_OK"

# 缺参数 → 用法提示（退出码 2）
out="$(bash "$GENERATOR" 2>&1)"
rc=$?
if [ "$rc" -eq 2 ]; then
  PASS=$((PASS + 1))
  echo "ok  - 缺参数 → 用法提示退出码 2"
else
  FAIL=$((FAIL + 1))
  echo "FAIL - 缺参数 → 用法提示退出码 2（实际 ${rc}）" >&2
fi

echo "----------------------------------------"
echo "通过 $PASS / $((PASS + FAIL))"
if [ "$FAIL" -ne 0 ]; then
  exit 1
fi
