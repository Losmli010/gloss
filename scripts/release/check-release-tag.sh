#!/usr/bin/env bash
# 校验发布 tag 与版本单点一致：tag 必须是 v<主>.<次>.<补丁>，且去掉 v 后等于
# 根 Cargo.toml [workspace.package] 的 version。防止 tag 与产物版本错位地发出去。
# 本地打 tag 前手动跑（just release-check vX.Y.Z）；release workflow 构建产物前
# 跑同一条脚本——本地与 CI 共用同一判定，不给两套实现漂移的机会。
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"

if [ "$#" -ne 1 ]; then
  echo "用法: $0 <tag>（例：v0.1.0）" >&2
  exit 2
fi
TAG="$1"

# release workflow 只由 v* tag 触发，这里收紧为 v + 恰好三段纯数字，挡住 v1.2、
# v1.2.3.4、v1.2.x、漏了 v 的 0.1.0 这类手滑。bash 3.2 的 glob 表达不了
# 「恰好三段数字」（v[0-9]*.[0-9]*.[0-9]* 会放过 v0.1.0.1），用 awk 数段。
case "$TAG" in
  v*) ;;
  *)
    echo "错误：tag \"$TAG\" 缺 v 前缀（例：v0.1.0）" >&2
    exit 1
    ;;
esac
TAG_VERSION="${TAG#v}"
if ! printf '%s' "$TAG_VERSION" | awk -F. '
  NF != 3 { exit 1 }
  {
    for (i = 1; i <= 3; i++)
      if ($i !~ /^[0-9]+$/) exit 1
  }'; then
  echo "错误：tag \"$TAG\" 不符合 v<主>.<次>.<补丁> 格式（例：v0.1.0）" >&2
  exit 1
fi

# 版本只认 [workspace.package] 段里的 version 赋值（约束「版本单点维护」）。
# 不能全局 grep "version ="：根包的 version.workspace = true 和依赖段的版本
# 都长一个样，抓错一处判定就废了。
CARGO_VERSION="$(awk '
  /^[ \t]*\[workspace\.package\][ \t]*$/ { in_section = 1; next }
  /^[ \t]*\[/ { in_section = 0 }
  in_section && $1 == "version" {
    sub(/^[^=]*=[ \t]*/, "")
    gsub(/^"|"$/, "")
    print
    exit
  }
' "$ROOT_DIR/Cargo.toml")"

if [ -z "$CARGO_VERSION" ]; then
  echo "错误：没能从 Cargo.toml 的 [workspace.package] 段读到 version" >&2
  echo "  版本单点被挪走了？先确认 [workspace.package] 的 version 还在。" >&2
  exit 1
fi

echo "tag version:   $TAG_VERSION"
echo "cargo version: $CARGO_VERSION"
# 全角标点紧跟变量名时必须加花括号：bash 3.2 会把其首字节吞进变量名（实测踩坑）
if [ "$TAG_VERSION" != "$CARGO_VERSION" ]; then
  echo "错误：版本号不一致：tag=${TAG_VERSION}，Cargo.toml=${CARGO_VERSION}" >&2
  echo "  把根 Cargo.toml [workspace.package] 的 version 改成 $TAG_VERSION 后重新打 tag。" >&2
  exit 1
fi
echo "✓ 版本号一致"
