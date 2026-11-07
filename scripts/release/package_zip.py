#!/usr/bin/env python3
"""把发布构建的可执行文件压成便携压缩包（zip）。
用法：python3 scripts/release/package_zip.py <gloss.exe> <output.zip> [--allow-dll <name>]...
可执行文件与同目录旁置的动态库（dll）压进压缩包根；旁置动态库按显式放行
名单收口（--allow-dll），名单可重复、缺省为空；名单外的动态库与放行而缺位
的动态库都显式报错不打包——构建目录里的杂散动态库会随自更新换装落到用户
安装位。自更新按可执行文件名在解包现场定位载荷，旁置文件随行换装（载荷结
构契约见更新模块的共享核心）。
"""

from __future__ import annotations

import os
import sys
import zipfile


def payload_files(exe: str, allowed_dlls: list[str]) -> list[str]:
    """载荷清单：可执行文件本体加放行名单内的同目录旁置动态库（dll），按文件名排序保证清单稳定。"""
    directory = os.path.dirname(os.path.abspath(exe))
    exe_name = os.path.basename(exe)
    allowed = {name.lower() for name in allowed_dlls}
    on_disk = {
        name.lower(): name
        for name in sorted(os.listdir(directory))
        if name.lower().endswith(".dll")
    }
    stray = sorted(name for lower, name in on_disk.items() if lower not in allowed)
    if stray:
        raise SystemExit(
            "error: unexpected dll(s) beside the executable: %s"
            " (allow-list them with --allow-dll or remove them from the"
            " build directory)" % ", ".join(stray)
        )
    missing = sorted(lower for lower in allowed if lower not in on_disk)
    if missing:
        raise SystemExit(
            "error: allowed dll(s) not found beside the executable: %s" % ", ".join(missing)
        )
    names = [exe_name]
    names.extend(on_disk[lower] for lower in sorted(allowed))
    return [os.path.join(directory, name) for name in names]


def main() -> int:
    argv = sys.argv[1:]
    allowed: list[str] = []
    rest: list[str] = []
    while argv:
        arg = argv.pop(0)
        if arg == "--allow-dll":
            if not argv:
                print("error: --allow-dll needs a value", file=sys.stderr)
                return 2
            allowed.append(argv.pop(0))
        else:
            rest.append(arg)
    if len(rest) != 2:
        print(
            "usage: %s <path/to/gloss.exe> <output.zip> [--allow-dll <name>]..." % sys.argv[0],
            file=sys.stderr,
        )
        return 2
    exe, out = rest
    if not os.path.isfile(exe):
        print(
            "error: executable not found: %s (run cargo build --release --target <triple> first)"
            % exe,
            file=sys.stderr,
        )
        return 1
    try:
        files = payload_files(exe, allowed)
    except SystemExit as failure:
        print(failure.code, file=sys.stderr)
        return 1
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
