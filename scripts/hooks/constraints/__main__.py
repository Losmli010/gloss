#!/usr/bin/env python3
"""约束门禁的命令入口：按路径直接执行本文件即运行约束包的聚合门禁，与按
模块方式调用本包等价；首参可选为仓库根，缺省按本文件位置向上回溯。
"""

import os
import sys

_HOOKS = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if _HOOKS not in sys.path:
    sys.path.insert(0, _HOOKS)

import constraints  # noqa: E402

if __name__ == "__main__":
    sys.exit(constraints.main())
