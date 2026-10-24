#!/usr/bin/env python3
# 从 assets/icons 矢量源重建图标产物（换 Logo 方案或调色后跑一次）：
#   assets/icons/Gloss.icns           打包用 —— cargo-bundle 的 icon 配置指向它
#   assets/icons/gloss-dock-icon.png  开发期 Dock 图标 —— 非 bundle 运行时由
#                                     gloss-platform 内嵌（include_bytes!）交给 NSApplication
#   web/assets/favicon.png            浏览器标签页图标（32px），SVG favicon 的 PNG 兜底
#   web/assets/apple-touch-icon.png   Safari 触摸/收藏图标（180px）
# .icns 与 Dock PNG 源自 macOS 规范画布稿 gloss-app-icon.svg；web 两张 PNG 源自
# 满幅主标稿 gloss-logo.svg（站点 logo.svg 拷贝仍按 assets/icons/README.md 手工同步）。
# 只用系统自带的 sips（SVG→PNG、缩放）与 iconutil（iconset→.icns）。

import os
import shutil
import subprocess
import sys
import tempfile

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
ICONS = os.path.join(ROOT, "assets", "icons")


def sips(*args):
    subprocess.run(["sips", *args], check=True, stdout=subprocess.DEVNULL)


def raster1024(src, dst):
    """光栅化尺寸取决于 SVG 声明的 width/height 属性：丢了它 sips 会按内嵌尺寸出小图，
    下面的档位会把小图上采样成模糊图标静默进仓库，这里当场拦下。"""
    sips("-s", "format", "png", src, "--out", dst)
    width = int(
        subprocess.run(
            ["sips", "-g", "pixelWidth", dst], capture_output=True, text=True, check=True
        ).stdout.split()[-1]
    )
    height = int(
        subprocess.run(
            ["sips", "-g", "pixelHeight", dst], capture_output=True, text=True, check=True
        ).stdout.split()[-1]
    )
    if width != 1024 or height != 1024:
        print(
            "错误：%s 光栅化后为 %d×%d，期望 1024×1024——SVG 的 width/height 属性不能省"
            % (os.path.basename(src), width, height),
            file=sys.stderr,
        )
        return False
    return True


def main():
    for tool in ("sips", "iconutil"):
        if not shutil.which(tool):
            print("错误：缺少系统工具 %s（图标产物只能在 macOS 上重建）" % tool, file=sys.stderr)
            return 1

    with tempfile.TemporaryDirectory() as work:
        iconset = os.path.join(work, "Gloss.iconset")
        os.makedirs(iconset)

        # 1024 主图：icns 全部档位与 Dock 图标都由它降采样而来
        if not raster1024(
            os.path.join(ICONS, "gloss-app-icon.svg"), os.path.join(work, "icon-1024.png")
        ):
            return 1

        # iconset 的十个档位（基础的 1x 与 @2x 各五档，iconutil 认这套文件名）；
        # 256 与 512 档在表里各出现两次（128@2x = 256、256@2x = 512），生成一次后复制
        icon1024 = os.path.join(work, "icon-1024.png")
        sips("-z", "16", "16", icon1024, "--out", os.path.join(iconset, "icon_16x16.png"))
        sips("-z", "32", "32", icon1024, "--out", os.path.join(iconset, "icon_16x16@2x.png"))
        sips("-z", "32", "32", icon1024, "--out", os.path.join(iconset, "icon_32x32.png"))
        sips("-z", "64", "64", icon1024, "--out", os.path.join(iconset, "icon_32x32@2x.png"))
        sips("-z", "128", "128", icon1024, "--out", os.path.join(iconset, "icon_128x128.png"))
        sips("-z", "256", "256", icon1024, "--out", os.path.join(iconset, "icon_128x128@2x.png"))
        shutil.copyfile(
            os.path.join(iconset, "icon_128x128@2x.png"), os.path.join(iconset, "icon_256x256.png")
        )
        sips("-z", "512", "512", icon1024, "--out", os.path.join(iconset, "icon_256x256@2x.png"))
        shutil.copyfile(
            os.path.join(iconset, "icon_256x256@2x.png"), os.path.join(iconset, "icon_512x512.png")
        )
        sips("-z", "1024", "1024", icon1024, "--out", os.path.join(iconset, "icon_512x512@2x.png"))

        subprocess.run(
            ["iconutil", "-c", "icns", iconset, "-o", os.path.join(ICONS, "Gloss.icns")], check=True
        )
        print("✓ %s" % os.path.join(ICONS, "Gloss.icns"))

        # Dock 图标：Retina 上限 128pt@2x = 256px。
        sips("-z", "256", "256", icon1024, "--out", os.path.join(ICONS, "gloss-dock-icon.png"))
        print("✓ %s" % os.path.join(ICONS, "gloss-dock-icon.png"))

        # web 站点图标：主标满幅稿降采样为标签页图标与触摸图标（index.html 的 <link> 引用）
        if not raster1024(
            os.path.join(ICONS, "gloss-logo.svg"), os.path.join(work, "logo-1024.png")
        ):
            return 1
        logo1024 = os.path.join(work, "logo-1024.png")
        sips(
            "-z", "32", "32", logo1024, "--out", os.path.join(ROOT, "web", "assets", "favicon.png")
        )
        sips(
            "-z",
            "180",
            "180",
            logo1024,
            "--out",
            os.path.join(ROOT, "web", "assets", "apple-touch-icon.png"),
        )
        print("✓ %s" % os.path.join(ROOT, "web", "assets", "favicon.png"))
        print("✓ %s" % os.path.join(ROOT, "web", "assets", "apple-touch-icon.png"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
