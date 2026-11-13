# 测试行为清单（`gloss`）

本文件登记入口包的测试行为（单元加性能自检）；总纲、人工测试与发版人工步骤见 `bdd.md`。

## 性能测试

文件：`tests/overlay.rs`、`tests/startup.rs`（均 `L3`，`harness = false` 自带 `main()`，跑在主线程，需窗口服务与 `GPU`）。

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| overlay | 真实窗口栈显隐生命周期与首帧、驻留集（RSS）预算 | 给定经公共（API） `build_window_stack` 预创建的生产窗口栈，当反复 `show` → 渲染 → `hide` 共 100 轮（每轮停留 80 毫秒），则统计 `show`→首帧延迟并计入门禁：跑满 100 轮且有延迟统计退出 0，无帧、冷启动首显超 250 毫秒预算、稳态首显（第 2 轮）超 100 毫秒预算、或驻留集（RSS）尾段净增长（第 25 轮 → 收尾）超 `2048kb` 预算退出 1（前 25 轮属一次性预热分配，泄漏信号看尾段斜率；冷启动首显单列预算——虚拟化 `GPU` 的着色管线首次编译噪声不进稳态口径，两笔预算数值以源码常量为准）；延迟分位、帧时间、尾段、句柄、预算字段进 `stderr` 结构化行（每轮一条 `round` 标记行，收尾一条 `overlay_perf` 汇总行，含 `first_ms` 冷启动与 `first_warm_ms` 稳态两笔首显）；驻留集（RSS）尾段门禁判定留在自检内不变，完整曲线由 `selftest-report` `wrapper` 外部 `ps` 轮询采样 | 2026-10-30 |
| startup | 启动渲染栈段预算门禁 | 给定经公共（API） `build_window_stack` 的生产同源初始化（`EventLoop` → 窗口栈 → 预热帧），当计进程内起点到预热帧完成的墙钟，则不超 4000 毫秒预算退出 0，超预算或初始化失败退出 1；分段耗时经日志里程碑 `06a/06b/06c` 供人工定位（预算数值以源码常量为准，改动随本清单同步） | 2026-10-07 |

## 单元测试

### `src/main.rs`

| 测试名称 | 测试目标 | 测试场景 | 更新时间 |
| --- | --- | --- | --- |
| channels_bundle_is_created | 组装点创建的通道捆绑可用 | 给定 `create_channels()` 创建的四通道捆绑，当向通道二发送 `AcquireText` 命令，则发送成功 | 2026-09-19 |
| log_dir_override_wins_over_home | 日志目录覆盖优先于平台默认根 | 给定 `GLOSS_LOG_DIR` 覆盖值与平台默认根同时存在，当 `resolve_log_dir`，则返回覆盖值原样 | 2026-11-05 |
| log_dir_falls_back_to_home_dot_gloss | 日志目录回落平台默认根 | 给定无覆盖值，当 `resolve_log_dir`，则返回平台默认根下的 `logs`；系统（macOS）形态为 `<home>/.gloss/logs` | 2026-11-05 |
| log_dir_is_none_without_override_and_home | 无覆盖无默认根时放弃文件日志 | 给定覆盖值与平台默认根均缺失，当 `resolve_log_dir`，则返回 `None` | 2026-11-05 |
| log_dir_empty_override_falls_back_to_home | 空串覆盖视同未设置 | 给定 `GLOSS_LOG_DIR` 为空串，当 `resolve_log_dir`，则回落平台默认根下的 `logs`；系统（macOS）形态为 `<home>/.gloss/logs` | 2026-11-05 |
