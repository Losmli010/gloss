# app-metrics.py 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## 文件头迁出的用法与口径（2026-10-24，注释纪律）

- 用法：`scripts/perf/app-metrics.py`（缺省 3 次、空闲窗口 30s）；`--runs N` 指定轮数、`--idle-secs N` 指定空闲窗口秒数。产物写 `target/perf/app-runtime.json`（v2 片段，本机 handoff 不入库，由 perf-collect.py 折算进趋势点）。
- 前置：`target/release/gloss` 已构建（`just app-metrics` 配方先构建）。
- 采集机制实名：`GLOSS_LOG_DIR` 指向独立临时目录 spawn release 二进制；轮询日志解析里程碑行（字段 `milestone` / `elapsed_ms`）；就绪里程碑 `06_ready`（窗口+GPU+预热帧就绪）；空闲起点的另一信号是日志行“system fonts applied”（后台字体补装完成）；`SIGTERM` 收尾。
- 内存双口径实名：RSS 采样用 `ps`（压缩内存与 swap 不计，低估真实占用）；phys_footprint 采样用 `vmmap`（活动监视器口径）。
- 体积字段按本机构建架构填 `binary_x64_kb` / `binary_arm64_kb`（零新增采集）。
