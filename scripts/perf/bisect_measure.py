#!/usr/bin/env python3
# 二分测量驱动：版本二分的包装，做两件测量纪律的事——
# 负载先检（超阈值以专用跳过码退出，让二分跳过该提交而不是误判好坏），
# 偶发失败复跑一次定性（首跑挂、复跑过 = 负载波动，不算坏）。
#
# 用法（配合版本二分）：起二分、以本脚本为驱动跑、收尾复位。
#
# 三种门禁对照：启动预算自检（就绪里程碑预算）、显隐预算自检
# （首帧预算 + RSS 尾段净增长 + 窗口句柄数）、分配计数门禁（判据在
# 仓库跟踪的历史数据最新点的克隆值，不在源码常量）。预算常量/基线
# 数据的变更会改变判据口径：收紧可能把两阈值之间的实测判成坏，放宽会掩盖
# 既有回归——二分区间起点选在口径变更之前，或临时对齐两端口径。

import subprocess
import sys

GATES = {
    "startup": "startup-selftest",
    "show": "selftest",
    "clone": "clone-check",
}


def main():
    gate = sys.argv[1] if len(sys.argv) > 1 else ""
    if gate not in GATES:
        print("用法: bisect_measure.py <startup|show|clone>", file=sys.stderr)
        return 2

    loadavg = subprocess.run(
        ["sysctl", "-n", "vm.loadavg"], capture_output=True, text=True, check=True
    ).stdout.split()[1]
    load = float(loadavg)
    if load > 10:
        print(
            "1 分钟负载 %s > 10，测量口径失真——等机器空下来再二分（exit 125 跳过本提交）" % loadavg,
            file=sys.stderr,
        )
        return 125

    cmd = ["just", GATES[gate]]
    if subprocess.run(cmd).returncode != 0:
        print("首跑失败——复跑一次确认（偶发失败先复跑）", file=sys.stderr)
        return subprocess.run(cmd).returncode
    return 0


if __name__ == "__main__":
    sys.exit(main())
