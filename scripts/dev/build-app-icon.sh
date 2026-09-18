#!/usr/bin/env bash
# 从 assets/icons/gloss-app-icon.svg 重建应用图标产物（换 Logo 方案或调色后跑一次）：
#   assets/icons/Gloss.icns           打包用 —— cargo-bundle 的 icon 配置指向它
#   assets/icons/gloss-dock-icon.png  开发期 Dock 图标 —— 非 bundle 运行时由
#                                     gloss-platform 内嵌（include_bytes!）交给 NSApplication
# 只用系统自带的 sips（SVG→PNG、缩放）与 iconutil（iconset→.icns），零新增工具链：
# sips 自 macOS 13 起经 ImageIO 支持 SVG 输入，输出保留 alpha。
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
icons="${root}/assets/icons"
svg="${icons}/gloss-app-icon.svg"

for tool in sips iconutil; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "错误：缺少系统工具 ${tool}（应用图标产物只能在 macOS 上重建）" >&2
    exit 1
  }
done

if [ ! -f "$svg" ]; then
  echo "错误：找不到图标矢量源 ${svg}" >&2
  exit 1
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/Gloss.iconset"

# 1024 主图：icns 全部档位与 Dock 图标都由它降采样而来
sips -s format png "$svg" --out "$work/icon-1024.png" >/dev/null

# 光栅化尺寸取决于 SVG 声明的 width/height 属性：丢了它 sips 会按内嵌尺寸出小图，
# 下面的档位会把小图上采样成模糊图标静默进仓库，这里当场拦下
width="$(sips -g pixelWidth "$work/icon-1024.png" | awk 'END { print $NF }')"
height="$(sips -g pixelHeight "$work/icon-1024.png" | awk 'END { print $NF }')"
if [ "$width" != 1024 ] || [ "$height" != 1024 ]; then
  echo "错误：矢量源光栅化后为 ${width}×${height}，期望 1024×1024——gloss-app-icon.svg 的 width/height 属性不能省" >&2
  exit 1
fi

# iconset 的十个档位（基础的 1x 与 @2x 各五档，iconutil 认这套文件名）；
# 256 与 512 档在表里各出现两次（128@2x = 256、256@2x = 512），生成一次后复制
sips -z 16 16 "$work/icon-1024.png" --out "$work/Gloss.iconset/icon_16x16.png" >/dev/null
sips -z 32 32 "$work/icon-1024.png" --out "$work/Gloss.iconset/icon_16x16@2x.png" >/dev/null
sips -z 32 32 "$work/icon-1024.png" --out "$work/Gloss.iconset/icon_32x32.png" >/dev/null
sips -z 64 64 "$work/icon-1024.png" --out "$work/Gloss.iconset/icon_32x32@2x.png" >/dev/null
sips -z 128 128 "$work/icon-1024.png" --out "$work/Gloss.iconset/icon_128x128.png" >/dev/null
sips -z 256 256 "$work/icon-1024.png" --out "$work/Gloss.iconset/icon_128x128@2x.png" >/dev/null
cp "$work/Gloss.iconset/icon_128x128@2x.png" "$work/Gloss.iconset/icon_256x256.png"
sips -z 512 512 "$work/icon-1024.png" --out "$work/Gloss.iconset/icon_256x256@2x.png" >/dev/null
cp "$work/Gloss.iconset/icon_256x256@2x.png" "$work/Gloss.iconset/icon_512x512.png"
sips -z 1024 1024 "$work/icon-1024.png" --out "$work/Gloss.iconset/icon_512x512@2x.png" >/dev/null

iconutil -c icns "$work/Gloss.iconset" -o "$icons/Gloss.icns"
echo "✓ ${icons}/Gloss.icns"

# Dock 图标：Retina Dock 上限 128pt@2x = 256px，再大只是增大二进制体积
sips -z 256 256 "$work/icon-1024.png" --out "$icons/gloss-dock-icon.png" >/dev/null
echo "✓ ${icons}/gloss-dock-icon.png"
