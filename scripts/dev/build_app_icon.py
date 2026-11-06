#!/usr/bin/env python3
"""从图标目录的矢量源重建图标产物（换标方案或调色后跑一次）：
  打包用图标           —— 打包配置指向它
  打包用图标集（Windows）—— 系统可执行文件资源与后置安装器的图标来源
  开发期程序坞（Dock）图标 —— 非打包运行时由平台适配层内嵌交给系统
  站点标签页图标       —— 三十二像素，矢量站标的位图兜底
  站点触摸与收藏图标    —— 一百八十像素
打包图标与程序坞（Dock）图标源自规范画布稿，站点两张位图源自满幅主标稿，
站点标稿拷贝仍按图标目录说明手工同步。
只用系统自带的图像转换与图标集工具；图标集容器（ICO）的组装与回读自检为
纯标准库实现，图像数据保留位图（PNG）编码进容器，不引入额外工具链。
"""

from __future__ import annotations

import os
import shutil
import struct
import subprocess
import sys
import tempfile

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
ICONS = os.path.join(ROOT, "assets", "icons")

# 图标集容器（ICO）的档位：任务栏小图标到资源管理器大视图的常用覆盖面
ICO_SIZES = (16, 24, 32, 48, 64, 128, 256)

# 位图数据的署名（PNG），回读自检用于核对每个档位的编码形态
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"


def sips(*args: str) -> None:
    subprocess.run(["sips", *args], check=True, stdout=subprocess.DEVNULL)


def raster1024(src: str, dst: str) -> bool:
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
            "error: %s rasterized to %dx%d, expected 1024x1024 — SVG width/height attributes must not be omitted"
            % (os.path.basename(src), width, height),
            file=sys.stderr,
        )
        return False
    return True


def pack_ico(images: list[tuple[int, bytes]]) -> bytes:
    """组装图标集容器（ICO）：目录头加每档位一项，位图数据直接用位图（PNG）编码——
    系统（Windows）的图标资源接受该形态的压缩档位。"""
    entries = b""
    payload = b""
    offset = 6 + 16 * len(images)
    for size, png in images:
        # 宽高字节取值 0 即 256（容器格式的历史口径）
        entries += struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(png), offset)
        payload += png
        offset += len(png)
    return struct.pack("<HHH", 0, 1, len(images)) + entries + payload


def verify_ico(path: str, sizes: tuple[int, ...]) -> bool:
    """回读自检：目录计数、档位、平面数与位深、偏移推进及每档位的
    位图（PNG）署名逐一核对，组装写坏当场失败，坏产物不得静默进仓库。"""
    with open(path, "rb") as fh:
        blob = fh.read()
    count = struct.unpack_from("<H", blob, 4)[0]
    if count != len(sizes) or len(blob) < 6 + 16 * count:
        return False
    offset = 6 + 16 * count
    for index, size in enumerate(sizes):
        fields = struct.unpack_from("<BBBBHHII", blob, 6 + 16 * index)
        width, height, _colors, _reserved, planes, depth, length, start = fields
        if (width, height) != (size % 256, size % 256):
            return False
        if (planes, depth) != (1, 32) or start != offset:
            return False
        if length < len(PNG_SIGNATURE) or blob[start : start + len(PNG_SIGNATURE)] != PNG_SIGNATURE:
            return False
        offset += length
    return offset == len(blob)


def main() -> int:
    for tool in ("sips", "iconutil"):
        if not shutil.which(tool):
            print(
                "error: missing system tool %s (icon artifacts can only be rebuilt on macOS)"
                % tool,
                file=sys.stderr,
            )
            return 1

    with tempfile.TemporaryDirectory() as work:
        iconset = os.path.join(work, "Gloss.iconset")
        os.makedirs(iconset)

        # 一千零二十四主图：打包图标全部档位与程序坞（Dock）图标都由它降采样而来
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
        print("ok: %s" % os.path.join(ICONS, "Gloss.icns"))

        # 程序坞（Dock）图标：视网膜屏上限为二百五十六像素。
        sips("-z", "256", "256", icon1024, "--out", os.path.join(ICONS, "gloss-dock-icon.png"))
        print("ok: %s" % os.path.join(ICONS, "gloss-dock-icon.png"))

        # 图标集（Windows）：同一主图降采样七档，位图（PNG）编码数据直接进容器（ICO），
        # 产物进仓库与打包图标同规，消费面是系统可执行文件资源与后置安装器
        images: list[tuple[int, bytes]] = []
        for size in ICO_SIZES:
            sized = os.path.join(work, "icon-%d.png" % size)
            sips("-z", str(size), str(size), icon1024, "--out", sized)
            with open(sized, "rb") as fh:
                images.append((size, fh.read()))
        ico_path = os.path.join(ICONS, "Gloss.ico")
        with open(ico_path, "wb") as fh:
            fh.write(pack_ico(images))
        if not verify_ico(ico_path, ICO_SIZES):
            print("error: %s failed the self check" % ico_path, file=sys.stderr)
            return 1
        print("ok: %s" % ico_path)

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
        print("ok: %s" % os.path.join(ROOT, "web", "assets", "favicon.png"))
        print("ok: %s" % os.path.join(ROOT, "web", "assets", "apple-touch-icon.png"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
