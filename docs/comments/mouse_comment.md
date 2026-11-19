# mouse 决策记录（系统（macOS）鼠标手势源绑定）

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），改名与合并用 git mv 保留历史；文件内不留指向本文件的指针。

## 平台绑定（2026-11-14，方案 A5 P2）

- 手势判定状态机与监听线程脚手架（含 panic 屏障、队列容量与手感常量）
  上移平台中立共享层（决策与沿革见 gesture 记录）后，本文件只承载系统
  （macOS）绑定：订阅清单与 `classify`（事件类型查表）、`imp::create`
  与 `imp::run` 两个闭包（创建相建立 tap 并挂当前线程运行循环、宿主相
  阻塞运行）、`raw_callback` 的事件翻译（坐标与纳秒时间戳换算）。
  `MouseSource::spawn` 保持装配点既有入口（内部改走共享
  `spawn_listening`），引用面零变化。

## 窄订阅面的 SIGILL 链路

- 2026-09-19 订阅“全部事件”的实现会把键盘事件送进回调，而把按键翻成字符要走输入法 API（macOS 的 TSM/HIToolbox 要求主线程），回调跑在监听线程上会以 SIGILL 打死整个进程——跑起来的 Gloss 只要用户按任意键（含 Ctrl-C）就消失。故键盘事件不进入本模块，热键归 `global-hotkey`（架构速览有同款简版约束）。
- 2026-09-19 具体元凶与现状（自建 CGEventTap 的依据）：rdev 的 `listen` 把 tap 订阅成 `kCGEventMaskForAllEvents`，键盘事件也进回调，且它把按键翻成字符要调 `TISCopyCurrentKeyboardInputSource`（TSM/HIToolbox，要求主线程）——回调跑在 `gloss-mouse-tap` 线程上，libdispatch 的 `dispatch_assert_queue` 断言失败后以 SIGILL（`EXC_BAD_INSTRUCTION`）打死进程，崩溃线程名即 `gloss-mouse-tap`。rdev 在本仓库仍用于按键注入（剪贴板兜底），只是不再用于手势监听。（2026-10-13 起：按键注入也已改走手写 ffi/event.rs，rdev 整体退役——见 gloss-platform_Cargo_comment.md 的 rdev 节。）

## 排除类型的测试可断言性

- 2026-09-19 在生产侧列出必须排除的类型（键盘 10/11/12 与滚轮 22），是为了让“键盘事件绝不进订阅面”可被 tests 的 `keyboard_events_are_never_subscribed` 断言（测试缝隙经 `imp::subscribed_mask`）。

## 授权表述收敛（2026-10-09）

- 2026-10-09 模块注释、`ListenError::EventTap` 与 `MouseSource::spawn` 文档中“输入监控/辅助功能”的双授权表述统一改为辅助功能单项：辅助功能授权下 listen-only tap 即可建立（同日真机日志佐证），输入监控引导已随 permissions.rs 一并移除。tap 建立失败 → 手势降级的路径与日志不变。

## 符号契约

- `CG_EVENT_LEFT_MOUSE_DOWN` / `CG_EVENT_LEFT_MOUSE_UP`：CGEventTypes.h 的事件类型原始值。用原始数字而不是 core-graphics 的 `CGEventType`：订阅规则（尤其“键盘绝不订阅”）不依赖系统头文件即可单测。
- `SUBSCRIBED_EVENT_TYPES`：订阅清单是唯一事实来源：订阅掩码与回调过滤都由它推出，实现里不另写第二份口径——否则键盘事件会从两处口径的缝隙里回来。
- `classify`：未订阅的类型返回 `None`，回调据此原样放行事件。
- `ListenError`：监听启动失败的原因，只用于日志与降级判定：手势可降级，失败不向上传播错误。`EventTap`：系统拒绝建立事件 tap：最常见的原因是未授予辅助功能权限。`RunLoopSource`：运行循环源建立失败（此后手势永久降级）。
- `MouseSource::spawn`：返回 `None` 即 tap 没建起来（未授权辅助功能最常见），组装点据此同步判定手势可用性，无须轮询。建立的等待是“创建相结果”通道事件，有界且无用户交互。
- `imp`：自建只订阅左键的 CGEventTap（系统（macOS）原语侧）。
- `imp::subscribed_mask`：订阅掩码：由共享的 `SUBSCRIBED_EVENT_TYPES` 推出——掩码不另写一份，否则键盘事件会从两处口径的缝隙里回来。
- `imp::TapHandle`：已建成待运行的 tap：`create` 与 `run` 两相的交接凭证。tap 与其 run loop source 已挂上当前线程的 run loop，句柄本身无状态——它让“没建成就不能运行”成为类型层面的约束。
- `imp::create`：在**当前线程**建立只订阅左键按下/释放的 tap（创建相）：建 tap、建 run loop source、挂上当前线程 run loop 并启用。不阻塞运行——那是 `run`（阻塞相）的事，两相之间创建结果才得以同步发回调用方（组装点据此判定手势可用性，见共享 `spawn_tap`）。调用方保证：`sink` 不向外抛 panic——它由 C 回调调用，panic 穿过 C 边界会 abort 进程（兜底在共享 `tap_loop`）。回调上下文必须活到进程结束：tap 没有停止 API，本线程的 run loop 一直跑到进程退出。故 **tap 建成后**有意泄漏这份 Box（每进程至多一份）——注意它是“泄漏”而非 `mem::forget` 式不可达：tap 的 user_info 一直指着它。两条失败路径都会把它收回。二次装箱是为了给 `user_info` 一个瘦指针（共享汇点已装箱一次，这里再装一层 `Box<dyn Fn>` 得到瘦指针，指针形状与上移前一致）。没建成 tap：回调永远不会来，把上下文收回，别白泄漏一份。没建起 run loop source：tap 与上下文都不会有人用，一并收回——这条路径之后手势就永久降级了，没有“活到进程结束”的理由。
- `imp::run`：阻塞运行当前线程的 run loop（阻塞相）：订阅的事件从这里投递进 `raw_callback`。只在系统层面停止投递时返回（如 tap 失效），没有停止 API——返回即手势失效，调用方按降级处理。
- `imp::raw_callback`：tap 回调：把订阅到的事件翻译成 `ButtonEvent` 交给汇点，未订阅的一律原样放行（listen-only 的 tap 也必须返回事件本身）。翻译只用常量查表加位置与时间戳各一次读取（不分配、不调用会 panic 的 API），时间戳按事件自身纳秒粒度换算为时长（`Duration`），panic 兜底在汇点侧（见共享 `tap_loop` 的调用方保证）。
