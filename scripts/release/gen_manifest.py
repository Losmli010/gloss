#!/usr/bin/env python3
"""生成站点部署用的更新清单（输出到标准输出，日志走标准错误）。
版本单点复用发布校验脚本：标签格式与清单版本一致性校验失败即失败；
体积与散列从产物文件实测计算，产物缺文件即失败，不得发出残缺清单。
用法：python3 scripts/release/gen_manifest.py <tag> <artifacts-dir>，
  产物目录按发布矩阵交产物：系统（macOS）双架构各要压缩包与镜像，
  系统（Windows）只要压缩包。
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import sys
from typing import Any

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # noqa: E402 —— 同目录版本单点

import check_release_tag  # noqa: E402 —— 路径引导先于导入

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
BASE_URL = "https://losmli010.github.io/gloss/latest"
# 发布矩阵：压缩包（zip）全平台必交，镜像（dmg）仅系统（macOS）平台携带——
# 系统（Windows）的便携压缩包即最终产物，条目不带安装器字段
PLATFORM_EXTS = {
    "aarch64-apple-darwin": ("zip", "dmg"),
    "x86_64-apple-darwin": ("zip", "dmg"),
    "x86_64-pc-windows-msvc": ("zip",),
}
TARGETS = tuple(PLATFORM_EXTS)


def entry_for(artdir: str, version: str, target: str) -> dict[str, int | str]:
    """单平台条目：压缩包（zip）取体积与散列（size、sha256）为客户端校验对象，镜像（dmg）只出直链供站点使用，仅系统（macOS）平台携带。"""
    stem = "gloss-%s-%s" % (version, target)
    zip_path = os.path.join(artdir, "%s.zip" % stem)
    size = os.path.getsize(zip_path)
    with open(zip_path, "rb") as fh:
        sha = hashlib.sha256(fh.read()).hexdigest()
    if not re.fullmatch(r"[0-9a-f]{64}", sha):
        raise SystemExit("error: unexpected sha256 result: %s" % sha)
    entry: dict[str, int | str] = {
        "url": "%s/%s.zip" % (BASE_URL, stem),
        "size": size,
        "sha256": sha,
    }
    if "dmg" in PLATFORM_EXTS[target]:
        entry["dmg_url"] = "%s/%s.dmg" % (BASE_URL, stem)
    return entry


def run(tag: str, artdir: str, root: str) -> dict[str, Any]:
    """返回清单字典（manifest dict）；版本校验与产物齐备性失败时抛系统退出异常（SystemExit），消息进标准错误（stderr）。"""
    version, cargo_version, error = check_release_tag.run(tag, root)
    # 版本单点校验的判定输出走标准错误，保持标准输出只含 JSON 数据格式
    if error:
        raise SystemExit(error)
    print("tag version:   %s" % version, file=sys.stderr)
    print("cargo version: %s" % cargo_version, file=sys.stderr)
    print("ok: versions match", file=sys.stderr)

    # 产物缺文件当场失败，不得发出残缺清单；齐备口径按平台取自发布矩阵
    for target in TARGETS:
        for ext in PLATFORM_EXTS[target]:
            path = os.path.join(artdir, "gloss-%s-%s.%s" % (version, target, ext))
            if not os.path.isfile(path):
                raise SystemExit("error: missing artifact file %s" % path)

    return {
        "schema": 1,
        "version": version,
        "channels": {"stable": {target: entry_for(artdir, version, target) for target in TARGETS}},
    }


def main() -> int:
    if len(sys.argv) != 3:
        print(
            "usage: %s <tag> <artifacts-dir> (e.g. %s v0.1.0 artifacts)"
            % (sys.argv[0], sys.argv[0]),
            file=sys.stderr,
        )
        return 2
    manifest = run(sys.argv[1], sys.argv[2], ROOT)
    print(json.dumps(manifest, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
