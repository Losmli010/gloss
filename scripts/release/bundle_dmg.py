#!/usr/bin/env python3
"""把打包好的应用包压成分发镜像（只读压缩格式，访达打开即拖装）。
用法：以解释器运行，首参为应用包路径，次参可选为输出镜像路径。
"""

import os
import subprocess
import sys


def main():
    if len(sys.argv) < 2:
        print("usage: %s <path/to/Gloss.app> [output.dmg path]" % sys.argv[0], file=sys.stderr)
        return 2

    app = sys.argv[1]
    if not os.path.isdir(app):
        print(
            "error: .app not found: %s (run cargo bundle --release --format osx first)" % app,
            file=sys.stderr,
        )
        return 1

    # 默认与应用包同目录同名
    out = (
        sys.argv[2]
        if len(sys.argv) > 2
        else os.path.join(
            os.path.dirname(app),
            os.path.basename(app[:-4] if app.endswith(".app") else app) + ".dmg",
        )
    )
    os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)

    # 覆盖同名旧镜像；只读压缩是发布产物的标准选择
    subprocess.run(
        [
            "hdiutil",
            "create",
            "-volname",
            "Gloss",
            "-srcfolder",
            app,
            "-ov",
            "-format",
            "UDZO",
            out,
        ],
        check=True,
    )
    print("ok: generated %s" % out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
