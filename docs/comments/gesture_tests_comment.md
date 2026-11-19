# gesture 测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md“注释纪律”节），测试的验收意图与辅助契约记录在此。

## 上移平台中立共享层（2026-11-14，方案 A5 P2）

- 本文件承接原系统（macOS）mouse 测试记录（git mv 保沿革）：两侧镜像的
  判定状态机测试合并为共享模块一节、一份锁两平台；共享侧新增两条脚手架
  锁定（见下）。测试行为清单（BDD）统一住 docs/tests/gloss-platform.md。

## 测试目标与层

- 第一层（L1）：手势判定状态机（时长与位移双达标、甩动过滤、边界值、
  状态复位、野事件忽略、通道抽干）与监听线程脚手架（同步握手、队列满
  丢弃），全部以替身事件与替身监听闭包驱动，不装真实监听器——真实
  tap/钩子的建立测试留各侧平台目录。

## 辅助契约

- pressed / released：构造带坐标与时长（`Duration`）的按键事件夹具助手。
- `press_duration_is_observed_at_the_boundary` 一条锁两平台时间戳粒度：
  差 1 纳秒（系统（macOS）粒度）与差 1 毫秒（系统（Windows）粒度）都
  滤除，恰好 150 毫秒判定。
- `a_failed_creation_leaves_the_source_absent_and_the_flag_cold`：替身
  创建闭包回报失败，`spawn` 同步握手返回缺席源且降级标志保持为假——启动
  失败不经运行中标志表达（锁定原两侧 spawn 测试只随真机条件部分覆盖的
  失败路径）。
- `events_beyond_the_queue_capacity_are_dropped_without_blocking`：替身
  创建闭包在回报前一次性投递容量加一对的完整手势事件对（投递先于同步
  握手完成，无时序竞态），轮询恰得容量内的手势数——锁定“满则丢弃、汇点
  不阻塞不崩”。
