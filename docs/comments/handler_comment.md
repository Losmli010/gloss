# handler.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），代码内不留指向本文件的指针。
> 2026-09-20 由 app_comment.md 按 R4 拆分迁入。

## ApplicationHandler::resumed（预热帧）

- 2026-09-18 浮层预创建即隐藏（Idle 态）仍要先在隐藏状态画一帧：egui 图集构建、Metal 管线编译与纹理上传都发生在首帧，不预热的话首次真实显示会超 100ms 预算（09 M1-T6）。

## ApplicationHandler::window_event（设置窗口失焦）

- 2026-09-18 设置窗口失焦保持打开（区别于浮层失焦即收）：编辑会话的草稿还在，失焦即收起等于丢掉用户没保存的输入，对人太狠；关闭/保存才作废草稿。
- 2026-09-21 T5 浮层常驻化后“浮层失焦即收”已删除：浮层失焦不再有任何处理臂（失焦不消失）；本条对照关系只剩设置窗一侧。浮层收起信号收口为 Esc / 关闭按钮，见 `is_dismiss_key` 条。

## is_dismiss_key（Esc 收起，2026-09-21）

- 2026-09-21 纯函数只吃 `(logical_key, state)` 两个事实，不整吃 `KeyEvent`：winit 的 KeyEvent 带私有字段（platform_specific），测试里字面构造不出来；收起判定本也只依赖这两个事实。Repeat 未单独过滤——Esc 长按连发的是幂等 dismiss（Idle 上 no-op），代价仅重复日志，见 overlay_comment.md 的 dismiss 条。
- 2026-09-21 “Esc 仅浮层持焦时生效”不靠代码而靠 winit 语义：键盘事件只投递给持焦窗口。这条平台前提 L1 测不到（handler 接线需要 ActiveEventLoop），落真机走查清单（PR #53 清单第 6 项）。

## ApplicationHandler::user_event（RedrawSettings 分臂，2026-09-27）

- 2026-09-27 `RedrawSettings` 只做“设置窗可见才 `request_redraw_settings`”并提前返回：来源是更新子系统的相位迁移（模块广播后投递），与主流程四通道无关，不抽干、不牵动浮层；可见性判定用 `WindowManager::is_settings_visible` 的 winit 事实，而非编辑会话是否存在——两者在关闭瞬间有短暂的错位窗。

## GlossApp::init

- 建窗口栈与两个窗口的帧状态，一次做完（呈现资源收进 workspace）。字体装载完成经 `UserEvent::FontsReady` 回主线程换表重绘。

## resumed 的启动序（除预热帧外）

- resumed 可能连续投递，渲染栈只起一次。
- 没有窗口与渲染栈就没有可做的事，带病进主循环只会静默空转——init 失败即 `event_loop.exit()`。
- 密钥预热是否此刻发送由 send_secret_prewarm 按启动权限预检结论门控：权限就绪才读——keychain 授权框不得抢在系统权限引导弹窗之前，未就绪则推迟到首次任务自然重读。
- 代码着色的语法集首次构造是百毫秒级，挪到后台线程，不占首个代码卡的首帧。
- 启动静默检查在此补发：更新客户端的信任链构建不挤占启动窗口。

## sooner

- 两个唤醒时刻里更早的那个；都没有则不用定时唤醒。

## settle_due_repaint（到点截止的处置）

- 可见时返回 true、由调用方请求重绘，截止留给随后的新帧刷新；隐藏时请求会被可见性门控丢弃，返回 false 并把截止一并清空——过期截止不清，`about_to_wait` 会一直 `WaitUntil` 过去时刻，定时器立即重触发且永远画不出那一帧，调度层空转烧满一核。

## user_event 的 FontsReady 分臂

- 字体装载完成：按当前主题强制重施加（含完整字体表）并重绘浮层。主题在这里统一施加，装载窗口内的偏好变更不会被后台快照覆盖。设置窗开着时同样要重绘：换表使排版缓存失效，不重绘它就停在内置字形的画面上直到下一次交互。

## window_event 的重绘路由

- 浮侧重绘走带可见性门控的入口：隐藏浮层的事件驱动重绘是整帧空转。

## new_events（到点分发）

- 到点的是 egui 要的下一帧：浮层与设置窗各自的截止时刻独立检查。

## about_to_wait

- 没有待处理的唤醒时刻就彻底睡下，等窗口事件或唤醒句柄把自己叫醒。

## egui_wants_repaint

- 其余事件先喂给对应窗口的 egui，它决定是否消化掉以及要不要重绘；帧未建（设置窗首帧前）时没有可喂的对象，事件回落到调用方的状态处理（关闭请求等不依赖帧）。

## apply_window_state_event

- 依赖窗口身份的状态类事件：设置窗关闭是“取消编辑”（隐藏丢弃草稿，进程照常），浮层收起键只观察不拦截，尺寸变化落到对应 surface。
- KeyboardInput 臂：egui_winit 已在同一事件上喂过 egui（输入框等自行消化）；壳只观察收起键，不拦截事件。

## is_dismiss_key（收起键的窗口语义）

- 浮层的收起键：Escape 按下（含重复）。键盘事件只会送进持焦窗口——浮层失焦后 Esc 天然不再生效；设置窗持焦时不收浮层。

## settle_due_repaint：到点截止在隐藏时必须随丢弃一并清除（2026-10-16，PR 修复 cpu_idle_ms 29850 ms）

趋势点 50f6931（采集 run 38878042854 前后）的 `app.cpu_idle_ms` 从历史 30–250 ms
飙到 29850 ms（30s 空闲窗口烧满一核）。取证结论（诊断分支 diag/runner-idle-cpu，
runner 连采 8 实例 7 个开机即热 + 符号采样）：

- #198 引入“隐藏浮层不请求重绘”门控（`Workspace::request_redraw` 的
  `is_visible` 检查）。启动预热帧在隐藏态画渲染自检卡，淡入动画请求 16ms
  重绘，截止写入 `overlay_repaint`；截止到点时重绘请求被门控丢弃，截止无人
  清除，`about_to_wait` 恒定 `WaitUntil` 过去时刻，winit 的 CFRunLoopTimer
  立即重触发——调度层空转，永远画不出那一帧。实例间是否触发取决于预热帧
  egui 时钟与 0.18s 淡入时长的相对时序（0.2.7 无门控不触发）。
- 备选方案：`about_to_wait` 过滤已过期截止。否——语义含混：到期未处理的
  截止对可见窗口是“该画未画”的真实信号，静默过滤会掩盖丢失帧；清除的
  正确位置就是“请求被门控丢弃”这个事实发生的地方。所有显示路径都显式
  请求重绘，清掉隐藏期的过期截止不吞任何该画的帧。
- `settings_repaint` 不需要同款处置：`request_redraw_settings` 无门控，
  设置窗截止总被自己的新帧刷新。

## user_event 的 OpenSettings 分臂（2026-10-29）

- 应用菜单的设置项经 `UserEvent::OpenSettings` 进壳，分臂直通 `open_settings()`：与浮层齿轮、失败卡按钮共用同一打开入口（会话已开则只前置聚焦）。事件循环退出前送达的事件照常走代理的 `send_event`，循环已关则静默丢弃（菜单点击时进程在跑，实际不触发）。
- `init` 末尾追加 `install_app_menu`：事件循环已启动、缺省菜单已装（或已被 `with_default_menu(false)` 关掉），此时装配本地化菜单时机安全。
