# events/mod.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## 线程 panic 留痕

- 2026-09-19 为什么 panic 要 error 留痕：防护齐备时线程 panic 近乎不可能，但一旦发生若不留痕，事件线程死了应用无任何信号。

## guarded 与 release panic=abort（P2-1，2026-10-13）

- 为什么 release 上 `panic = "abort"` 后 `guarded` 保留而不删除：release 下
  `Err` 分支不可达（panic 在事发点 abort 进程，降级语义随之消失——行为变更
  须在 PR 描述显式记录，开 PR 时落实），但 dev/test 构建仍是 unwind（test 目标由 cargo 强制 unwind），屏障
  照常生效：开发构建里回调 panic 不带倒事件线程（panic 一旦穿过 `run_loop` 的
  `extern "C"` 定时器回调 `fire` 即 abort），且
  `panicking_handler_does_not_kill_thread` / `panicking_source_does_not_kill_thread`
  两条 L1 单测在测试 profile 锁定该行为——删除即同时退化测试 profile。处置为
  旁注“生效边界”；日后要恢复 release 降级语义，删 profile 一行即可。
- 为什么 `Err` 分支的 `error!` 留痕在 abort 下不视为违反“降级必有痕”：分支在
  release 不可达即无静默吞错面；它在 unwind 构建可达且有痕。

## 共享形状迁往事件线程骨架（2026-11-07，移植评审遗留 A1）

- 事件源契约（`EventSource`）、汇点（`EventSink`）、线程句柄
  （`EventThread`）、`spawn`、恐慌屏障（`guarded`）、节拍抽干（`tick`）
  与节拍周期（`TICK_MS`）是两平台一致的平台无关形状，迁往
  `gloss-platform/src/event_thread.rs`（缘由与符号契约见事件线程骨架
  记录）；本模块只剩运行循环宿主，`spawn` 包装经函数指针注入本模块的
  `run_loop`，公开签名不变。

## 运行循环宿主符号契约（自代码注释迁入）

- `LoopState`：事件线程 run loop 的共享状态盒：`fire` 回调经 context 的裸指针取回。
- `fire`：定时器回调：经共享骨架的 `tick` 抽干各源；通道② Sender 全部 drop 时停掉 run loop，让 `CFRunLoopRun` 返回、线程随之结束。panic 穿过 C 定时器回调（fire）会导致进程 abort——命令处理、源 poll 与发送同受骨架里的屏障约束，屏障只在 unwind 构建生效。
- `install_timer`：以 `state` 为回调上下文建周期定时器并挂进当前 run loop；周期取共享骨架的 `TICK_MS`；创建失败返回 `None`（调用方收卷事件线程）。
- `run_loop`：事件线程的驱动：RunLoop 不能阻塞在 crossbeam 上，挂一个周期定时器跑 `tick`；自带 run loop 的源在各自线程上运行，产物经通道汇入这里抽干。收尾时 RunLoop 已停止且不在回调中：CF timer 的规范清理是先 invalidate 再 release，最后收回状态盒。
