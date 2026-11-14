# windows_mouse（系统（Windows）鼠标手势源绑定）测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md“注释纪律”节），测试的验收意图与辅助契约记录在此。

## 平台绑定（2026-11-14，方案 A5 P2）

- 判定状态机与脚手架的共享测试随逻辑上移共享层（见 gesture 测试记录）；
  本文件只留系统（Windows）绑定面：订阅面断言与真实钩子的建立测试。测试
  行为清单（BDD）统一住 docs/tests/gloss-platform.md。

## 辅助契约

- 名称前缀（`hook_`）：与系统（macOS）目录的同名文件互相区分（清单限定
  名唯一按文件绑定，两平台同名文件无法以限定符消歧，见
  windows_events_mod_tests 记录）。
- `hook_spawn_resolves_synchronously_and_startup_failure_stays_off_the_flag`：
  建立握手同步返回，启动失败不复位降级标志；该测试在真机上会装真实钩子
  （监听无授权门），随测试进程退出消亡。
- `hook_mouse_messages_are_the_only_subscription`：键盘、移动、右键消息
  分类为空，未订阅消息号回落为空。
