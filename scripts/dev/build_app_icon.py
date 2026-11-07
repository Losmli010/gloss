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
纯标准库实现——小档位走容器（ICO）的传统位图（BMP）编码（信息头加自底
向上像素加全零掩码，系统渲染兼容面以传统形态承载），大档位保留位图（PNG）
编码进容器，不引入额外工具链。
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

# 小档位改用传统位图（BMP）编码的上限：此下的档位走信息头加像素加掩码的
# 容器（ICO）传统形态，之上的档位保留位图（PNG）压缩
ICO_BMP_MAX_SIZE = 48

# 位图数据的署名（PNG），回读自检用于核对每个档位的编码形态
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"

BMP_INFO_HEADER_LEN = 40

# 位图（BMP）条目只认标准通道序（蓝绿红阿尔法），转换工具写出别的通道面
# 当场报错，坏产物不得静默进仓库
BMP_MASKS_BGRA = (0x00FF0000, 0x0000FF00, 0x000000FF, 0xFF000000)


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
    """组装图标集容器（ICO）：目录头加每档位一项，位图数据按档位取位图（PNG）或
    传统位图（BMP）编码——两种形态都在系统（Windows）的图标资源接受面内。"""
    entries = b""
    payload = b""
    offset = 6 + 16 * len(images)
    for size, image in images:
        # 宽高字节取值 0 即 256（容器格式的历史口径）
        entries += struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(image), offset)
        payload += image
        offset += len(image)
    return struct.pack("<HHH", 0, 1, len(images)) + entries + payload


def verify_ico(path: str, sizes: tuple[int, ...], bmp_sizes: tuple[int, ...] = ()) -> bool:
    """回读自检：目录计数、档位、平面数与位深、偏移推进及每档位的编码形态
    （小档位传统位图（BMP）几何、大档位位图（PNG）署名）逐一核对，组装写坏
    当场失败，坏产物不得静默进仓库。"""
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
        if size in bmp_sizes:
            if not bmp_entry_shape_ok(size, blob[start : start + length]):
                return False
        elif (
            length < len(PNG_SIGNATURE) or blob[start : start + len(PNG_SIGNATURE)] != PNG_SIGNATURE
        ):
            return False
        offset += length
    return offset == len(blob)


def bmp_entry_shape_ok(size: int, payload: bytes) -> bool:
    """图标条目的传统位图（BMP）形态核对：四十字节信息头、无压缩、双高声明、
    像素与全零掩码占满载荷。"""
    if len(payload) < BMP_INFO_HEADER_LEN:
        return False
    header_size, width, height, planes, depth = struct.unpack_from("<IiiHH", payload, 0)
    compression = struct.unpack_from("<I", payload, 16)[0]
    if header_size != BMP_INFO_HEADER_LEN or (width, height) != (size, size * 2):
        return False
    if (planes, depth, compression) != (1, 32, 0):
        return False
    mask_row = ((size + 31) // 32) * 4
    expected = BMP_INFO_HEADER_LEN + size * size * 4 + mask_row * size
    if len(payload) != expected:
        return False
    return payload[BMP_INFO_HEADER_LEN + size * size * 4 :] == b"\x00" * (mask_row * size)


def ico_bmp_payload(size: int, blob: bytes) -> bytes:
    """从图像转换工具的位图（BMP）文件字节提取像素，合成图标集条目的传统
    位图（BMP）载荷（四十字节信息头、自底向上像素、全零掩码）。"""
    if len(blob) < 14 + BMP_INFO_HEADER_LEN or blob[:2] != b"BM":
        raise SystemExit("error: the converted icon is not a BMP file")
    header_size = struct.unpack_from("<I", blob, 14)[0]
    width, height, planes, bpp = struct.unpack_from("<iiHH", blob, 18)
    compression = struct.unpack_from("<I", blob, 30)[0]
    if header_size < BMP_INFO_HEADER_LEN or (width, planes, bpp, abs(height)) != (
        size,
        1,
        32,
        size,
    ):
        raise SystemExit(
            "error: unexpected BMP geometry %dx%dx%d (header %d)"
            % (width, height, bpp, header_size)
        )
    if compression == 3:
        masks = struct.unpack_from("<IIII", blob, 14 + BMP_INFO_HEADER_LEN)
        if masks != BMP_MASKS_BGRA:
            raise SystemExit(
                "error: unexpected BMP channel masks %s"
                % (", ".join("%08x" % mask for mask in masks))
            )
    elif compression != 0:
        raise SystemExit("error: unexpected BMP compression %d" % compression)
    pixels_at = struct.unpack_from("<I", blob, 10)[0]
    row_bytes = width * 4
    if len(blob) < pixels_at + row_bytes * size:
        raise SystemExit("error: truncated BMP pixel data")
    rows = [
        blob[pixels_at + at * row_bytes : pixels_at + (at + 1) * row_bytes] for at in range(size)
    ]
    if height < 0:
        # 顶行序存储翻成容器条目的自底向上
        rows.reverse()
    if not any(row[at] for row in rows for at in range(3, row_bytes, 4)):
        raise SystemExit("error: the converted icon carries no alpha at all")
    mask_row = b"\x00" * (((size + 31) // 32) * 4)
    header = struct.pack(
        "<IiiHHIIiiII",
        BMP_INFO_HEADER_LEN,
        size,
        size * 2,
        1,
        32,
        0,
        row_bytes * size + len(mask_row) * size,
        0,
        0,
        0,
        0,
    )
    return header + b"".join(rows) + mask_row * size


def bmp_entry(size: int, png: str) -> bytes:
    """把单档位（PNG）转成图标集条目的传统位图（BMP）载荷：图像转换工具出
    位图（BMP），纯标准库解析像素并重组为容器（ICO）的传统形态。"""
    bmp = png + ".bmp"
    sips("-s", "format", "bmp", png, "--out", bmp)
    with open(bmp, "rb") as fh:
        return ico_bmp_payload(size, fh.read())


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

        # 图标集（Windows）：同一主图降采样七档，小档位转传统位图（BMP）编码、
        # 大档位保留位图（PNG）编码进容器（ICO），产物进仓库与打包图标同规，
        # 消费面是系统可执行文件资源与后置安装器
        bmp_sizes = tuple(size for size in ICO_SIZES if size <= ICO_BMP_MAX_SIZE)
        images: list[tuple[int, bytes]] = []
        for size in ICO_SIZES:
            sized = os.path.join(work, "icon-%d.png" % size)
            sips("-z", str(size), str(size), icon1024, "--out", sized)
            if size in bmp_sizes:
                images.append((size, bmp_entry(size, sized)))
            else:
                with open(sized, "rb") as fh:
                    images.append((size, fh.read()))
        ico_path = os.path.join(ICONS, "Gloss.ico")
        with open(ico_path, "wb") as fh:
            fh.write(pack_ico(images))
        if not verify_ico(ico_path, ICO_SIZES, bmp_sizes):
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
