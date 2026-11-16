# restore.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 重试骨架与写盘结果形状（`restore_with_retry`、`RestoreOutcome`、剂量常量）随方案 A2（2026-11-17）上移共享层，见 fallback 记录。

## RestoreStage

- 恢复序列的失败环节：随 error! 进日志（生产可定位），测试二进制无 subscriber，live 测试经 `gloss_core::log::capture` 取同一条进断言消息。
- `Open`：打不开粘贴板（名称构造失败，或 PasteboardCreate 返回非零）。
- `Alloc`：预建 flavor 的 CFDataCreate 返回空。
- `Clear`：PasteboardClear 返回非零。
- `Put`：PasteboardPutItemFlavor 返回非零。

## restore_pasteboard

- 把快照原样写回：重试骨架见共享层 `restore_with_retry`（fallback 记录），单次序列见本模块 `restore_pasteboard_once`。

## restore_and_log

- 恢复原剪贴板内容并返回实际写盘数（供恢复收尾计入写盘归因）；失败留痕（环节与状态码，供定位粘贴板服务的持续不可用）但不影响调用方的主结果。

## restore_pasteboard_once

- 单次恢复序列；失败的具体环节由各分支留 debug 痕。阶段一：预建全部 flavor 数据；任一失败即放弃，已建引用由 staged 里各守卫随 Vec 一起释放。阶段二：数据全部就绪才清板写入。

## stage_flavor_data

- 阶段一：预建全部 flavor 的 CFData（引用由守卫随 Vec 持有）；任一分配失败返回 `None`。
