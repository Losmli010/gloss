# restore.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。

## RESTORE_ATTEMPTS / RESTORE_RETRY_DELAY

- 恢复写盘的尝试次数与退避：CI 实测高负载虚拟机上粘贴板服务会出现 ≥100ms 级的持续不可用（3×50ms 重试未成功清板一次，板上永久留下注入副本、用户原内容丢失），短窗口熬不过去；最坏退避 5×200ms ≈ 1s，低于事件线程既有的最坏占用（写入确认两轮 1.6s），且只在已失败的路径上付出。

## RestoreStage

- 恢复序列的失败环节：随 error! 进日志（生产可定位），测试二进制无 subscriber，live 测试经 `gloss_core::log::capture` 取同一条进断言消息。
- `Open`：打不开粘贴板（名称构造失败，或 PasteboardCreate 返回非零）。
- `Alloc`：预建 flavor 的 CFDataCreate 返回空。
- `Clear`：PasteboardClear 返回非零。
- `Put`：PasteboardPutItemFlavor 返回非零。

## RestoreOutcome

- 恢复写盘的结果：`ok` 表示快照已完整写回（重试耗尽也不影响调用方主结果）；`clears` 是实际发生的清板次数——Clear 是恢复序列里唯一推进 changeCount 的调用（Put 实测不推进），哨兵的写盘归因必须按它计数，半程失败后的重试成功会清两次板，固定记 1 会把差额误判成用户的外来复制；`stage`/`status` 仅在 `ok` 为假时有意义（系统调用无状态码的场合记 0，名称构造失败记 -1）。

## restore_pasteboard

- 把快照原样写回：见 `restore_with_retry` 与 `restore_pasteboard_once`。

## restore_and_log

- 恢复原剪贴板内容；失败留痕（环节与状态码，供定位粘贴板服务的持续不可用）但不影响调用方的主结果。

## restore_with_retry

- 带重试的恢复：单次序列失败短退避后重试（每次开全新粘贴板引用，上一次 Clear 成功而 Put 失败的半程态由本次 Clear 重置），成功即止；`clears` 累加全部尝试的实际清板数，含失败尝试。退避经参数注入以便单测零延时驱动。

## restore_pasteboard_once

- 单次恢复序列；失败的具体环节由各分支留 debug 痕。阶段一：预建全部 flavor 数据；任一失败即放弃，已建引用由 staged 里各守卫随 Vec 一起释放。阶段二：数据全部就绪才清板写入。

## stage_flavor_data

- 阶段一：预建全部 flavor 的 CFData（引用由守卫随 Vec 持有）；任一分配失败返回 `None`。
