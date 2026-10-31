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

## 符号契约（自代码注释迁入）

- `EventSource`：组装点提供的闭包与命令处理器同等对待：panic 不允许带倒事件线程。
- `TICK`：源事件的 drain 周期：run loop 无法阻塞等待 crossbeam 通道，只能按定时器抽干。手势到浮层的端到端延迟上界即此值。
- `EventSink`：事件线程的产物出口：④ 回传事件与 ① 平台事件，由组装点接上真实通道。`wake` 是主线程唤醒回调：主线程可能睡在事件循环里，发送成功后不叫醒它，消息要等下一个无关事件才被看到。发送失败（接收端已消失）不唤醒。
  - `new`：`events` 是 ④ 回传通道，`platform` 是 ① 平台事件通道；`wake` 只在发送成功后调用。
  - `send_event` / `send_platform`：主线程已退出时返回 `false`：消息丢弃，也不唤醒。
- `EventThread::join`：退出协议：调用方 drop 通道②的 Sender，线程抽干剩余命令后自行结束；`join` 返回即线程已终止。
- `spawn`：`on_command` 在事件线程上顺序消费通道②，`sources` 每个 tick 抽干一轮；两者都不允许把 panic 带进线程。线程缺失时应用仍能启动，但取材手势全部失效——必须留痕。
- `guarded`：应用代码注入的回调不允许把 panic 带进事件线程；捕获后循环继续服务。release 以 `panic = "abort"` 构建时 panic 在事发点直接 abort，`Err` 分支不可达——屏障只在 unwind 构建（dev/test 及其单测）生效。
- `tick`：一轮消费：先抽干通道②（出现 Disconnected 即退出信号），再抽干全部事件源——同一线程内命令总排在系统事件之前。`send_platform` 成功时经 `notify()` 调组装点注入的唤醒回调，panic 会穿过 C 定时器回调（fire）导致进程 abort——与命令处理、源 poll 同受屏障约束。
- `LoopState`：事件线程 run loop 的共享状态盒：`fire` 回调经 context 的裸指针取回。
- `fire`：定时器回调：抽干各源；通道② Sender 全部 drop 时停掉 run loop，让 `CFRunLoopRun` 返回、线程随之结束。
- `install_timer`：以 `state` 为回调上下文建周期定时器并挂进当前 run loop；创建失败返回 `None`（调用方收卷事件线程）。
- `run_loop`：事件线程的驱动：RunLoop 不能阻塞在 crossbeam 上，挂一个周期定时器跑 `tick`；自带 run loop 的源在各自线程上运行，产物经通道汇入这里抽干。收尾时 RunLoop 已停止且不在回调中：CF timer 的规范清理是先 invalidate 再 release，最后收回状态盒。
