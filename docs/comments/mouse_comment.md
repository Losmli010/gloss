# mouse.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## 手势策略取舍

- 2026-09-19 取“拖拽释放”而不用其它方案的原因：双击会把“打开链接”这类普通操作误判成划词，修饰键确认需要额外的键盘全局钩子，停留判定需要定时器——拖拽释放是误触最少的最简状态机。其余策略（含框选 `RegionGesture`）待配置化与图像任务落地时引入。

## 窄订阅面的 SIGILL 链路

- 2026-09-19 订阅“全部事件”的实现会把键盘事件送进回调，而把按键翻成字符要走输入法 API（macOS 的 TSM/HIToolbox 要求主线程），回调跑在监听线程上会以 SIGILL 打死整个进程——跑起来的 Gloss 只要用户按任意键（含 Ctrl-C）就消失。故键盘事件不进入本模块，热键归 `global-hotkey`（架构速览有同款简版约束）。
- 2026-09-19 具体元凶与现状（自建 CGEventTap 的依据）：rdev 的 `listen` 把 tap 订阅成 `kCGEventMaskForAllEvents`，键盘事件也进回调，且它把按键翻成字符要调 `TISCopyCurrentKeyboardInputSource`（TSM/HIToolbox，要求主线程）——回调跑在 `gloss-mouse-tap` 线程上，libdispatch 的 `dispatch_assert_queue` 断言失败后以 SIGILL（`EXC_BAD_INSTRUCTION`）打死进程，崩溃线程名即 `gloss-mouse-tap`。rdev 在本仓库仍用于按键注入（剪贴板兜底），只是不再用于手势监听。（2026-10-13 起：按键注入也已改走手写 ffi/event.rs，rdev 整体退役——见 gloss-platform_Cargo_comment.md 的 rdev 节。）

## 排除类型的测试可断言性

- 2026-09-19 在生产侧列出必须排除的类型（键盘 10/11/12 与滚轮 22），是为了让“键盘事件绝不进订阅面”可被 tests 的 `keyboard_events_are_never_subscribed` 断言（测试缝隙经 `tap::subscribed_mask`）。

## MIN_PRESS_NANOS（最短按压时长 150ms，2026-09-30）

- 起因：手势判定原本只有位移一条规则，HID 全局 tap 连拖 Gloss 自己窗口、
  以及各种瞬时甩动都会被判成划词（interaction_review 的 P2-11）。纯位移
  判据区分不了“有意的拖拽”与“按下后立刻释放的甩动”——两者位移都可以
  超过 12pt，差别只在按压时长。
- 取值缘由：人手有意的拖拽选择从按下到释放稳定在数百毫秒量级（一次普通
  点按的按压就有约 100ms，有意的拖拽还要加上拖动的时间）；而甩动/连滑类
  误触的按下与释放间隔趋近于零。150ms 落在两者之间、两侧都留足裕量——
  快甩滤得掉，最慢的正常点按与最快的拖拽都不受影响。不做成配置项：阈值
  是手感参数，暴露出去只会让用户调坏（与“手势开关不进设置”同一取舍，
  见“明确不做”）。
- 时间戳取 CGEvent 自身的 `kCGEventTimestampField`（ns，单调）而不是回调
  投递时刻：tap 回调经 RunLoop 投递，同一手势的按下/释放两事件的投递延迟
  可以差一个 tick（33ms），拿投递时刻算时长会漂；事件自带时间戳则与投递
  节奏无关。模拟事件（缺时间戳）按 0 处理，时长判定失败闭合（滤除）。
- 注入式 live 测试相应在释放前真实 sleep 过时长下限（200ms）——注入事件
  自带真实时间戳，不睡就会被本条滤除，那是想要的行为。

## GestureDetector::feed（释放坐标载荷）

- 2026-09-20 手势判定从“是否触发”改为“触发 + 释放坐标”：释放事件自带坐标（事件自带坐标的既有口径，见 ButtonEvent 的模块注释），划词浮层跟随选区用的就是它。坐标整型化为 ScreenPoint（与 ScreenRect 同口径）在 platform→组装点边界完成，手势层保持 f64 原值判定阈值。

## 授权表述收敛（2026-10-09）

- 2026-10-09 模块注释、`ListenError::EventTap` 与 `MouseSource::spawn` 文档中“输入监控/辅助功能”的双授权表述统一改为辅助功能单项：辅助功能授权下 listen-only tap 即可建立（同日真机日志佐证），输入监控引导已随 permissions.rs 一并移除。tap 建立失败 → 手势降级的路径与日志不变。

## tap sink 的 panic 屏障与 release panic=abort（P2-1，2026-10-13）

- 为什么 release 上 `panic = "abort"` 后 sink 闭包里的 `catch_unwind` 保留而不删除：
  release 下屏障不再拦截，panic 在事发点 abort 进程（C 回调屏障的防 abort 保护
  同步消失——行为变更须在 PR 描述显式记录，开 PR 时落实）；dev/test 构建仍是 unwind（test 目标由 cargo
  强制 unwind），无屏障时 sink 的 panic 会穿过 `raw_callback` 这个 C 回调边界
  直接 abort——屏障保的是开发构建的监听存活。删除只退化开发/测试构建的容错面，
  处置为旁注“生效边界”，与 events/mod.rs 的 `guarded` 同一口径。

## 符号契约（自代码注释迁入）

- `DRAG_THRESHOLD`：判定为拖拽所需的最小位移（系统逻辑点）：远高于点击抖动，又低于任何有意拖拽；坐标是逻辑点，不必按 DPI 折算。
- `MIN_PRESS_NANOS`：判定为拖拽所需的最短按压时长（纳秒）。有意的拖拽稳定在数百毫秒（点击本身的按压就有约 100ms），甩动与 HID 连滑趋近于零；150ms 落在两者之间，两侧都留裕量。
- `ButtonEvent`：tap 层交给手势层的事件，坐标与时间戳都取自**该事件自身**。不能缓存“最近一次移动位置”：真实拖拽期间系统只投递拖拽事件而非移动事件，缓存会停在按下之前，释放时算出的位移恒为零，手势永远判不出来。时间戳同理取事件自带的（纳秒，单调），按压时长不依赖回调投递延迟。`time` 字段：事件自身的时间戳（纳秒，自系统启动起）。
- `CG_EVENT_LEFT_MOUSE_DOWN` / `CG_EVENT_LEFT_MOUSE_UP`：CGEventTypes.h 的事件类型原始值。用原始数字而不是 core-graphics 的 `CGEventType`：订阅规则（尤其“键盘绝不订阅”）不依赖系统头文件即可单测。
- `SUBSCRIBED_EVENT_TYPES`：订阅清单是唯一事实来源：订阅掩码与回调过滤都由它推出，实现里不另写第二份口径——否则键盘事件会从两处口径的缝隙里回来。
- `classify`：未订阅的类型返回 `None`，回调据此原样放行事件。
- `GestureDetector::feed`：无按下记录的释放（如监听启动前就按下的拖拽），静默忽略。
- `MouseGesture`：划词手势产物（platform 本地类型；组装点映射为 ① 的平台事件）。`Selection`：一次完整的拖拽选择，载荷是释放坐标（系统全局坐标，逻辑点、左上原点）——浮层跟随划词位置用。`pos`：整型化的逻辑点，与 `ScreenPoint` 同一口径。
- `ListenError`：监听启动失败的原因，只用于日志与降级判定：手势可降级，失败不向上传播错误。`EventTap`：系统拒绝建立事件 tap：最常见的原因是未授予辅助功能权限。
- `MouseSource::spawn`：返回 `None` 即 tap 没建起来（未授权辅助功能最常见），组装点据此同步判定手势可用性，无须轮询。建立的等待是“创建相结果”通道事件，有界且无用户交互。标志只在 tap 中途停止投递时置位，启动失败不经它表达。
- `spawn_tap`：启动 tap 监听线程并**同步等待**创建相结果：成功返回事件通道，失败返回 `None`（原因已记日志）。创建相与阻塞相分离，失败在进 run loop 前就确定。创建相在回报前就消亡（panic 于纯 FFI 与装箱路径，理论不可达）：按缺失降级，与创建失败同一出口。
- `tap_loop`：tap 监听线程的执行体：事件经 panic 屏障投递（满则丢弃），创建成功先回报，再进阻塞的 run loop。事件 tap 回调是 C 回调，panic 穿过它直接 abort 进程：与事件线程的 guarded 同一纪律，unwind 构建下兜底后监听继续；release 以 `panic = "abort"` 构建时本屏障不再拦截，panic 在事发点 abort 进程（生效边界同 guarded）。兜底放在这里而不在 tap 实现里，是模块与实现之间的分工——实现侧只需保证自己不 panic（它的活只是常量查表加读一次坐标）。panic 屏障：闭包的结果（含 panic 信号）有意整体丢弃，监听循环只关心是否继续。通道满时丢弃：手势判定不需要完整事件流，反压 tap 回调的代价远大于丢一次触发机会。创建成功先回报再进阻塞的 run loop（回报通道只有一位且调用方正同步等待；即便对方已提前放弃也不影响监听）。此后只有“运行中停止投递”一条退出路径（tap 没有停止 API），置位降级标志并留 info 便于诊断。
- `tap::imp`：自建只订阅左键的 CGEventTap。
- `imp::subscribed_mask`：订阅掩码：由共享的 `SUBSCRIBED_EVENT_TYPES` 推出——掩码不另写一份，否则键盘事件会从两处口径的缝隙里回来。
- `imp::TapHandle`：已建成待运行的 tap：`create` 与 `run` 两相的交接凭证。tap 与其 run loop source 已挂上当前线程的 run loop，句柄本身无状态——它让“没建成就不能运行”成为类型层面的约束。
- `imp::create`：在**当前线程**建立只订阅左键按下/释放的 tap（创建相）：建 tap、建 run loop source、挂上当前线程 run loop 并启用。不阻塞运行——那是 `run`（阻塞相）的事，两相之间创建结果才得以同步发回调用方（组装点据此判定手势可用性，见 `spawn_tap`）。调用方保证：`sink` 不向外抛 panic——它由 C 回调调用，panic 穿过 C 边界会 abort 进程（兜底在 `spawn_tap`）。回调上下文必须活到进程结束：tap 没有停止 API，本线程的 run loop 一直跑到进程退出。故 **tap 建成后**有意泄漏这份 Box（每进程至多一份）——注意它是“泄漏”而非 `mem::forget` 式不可达：tap 的 user_info 一直指着它。两条失败路径都会把它收回。二次装箱是为了给 `user_info` 一个瘦指针。没建成 tap：回调永远不会来，把上下文收回，别白泄漏一份。没建起 run loop source：tap 与上下文都不会有人用，一并收回——这条路径之后手势就永久降级了，没有“活到进程结束”的理由。
- `imp::run`：阻塞运行当前线程的 run loop（阻塞相）：订阅的事件从这里投递进 `raw_callback`。只在系统层面停止投递时返回（如 tap 失效），没有停止 API——返回即手势失效，调用方按降级处理。
- `imp::raw_callback`：tap 回调：把订阅到的事件翻译成 `ButtonEvent` 交给 sink，未订阅的一律原样放行（listen-only 的 tap 也必须返回事件本身）。翻译只用常量查表加位置与时间戳各一次读取（不分配、不调用会 panic 的 API），panic 兜底在 sink 侧（见 `create` 的调用方保证）。
