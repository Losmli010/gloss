# bench-summary.py 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

- 2026-09-21 用 python3 标准库而非 jq：机器与 CI 都没有 jq 依赖，macOS（CLT）与 GitHub macOS runner 自带 python3；estimates.json 嵌套较深，bash+awk 解析过脆。
- 2026-09-21 基准名的来历：criterion 落盘目录是扁平的（ID 里的 `/` 被 make_filename_safe 替换为 `_`），展示名优先读 new/benchmark.json 的 full_id（保留原始 `/` 分隔），读不到再退回目录名。
- 2026-09-21 滚动基线 last 可作对照的缘由：criterion 先对照 baseline_directory 里的旧数据、跑完才把 new 复制过去（copy_new_dir_to_base 仅 Baseline::Save 时执行），所以 bench 每次对照的都是上一次的数据；--baseline（CompareStrict）不回写，命名锚点在多次 bench-check 间保持稳定。
- 2026-09-21 判定列口径：“回归/改善/无显著变化”只由 change/estimates.json 均值变化的 95% 置信区间是否跨零决定，未引入 criterion 的 noise threshold 口径；跨零即判“无显著变化”比 noise 口径更灵敏，精确取舍以区间数字为准、判定列只作速读。
- 2026-09-21 首次运行（或 cargo clean 后）没有 change/，变化两列输出“—”，不算错误。
- 2026-09-21 只读取不运行的缘由：对照对象取决于上一次怎么跑（bench 对照上一次运行、bench-check 对照命名基线），criterion 已把对照结果写进 change/，脚本不重复实现对照逻辑。
- 2026-10-05 移入 `scripts/perf/`（性能脚本归拢），行为不变；配方名
  bench-summary 不变。

## 文件头迁出的用法（2026-10-24，注释纪律）

- 用法：`scripts/perf/bench-summary.py [对照标签] [criterion 目录]`——首参对照
  标签（缺省“上一次运行”），次参 criterion 输出目录（缺省 `target/criterion`）。
- 对照对象由最近一次的运行方式决定：`just bench` 对照上一次运行，`just bench-check`
  对照命名基线；本脚本只读取数据，不运行基准。
- 输出：Markdown 表，每个基准一行（本次均值、95% 置信区间、相对对照的变化与判定）。
