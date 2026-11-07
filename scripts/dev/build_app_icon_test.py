"""图标重建脚本的单测：图标集条目的传统位图（BMP）载荷合成与容器回读自检。

夹具用手工构造的位图（BMP）文件字节（顶行序与底行序、通道面异常、截断）
驱动合成纯函数，容器自检覆盖位图（BMP）档与位图（PNG）档两种形态。运行
方式沉记录文件。
"""

from __future__ import annotations

import struct
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).parent))

import build_app_icon  # noqa: E402


def bmp_file(size: int, top_down: bool) -> bytes:
    """构造图像转换工具形态的位图（BMP）文件：五版信息头（通道掩码在内）、标准通道序、逐像素渐变。"""
    pixels = b"".join(
        bytes((column % 256, (size - row) % 256, 0x40, 0xFF))
        for row in range(size)
        for column in range(size)
    )
    header = struct.pack(
        "<IiiHHIIiiII", 124, size, -size if top_down else size, 1, 32, 3, len(pixels), 0, 0, 0, 0
    )
    header += struct.pack("<IIII", *build_app_icon.BMP_MASKS_BGRA)
    header += b"\x00" * (124 - 56)
    file_header = b"BM" + struct.pack("<IHHI", 14 + 124 + len(pixels), 0, 0, 14 + 124)
    return file_header + header + pixels


def test_bmp_payload_synthesizes_canonical_entry() -> None:
    size = 16
    payload = build_app_icon.ico_bmp_payload(size, bmp_file(size, top_down=True))
    header_size, width, height, planes, depth = struct.unpack_from("<IiiHH", payload, 0)
    compression = struct.unpack_from("<I", payload, 16)[0]
    assert (header_size, width, height, planes, depth, compression) == (
        40,
        size,
        size * 2,
        1,
        32,
        0,
    )
    row_bytes = size * 4
    # 顶行序输入翻成自底向上：载荷首行是源图像的最后一行
    assert payload[40 : 40 + row_bytes] == b"".join(
        bytes((column % 256, (size - (size - 1)) % 256, 0x40, 0xFF)) for column in range(size)
    )
    mask_row = ((size + 31) // 32) * 4
    assert len(payload) == 40 + size * size * 4 + mask_row * size
    assert payload[40 + size * size * 4 :] == b"\x00" * (mask_row * size)


def test_bmp_payload_flips_bottom_up_input() -> None:
    size = 16
    payload = build_app_icon.ico_bmp_payload(size, bmp_file(size, top_down=False))
    row_bytes = size * 4
    assert payload[40 : 40 + row_bytes] == b"".join(
        bytes((column % 256, (size - 0) % 256, 0x40, 0xFF)) for column in range(size)
    )


def test_bmp_payload_rejects_wrong_channel_masks() -> None:
    blob = bytearray(bmp_file(16, top_down=True))
    struct.pack_into("<I", blob, 14 + 40, 0xFF000000)
    with pytest.raises(SystemExit, match="channel masks"):
        build_app_icon.ico_bmp_payload(16, bytes(blob))


def test_bmp_payload_rejects_truncated_pixels() -> None:
    blob = bmp_file(16, top_down=True)
    with pytest.raises(SystemExit, match="truncated"):
        build_app_icon.ico_bmp_payload(16, blob[:-8])


def test_bmp_payload_rejects_wrong_geometry() -> None:
    with pytest.raises(SystemExit, match="geometry"):
        build_app_icon.ico_bmp_payload(24, bmp_file(16, top_down=True))


def test_bmp_payload_rejects_alphaless_pixels() -> None:
    blob = bytearray(bmp_file(16, top_down=True))
    pixels_at = struct.unpack_from("<I", blob, 10)[0]
    for at in range(pixels_at + 3, len(blob), 4):
        blob[at] = 0
    with pytest.raises(SystemExit, match="alpha"):
        build_app_icon.ico_bmp_payload(16, bytes(blob))


def test_pack_and_verify_round_trip_with_both_encodings() -> None:
    size = 16
    bmp_payload = build_app_icon.ico_bmp_payload(size, bmp_file(size, top_down=True))
    png_payload = build_app_icon.PNG_SIGNATURE + b"rest-of-png"
    sizes = (size, 64)
    packed = build_app_icon.pack_ico([(size, bmp_payload), (64, png_payload)])
    path = Path(str(build_app_icon.ICONS) + "-test.ico")
    try:
        path.write_bytes(packed)
        assert build_app_icon.verify_ico(str(path), sizes, (size,))
        assert not build_app_icon.verify_ico(str(path), sizes, ())
        # 位图（BMP）档被改坏（信息头改成五版）时自检必须失败
        broken = bytearray(packed)
        struct.pack_into("<I", broken, 6 + 16 * len(sizes), 124)
        path.write_bytes(bytes(broken))
        assert not build_app_icon.verify_ico(str(path), sizes, (size,))
    finally:
        path.unlink(missing_ok=True)
