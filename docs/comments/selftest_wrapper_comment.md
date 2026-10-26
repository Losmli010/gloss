# selftest_wrapper.py 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），代码内不留指向本文件的指针。

## wrapper 落地形态（T3，2026-10-07）

- 任务卡要求「justfile 的 selftest-report 改造为 wrapper」；wrapper 本体落 `scripts/perf/selftest_wrapper.py`（仓库既有模式：just 配方薄、逻辑进 scripts/），just 配方只传 `--out`。
- **python 而不是 bash**：需要亚秒时间戳（macOS 系统 bash 3.2 无 `EPOCHREALTIME`、`date` 无 `%N`）、逐行 JSON 解析（jq 不可假设在位）、cargo `--message-format=json` 的产物解析——python3 是仓库既有硬依赖（同目录 app-metrics.py 等同一前提）。兼容系统 python 3.8：`from __future__ import annotations` 让内建泛型注解可用。
- **直接跑自检二进制而不是经 cargo test**：wrapper 要轮询测试进程 RSS，需要稳定 pid——cargo test 的子进程 pid 得靠 pgrep 猜；直接运行 `--no-run` 产物（路径取自 cargo `--message-format=json` 的 compiler-artifact 消息）即得 Popen 的 pid。退出码语义与 cargo test 一致（就是自检二进制自身的返回码）。
- **stderr 落临时文件而不是 PIPE**：管道缓冲约 64KB，子进程可能在 wrapper 忙于轮询循环时把管道写满而阻塞——文件无此风险，进程结束后 seek(0) 读全量再过滤。
- **RUST_LOG 兜底 info**：startup 信号行走日志系统（info 级），用户环境 `RUST_LOG=off` 会把信号静掉；wrapper 只在未设置时兜底 info，不覆盖显式配置。overlay 的信号行（eprintln! 直写 stderr）不依赖订阅者，未受此影响。
- **信号契约**：overlay stderr 的 `kind=overlay_round` / `kind=overlay_perf` JSON 行原样提取；startup 提取 `message=startup self-test finished` 的 JSON 行；**非信号 stderr 原样回显到 wrapper 自身 stderr**（2026-10-07 PR #129 评审补）：否则自检失败时 error! 诊断（超预算/初始化失败）随临时缓冲销毁，定位必须手工重跑；RSS 曲线由 wrapper 产出为 `kind=rss_curve` 行（samples 为 [自进程启动毫秒, RSS kb] 数组）。commit 注入沿用 2026-09-21 的决定（测试只读 `GLOSS_PERF_COMMIT` 环境变量），注入方从 just 配方换成 wrapper。
- **退出码**：任一自检非 0 → wrapper 1；「自检退出 0 但预期信号行缺失」也判失败——handoff 静默缺信号比失败更难排查。数据先落盘再判失败。
- **GLOSS_PERF_OUT 废除**：原机制（测试内读 GLOSS_PERF_OUT 追加写 JSONL + 配方 mkdir）整体删除；RSS 曲线是唯一移交外部的数据（测试内本就调同一 `ps -o rss=`，外部采样无精度损失，轮询密度由 wrapper 掌控）。
- **已知局限**：ps 轮询每次 spawn 有 ms 级 CPU 开销，负载高时间隔拉长（实测本机负载 150 时采样间隔中位 98ms，设计间隔 50ms）——曲线是趋势信号（仅记录），密度退化可接受；极端内存压力下进程大部分页面被换出时 ps 读数趋 0（实测 swap 5.8G 时出现），趋势采集应在低负载窗口进行（同 #126 的负载口径）。门禁判定不受影响：首帧与 RSS 尾段门禁都在测试进程内判，wrapper 只搬运信号。

## 文件头迁出的流程与用法（2026-10-24，注释纪律）

- 用法：`scripts/perf/selftest_wrapper.py`；`scripts/perf/selftest_wrapper.py --out
  /tmp/selftest.jsonl --poll-ms 50` 指定输出与轮询间隔。
- 流程实名：`cargo test --no-run` 构建两个自检二进制；自检只读 `GLOSS_PERF_COMMIT`
  环境变量（wrapper 注入短 commit）；信号行为 stderr 上 `kind=overlay_round` /
  `kind=overlay_perf` 与 `message=startup self-test finished` 的 JSON 行；RSS 曲线行
  `kind=rss_curve`（samples 为 [自进程启动的毫秒, RSS kb] 数组）；缺省输出
  `target/perf/selftest.jsonl`（本地 handoff，不入 git）。
