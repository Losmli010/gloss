#!/usr/bin/env python3
# 把打包好的 .app 压成 .dmg（UDZO 压缩镜像，Finder 打开即拖装）。
# 用法：./scripts/release/bundle_dmg.py <path/to/Gloss.app> [输出.dmg 路径]

import os
import subprocess
import sys


def main():
    if len(sys.argv) < 2:
        print("用法: %s <path/to/Gloss.app> [输出.dmg 路径]" % sys.argv[0], file=sys.stderr)
        return 2

    app = sys.argv[1]
    if not os.path.isdir(app):
        print(
            "错误：找不到 .app：%s（先跑 cargo bundle --release --format osx）" % app,
            file=sys.stderr,
        )
        return 1

    # 默认与 .app 同目录同名
    out = (
        sys.argv[2]
        if len(sys.argv) > 2
        else os.path.join(
            os.path.dirname(app),
            os.path.basename(app[:-4] if app.endswith(".app") else app) + ".dmg",
        )
    )
    os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)

    # -ov 覆盖同名旧镜像；UDZO 是只读压缩格式，发布产物的标准选择
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
    print("✓ 已生成 %s" % out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
