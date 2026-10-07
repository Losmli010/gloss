#!/usr/bin/env bash
# 二分测量驱动：git bisect run 的包装，做两件测量纪律的事——
# 负载先检（超阈值 exit 125 让 bisect 跳过该提交而不是误判好坏），
# 偶发失败复跑一次定性（首跑挂、复跑过 = 负载波动，不算坏）。
#
# 用法（配合 git bisect）：
#   git bisect start HEAD <good-commit>
#   git bisect run scripts/perf/bisect_measure.sh startup   # 或 show / clone
#   git bisect reset
#
# gate 对照：startup=启动预算自检（m5_ready 预算），show=显隐预算自检
# （首帧预算 + RSS 尾段净增长 + 窗口句柄数），clone=分配计数门禁（判据在
# git 跟踪的 clone-stats.json 基线，不在源码常量）。预算常量/基线文件的
# 变更会改变判据口径：收紧可能把两阈值之间的实测判成坏，放宽会掩盖既有
# 回归——二分区间起点选在口径变更之前，或临时对齐两端口径。
set -euo pipefail

GATE="${1:-}"
if [ -z "$GATE" ]; then
  echo "用法: bisect_measure.sh <startup|show|clone>" >&2
  exit 2
fi

LOAD="$(sysctl -n vm.loadavg | awk '{print $2}')"
if awk -v l="$LOAD" 'BEGIN { exit !(l > 10) }'; then
  echo "1 分钟负载 $LOAD > 10，测量口径失真——等机器空下来再二分（exit 125 跳过本提交）" >&2
  exit 125
fi

case "$GATE" in
  startup) CMD=(just startup-selftest) ;;
  show) CMD=(just selftest) ;;
  clone) CMD=(just clone-check) ;;
  *)
    echo "未知 gate: ${GATE}（可选 startup|show|clone）" >&2
    exit 2
    ;;
esac

if ! "${CMD[@]}"; then
  echo "首跑失败——复跑一次确认（偶发失败先复跑）" >&2
  "${CMD[@]}"
fi
