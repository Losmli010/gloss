# events.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），代码内不留指向本文件的指针。
> 2026-09-20 由 app_comment.md 按 R4 拆分迁入。

## Waker（推送唤醒 vs 轮询）

- 2026-09-18 用 `EventLoopProxy` 推送唤醒，不用主线程 `try_recv` 轮询（08 §7.3）：轮询只能在 winit 因别的事件醒来时顺带取消息，空闲时最坏延迟一帧，且要求主线程周期性空转；代理唤醒由发送方立即触发，无消息时主线程可以一直睡。被否备选：固定间隔轮询通道——空闲功耗与唤醒延迟都更差，还要为“睡多久”凭空定一个数。

## run 的入参（任务 14，2026-09-24）

- `run(...)` 在 `hotkeys` 与 `system_locale` 之间多收一个 `scene: Arc<dyn SceneProbe>`
  并原样转给 `GlossApp::new`。位置放在两个已有适配器之间（而不是塞到末尾）：
  `endpoints` 之后的这一串全是适配器，`system_locale` 是配置性质的标量，
  按“适配器成串、标量在后”排才不至于下次再加一个适配器时又要两头找位置。

## UserEvent::RedrawSettings（相位迁移的独立唤醒事件，2026-09-27）

- 2026-09-27 为什么不复用 `Wake` 而加独立变体：`Wake` 的语义是“主流程有消息待抽干”并附带浮层重绘；更新子系统的相位迁移只牵动设置窗，不该让主流程通道白抽一遍、浮层白画一帧。独立事件让 `user_event` 分臂处理：`RedrawSettings` 只在设置窗可见时请求其重绘（隐藏窗口的重绘请求是纯浪费），不抽干任何通道。

## UserEvent（变体语义）

- `Wake`：有跨线程消息待处理。
- `RedrawSettings`：设置窗口内容自外部变化（更新子系统相位迁移）：窗口可见时请求重绘（详见上节）。
- `FontsReady`：系统字体装载完成：换完整字体表并按当前主题重施加，浮层请求重绘。

## Waker::wake / wake_settings

- 唤醒主线程；返回 `false` 表示事件循环已退出。
- `wake_settings` 唤醒主线程请求设置窗重绘（设置窗内容自外部变化时用）；返回 `false` 同义。

## run（启动事件循环的主入口）

- 启动事件循环，直到退出才返回。
- `endpoints` 是 App 侧通道端点（① 收平台事件、② 发取材命令、③ 发推理任务、④ 收回传事件），由组装点拆出移交；`config` 是运行时配置句柄，在每一批平台事件的起手处取一份快照交给状态机（见 `GlossApp::drain_platform_events`），配置热更新因此无需重启；`store` 是密钥存储（2026-10-30 端口拆分后 App 只持 `SecretStore`），设置页经它写 keychain（密钥不经快照，也不进句柄）。
- `on_waker` 拿到唤醒句柄——`main.rs` 是唯一组装点，句柄要由它分发给平台事件线程与 tokio，库这边不替上层决定跨线程拓扑。回调返回 `Err` 时事件循环不启动，错误原样上抛——消费运行时（通道③的消费者）装配失败属于启动硬错误：继续带病运行会让用户触发的任务静默石沉大海。
- `theme` 施加到各 egui 上下文（见 `Workspace::apply_theme`）。`system_locale` 是组装点读到的系统语言，供配置里的 `Language::System` 落定成 [`Locale`]（prompt 模板与界面文案共用）。`scene` 是触发前场景探针（安全输入态、前台应用），供敏感信息防护的场景闸门判定——同样是平台适配器的事，壳只消费。`permissions_ready` 是取材授权“辅助功能”的门控：启动时刻取预检结论，未就绪的会话在运行中授权落定后置位（见 `GlossApp::on_accessibility_granted`），密钥预热据此门控（见 `GlossApp::send_secret_prewarm`）。
- `update` 是更新子系统的壳侧接线（组装点经 `update::start_once()` 建立，见 [`UpdateWiring`]）：设置页每帧读 watch 通道里的 [`crate::update::state::UpdateState`] 渲染，用户动作经出口转投模块——与主流程四通道完全隔离。
- `clipboard_watch_enabled` 是剪贴板图片哨兵的共享开关位（组装点按启动快照创建、与事件线程的哨兵源同份）：App 在设置页保存成功时置位，热切换不重启（见 `Env` 的 `clipboard_watch_enabled` 字段）。

## UserEvent::OpenSettings 与缺省菜单关闭（2026-10-29）

- 新变体 `OpenSettings`：应用菜单设置项的唯一回程。菜单回调在窗口库事件循环内触发（主线程动作），直接 `send_event` 进用户事件通道，不走平台事件通道——后者是为事件线程装配的回程，菜单安装点手里只有代理。
- 事件循环构建加 `with_default_menu(false)`：窗口库的缺省菜单词条硬编码英文，先关掉再由应用菜单模块整体装配本地化菜单，免得“英文菜单闪一帧再换”。
