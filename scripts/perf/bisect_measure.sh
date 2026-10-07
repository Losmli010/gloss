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
# （首帧预算 + RSS 尾段净增长），clone=分配计数门禁。各 gate 的预算常量
# 在各自源码里；预算常量被改动的提交本身会被判成坏——二分区间起点选在
# 预算变更之前，或临时对齐两端的预算口径。
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
    echo "未知 gate: $GATE（可选 startup|show|clone）" >&2
    exit 2
    ;;
esac

if ! "${CMD[@]}"; then
  echo "首跑失败——复跑一次确认（偶发失败先复跑）" >&2
  "${CMD[@]}"
fi
