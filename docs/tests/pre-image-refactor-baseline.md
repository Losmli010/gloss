# 架构保持重构性能基线（阶段 0）

面向图像里程碑的架构保持重构计划阶段 0 落盘的性能基线：后续各阶段收尾按同一口径重跑基准，与本表逐点比对确认无回退后才进下一阶段。基线对应主分支提交 `a12ae47`，采集日期 2026-10-30，全部数字为当日本机实测，无外推。

## 回归阈值（停工线）

任一基准组内任一基准点的中位数相对本表基线值回退超过 `5%`，即判该组回退：停工排查，不堆叠后续阶段。判定前先复跑三次取中位数，单次波动不作数；复跑仍超阈值才算回退。

零分配点单独判定：克隆两组的 `i18n/for_error` 与 `i18n/for_error_detail` 共四个点在基线实测每次调用零分配，基准框架对零值测量拒绝建立分布（六次运行日志均报 `zero time per iteration`），不产出中位数——这些点的基线值为 0，判定收紧为出现任何非零分配即触发停工排查。

## 采集口径

- 三次取中位数：每个基准组独立跑三次；每次取基准框架报告的中位数估计（`estimates.json` 的 `median.point_estimate`），三次中再取中位数入表为基线值，三次原始值取整列表备查。
- 负载纪律：六次基准运行串行执行，期间无并行编译或其它重负载；时延组三次间的散布实测最高约 13%（`complete` 组，第二次运行遭瞬时负载抬升），中位数口径吸收该波动，故判定必须以三次中位数为准。
- 命名基线留痕：三次运行分别以 `refactor0-run1`、`refactor0-run2`、`refactor0-run3` 存入基准框架产物目录（构建共享目录 `gloss-shared-target` 下的 `criterion`，不进版本库）；共享产物目录由本机构建配置指向（仓库外 `gloss-worktrees/.cargo/config.toml` 的构建目标目录，或等价的 `CARGO_TARGET_DIR`），重跑比对须在应用该配置的检出下执行，否则产物落主仓 `target/`、与本表命名基线失去快速对照；本表是长期事实源，产物目录仅供本机快速对照。

## 环境与工具链

- 处理器 `Intel Core i5-7360U @ 2.30GHz`，内存 8 吉字节，系统（macOS）13.7.8，第三层自检与基准的图形栈走金属后端（Metal）的集成显卡 `Intel Iris Plus Graphics 640`。
- 工具链 `rustc 1.96.1`，基准框架（criterion）`0.8.2`，采样按默认配置（样本 100、预热 3 秒、测量 5 秒）。
- 采集命令：`cargo bench --bench core -- --save-baseline refactor0-runN` 与 `cargo bench --bench clone -- --save-baseline refactor0-runN`，`N` 依次取 1、2、3。

## 基准基线表

时延四组单位为纳秒，克隆两组分别以分配次数与分配字节计量（三次散布为零，分配计数天然稳定）；表列依次为三次运行的逐点中位数与三次取整后的基线中位数。

### 缓存键

本组四个点，单位纳秒。

| 基准点 | 第一次 | 第二次 | 第三次 | 基线中位数 |
| --- | --- | --- | --- | --- |
| `cache_key/text/short` | 160 | 166 | 158 | 160 |
| `cache_key/text/long` | 1748 | 1741 | 1730 | 1741 |
| `cache_key/image/1mb` | 246176 | 244867 | 243426 | 244867 |
| `cache_key/image/4mb` | 1010473 | 1010663 | 1009932 | 1010473 |

### 结果完成

本组四个点，单位纳秒；三次间散布为全表最高（约 13%，见采集口径节）。

| 基准点 | 第一次 | 第二次 | 第三次 | 基线中位数 |
| --- | --- | --- | --- | --- |
| `complete/word_card/ok` | 2499 | 2755 | 2432 | 2499 |
| `complete/word_card/many_examples` | 8515 | 9209 | 8563 | 8563 |
| `complete/long_note/ok` | 1462 | 1575 | 1460 | 1462 |
| `complete/prose/degraded` | 106 | 106 | 106 | 106 |

### 提示渲染

本组两个点，单位纳秒。

| 基准点 | 第一次 | 第二次 | 第三次 | 基线中位数 |
| --- | --- | --- | --- | --- |
| `prompt_render/word/short` | 10446 | 10472 | 10486 | 10472 |
| `prompt_render/code/long` | 8367 | 8255 | 8335 | 8335 |

### 任务缓存

本组四个点，单位纳秒。

| 基准点 | 第一次 | 第二次 | 第三次 | 基线中位数 |
| --- | --- | --- | --- | --- |
| `task_cache/get/hit` | 573 | 594 | 584 | 584 |
| `task_cache/get/miss` | 125 | 125 | 125 | 125 |
| `task_cache/set/same_key` | 2149 | 2171 | 2153 | 2153 |
| `task_cache/set/new_key` | 1959 | 1964 | 1958 | 1959 |

### 克隆分配次数

本组六个点，单位为次；值为 0 的两个点是零分配点，判定规则见回归阈值节。

| 基准点 | 第一次 | 第二次 | 第三次 | 基线中位数 |
| --- | --- | --- | --- | --- |
| `clone_allocs/machine/lifecycle` | 7 | 7 | 7 | 7 |
| `clone_allocs/i18n/for_error` | 0 | 0 | 0 | 0 |
| `clone_allocs/i18n/for_error_fill` | 1 | 1 | 1 | 1 |
| `clone_allocs/i18n/for_error_detail` | 0 | 0 | 0 | 0 |
| `clone_allocs/ui/settings_frame` | 224 | 224 | 224 | 224 |
| `clone_allocs/ui/popup_frame` | 204 | 204 | 204 | 204 |

### 克隆分配字节

本组六个点，单位为字节。

| 基准点 | 第一次 | 第二次 | 第三次 | 基线中位数 |
| --- | --- | --- | --- | --- |
| `clone_bytes/machine/lifecycle` | 317 | 317 | 317 | 317 |
| `clone_bytes/i18n/for_error` | 0 | 0 | 0 | 0 |
| `clone_bytes/i18n/for_error_fill` | 53 | 53 | 53 | 53 |
| `clone_bytes/i18n/for_error_detail` | 0 | 0 | 0 | 0 |
| `clone_bytes/ui/settings_frame` | 56435 | 56435 | 56435 | 56435 |
| `clone_bytes/ui/popup_frame` | 78056 | 78056 | 78056 | 78056 |

## 测试套件时长

`cargo test --workspace --all-features`（`just test` 同款）在预构建完成后计时：墙钟 140 秒（`/usr/bin/time -p` 报 `real 139.97` 秒），全目标零失败。

## 门禁留档

`just check` 于 2026-10-30 在基线提交 `a12ae47` 上运行，退出码 0，末行 `ok: all quality gates passed`：文本六门禁与各格式检查全过，约束门禁 17 项全过（行数上限零豁免），静态检查（Clippy）严格模式零告警，检查器单测 687 项全过，全量测试零失败——其中第三层显隐自检跑满一百轮且判定 `pass`，启动预算自检以 1127 毫秒完成（预算 4000 毫秒）。完整输出随本表入库提交的拉取请求描述留档。
