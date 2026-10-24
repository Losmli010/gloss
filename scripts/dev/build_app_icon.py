#!/usr/bin/env python3
# 从图标目录的矢量源重建图标产物（换标方案或调色后跑一次）：
#   打包用图标           —— 打包配置指向它
#   开发期 Dock 图标     —— 非打包运行时由平台适配层内嵌交给系统
#   站点标签页图标       —— 三十二像素，矢量站标的位图兜底
#   站点触摸/收藏图标    —— 一百八十像素
# 打包图标与 Dock 图标源自规范画布稿，站点两张位图源自满幅主标稿
# （站点标稿拷贝仍按图标目录说明手工同步）。
# 只用系统自带的图像转换与图标集工具。

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
    """光栅化尺寸取决于矢量图声明的宽高属性：丢了它转换工具会按内嵌尺寸出小图，
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

        # 一千零二十四主图：打包图标全部档位与 Dock 图标都由它降采样而来
        if not raster1024(
            os.path.join(ICONS, "gloss-app-icon.svg"), os.path.join(work, "icon-1024.png")
        ):
            return 1

        # 图标集的十个档位（一倍与二倍各五档，图标集工具认这套文件名）；
        # 256 与 512 档在表里各出现两次（中间档的二倍值），生成一次后复制
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

        # Dock 图标：视网膜屏上限为二百五十六像素。
        sips("-z", "256", "256", icon1024, "--out", os.path.join(ICONS, "gloss-dock-icon.png"))
        print("✓ %s" % os.path.join(ICONS, "gloss-dock-icon.png"))

        # 站点图标：主标满幅稿降采样为标签页图标与触摸图标（站点页面的链接引用）
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
