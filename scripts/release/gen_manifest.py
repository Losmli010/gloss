#!/usr/bin/env python3
"""生成站点部署用的更新清单（输出到标准输出，日志走标准错误）。
版本单点复用发布校验脚本：标签格式与清单版本一致性校验失败即失败；
体积与散列从产物文件实测计算，产物缺文件即失败，不得发出残缺清单。
用法：以解释器运行，首参为发布标签，次参为产物目录；
  产物目录内须有双架构的压缩包与镜像共四个文件。
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
TARGETS = ("aarch64-apple-darwin", "x86_64-apple-darwin")


def entry_for(artdir: str, version: str, target: str) -> dict[str, int | str]:
    """单架构条目：zip 取 size/sha256（客户端校验对象），dmg 只出直链供站点使用。"""
    stem = "gloss-%s-%s" % (version, target)
    zip_path = os.path.join(artdir, "%s.zip" % stem)
    size = os.path.getsize(zip_path)
    with open(zip_path, "rb") as fh:
        sha = hashlib.sha256(fh.read()).hexdigest()
    if not re.fullmatch(r"[0-9a-f]{64}", sha):
        raise SystemExit("error: unexpected sha256 result: %s" % sha)
    return {
        "url": "%s/%s.zip" % (BASE_URL, stem),
        "dmg_url": "%s/%s.dmg" % (BASE_URL, stem),
        "size": size,
        "sha256": sha,
    }


def run(tag: str, artdir: str, root: str) -> dict[str, Any]:
    """返回 manifest dict；版本校验与产物齐备性失败时抛 SystemExit（消息进 stderr）。"""
    version, cargo_version, error = check_release_tag.run(tag, root)
    # 版本单点校验的判定输出走标准错误，保持标准输出只含数据格式（JSON）
    if error:
        raise SystemExit(error)
    print("tag version:   %s" % version, file=sys.stderr)
    print("cargo version: %s" % cargo_version, file=sys.stderr)
    print("ok: versions match", file=sys.stderr)

    # 产物缺文件当场失败，不得发出残缺清单
    for target in TARGETS:
        for ext in ("zip", "dmg"):
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
