#!/usr/bin/env bash
# 把打包好的 .app 压成 .dmg（UDZO 压缩镜像，Finder 打开即拖装）。
# cargo-bundle 的 osx 格式只产 .app，.dmg 由本脚本用系统自带的 hdiutil 补齐，
# 不引入 create-dmg 之类的第三方依赖。
# 用法：./scripts/bundle-dmg.sh <path/to/Gloss.app> [输出.dmg 路径]
set -euo pipefail

if [ "$#" -lt 1 ]; then
  echo "用法: $0 <path/to/Gloss.app> [输出.dmg 路径]" >&2
  exit 2
fi

APP="$1"
if [ ! -d "$APP" ]; then
  echo "错误：找不到 .app：${APP}（先跑 cargo bundle --release --format osx）" >&2
  exit 1
fi

# 默认与 .app 同目录同名
OUT="${2:-$(dirname "$APP")/$(basename "$APP" .app).dmg}"
mkdir -p "$(dirname "$OUT")"

# -ov 覆盖同名旧镜像；UDZO 是只读压缩格式，发布产物的标准选择
hdiutil create -volname "Gloss" -srcfolder "$APP" -ov -format UDZO "$OUT"
echo "✓ 已生成 $OUT"
