# events/mod.rs 测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md“注释纪律”节），测试的验收意图与辅助契约记录在此。

## 测试行为速览（BDD）

已统一迁往 docs/tests/bdd.md（git 追踪，以测试代码为唯一事实源）；本文件只留缘由与形态说明。

## 辅助

- echo / no_sources：本地桩类型（platform 不依赖 gloss-app，桩足以驱动整条循环）。
- 晚到命令用例先等过首个消费窗口再发第二条：命令要跨一个定时器周期才被抽干。

## 共享测试迁往事件线程骨架（2026-11-07，移植评审遗留 A1）

- 抽干顺序（`tick_drains_commands_before_polling_sources`）、汇点容忍关闭
  （`sink_send_tolerates_closed_receivers`）与成功发送唤醒
  （`successful_send_wakes_main_thread`）测的是共享形状，随代码迁入
  `event_thread.rs` 的测试模块；本模块保留起真线程跑运行循环宿主的五条
  端到端组装测试，行为断言不变。
