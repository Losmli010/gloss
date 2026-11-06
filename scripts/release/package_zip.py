#!/usr/bin/env python3
"""把发布构建的可执行文件压成便携压缩包（zip）。
用法：python3 scripts/release/package_zip.py <gloss.exe> <output.zip>
可执行文件与同目录旁置的动态库（dll）压进压缩包根；自更新按可执行文件名
在解包现场定位载荷，旁置文件随行换装（载荷结构契约见更新模块的共享核心）。
"""

from __future__ import annotations

import os
import sys
import zipfile


def payload_files(exe: str) -> list[str]:
    """载荷清单：可执行文件本体加同目录旁置动态库（dll），按文件名排序保证清单稳定。"""
    directory = os.path.dirname(os.path.abspath(exe))
    exe_name = os.path.basename(exe)
    names = [exe_name]
    names.extend(name for name in sorted(os.listdir(directory)) if name.lower().endswith(".dll"))
    return [os.path.join(directory, name) for name in names]


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: %s <path/to/gloss.exe> <output.zip>" % sys.argv[0], file=sys.stderr)
        return 2
    exe, out = sys.argv[1], sys.argv[2]
    if not os.path.isfile(exe):
        print(
            "error: executable not found: %s (run cargo build --release --target <triple> first)"
            % exe,
            file=sys.stderr,
        )
        return 1
    files = payload_files(exe)
    for path in files:
        if not os.path.isfile(path):
            print("error: payload file missing: %s" % path, file=sys.stderr)
            return 1
    os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
    # 覆盖同名旧产物；压缩（deflate）与解压端（更新模块）的能力面对齐
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as bundle:
        for path in files:
            bundle.write(path, arcname=os.path.basename(path))
    print("ok: %s (%s)" % (out, ", ".join(os.path.basename(p) for p in files)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
