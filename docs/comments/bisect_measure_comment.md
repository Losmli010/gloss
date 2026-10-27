# bisect_measure.py 决策记录

- 2026-10-07 exit 125 = git bisect 的 skip 约定：负载超限时跳过该提交
  而不是误判好坏；偶发失败复跑一次定性（首跑挂复跑过 = good），两连挂
  以最后一次命令的退出码交回 bisect 判坏（cargo 101 / just 1 都 < 125）。
- 2026-10-07（PR #123 评审抓到的 HIGH）macOS 自带 bash 3.2 在 UTF-8
  locale 下把 `$GATE（` 的全角左括号并进变量名，set -u 直接 unbound
  variable 崩溃（exit 1）——gate 名打错时每个 bisect 步都会判坏，产出
  垃圾 first-bad-commit。教训：bash 3.2 兼容面里，变量后接全角标点必须
  花括号 `${GATE}（`。
- 2026-10-07 gate 判据口径差异：startup/show 的预算常量在源码（
  tests/startup.rs / tests/overlay.rs），clone 的判据在 git 跟踪的
  scripts/perf/baselines/clone-stats.json——口径变更提交影响二分区间
  选择的说明落在脚本头注释与计划文档附录。
