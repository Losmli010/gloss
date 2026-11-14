# mouse（系统（macOS）鼠标手势源绑定）测试记录

> 按符号锚定（不写行号，防漂移）。改到相关测试时同步更新本文件。
> 测试代码禁止注释（AGENTS.md“注释纪律”节），测试的验收意图与辅助契约记录在此。

## 平台绑定（2026-11-14，方案 A5 P2）

- 判定状态机与脚手架的共享测试随逻辑上移共享层（见 gesture 测试记录）；
  本文件只留系统（macOS）绑定面：订阅面断言与真实 tap 的建立测试。测试
  行为清单（BDD）统一住 docs/tests/gloss-platform.md。

## 辅助契约

- `keyboard_events_are_never_subscribed`：经生产订阅清单、`classify` 与
  `imp::subscribed_mask` 测试缝隙断言订阅掩码只有左键两位，键盘、滚轮与
  修饰键类型不置位（SIGILL 链路的回归护栏）。
- `spawn_resolves_synchronously_and_startup_failure_stays_off_the_flag`：
  真实 `tap` 启动（结果随测试机授权而异）同步返回且启动失败不经降级标志
  表达（标志只属运行中失效）。
- 真机注入式活测（`injected_drag_yields_selection_gesture`，L4）留在本
  文件，登记在人工测试节。
