# app/mod.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关代码时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），代码内不留指向本文件的指针。
> 2026-09-20 由 app_comment.md 按 R4（app.rs 拆分为 app/ 子模块）拆分迁入。

## GlossApp（双窗口 repaint 机制）

- 2026-09-18 浮层与设置窗口各有独立的下一帧截止时刻（`overlay_repaint` / `settings_repaint`），不能共用一份：两个窗口是两个独立 viewport，egui 各自给出 repaint_delay（动画如光标闪烁、进度指示各按各的节奏）；共用字段时任一窗口画一帧都会把另一个窗口的截止时刻冲掉，表现为另一个窗口动画停格到下一轮事件。
- 2026-09-18 唤醒时刻的归并在 `about_to_wait`：取两个 repaint 截止与 auto_hide 三者更早者设 `ControlFlow::WaitUntil`，都没有则 `ControlFlow::Wait` 彻底睡下，等窗口事件或唤醒句柄叫醒。`new_events` 的 ResumeTimeReached 到点后按各自的截止时刻分别请求重绘（到点的可能是自动隐藏、某一窗口的下一帧，或两者同时）。
- 2026-09-18 设置窗口关闭时把 `settings_repaint` 置 `None`：窗口收起后没有动画可画，不为它唤醒事件循环（空转耗电）。

## GlossApp::new 的 system_locale 字段（R1，2026-09-22）

- 为什么系统语言由组装点经 `GlossApp::new`/`app::run` 传进来，而不是在壳里调 `gloss_platform::locale`：组装点唯一约束（AGENTS.md）——壳消费适配器给的值，不自己去碰平台。`main.rs` 顺手把读到的值记一条启动日志，真机走查可直接看到落定结果。
- 为什么存成字段而不是每次触发现读：系统语言是进程级常量（改了要重启），存一次即免掉重复的系统调用；`drain_platform_events` 每批事件把它作为第三参传给状态机（见 machine_comment.md 为何走参数）。
- 为什么它不是「第二条热更新通路」（owner 质疑，2026-09-22）：配置热更新那套（`ConfigHandle` 先落盘再换快照）**原样没动**——`language` 三态照样走它，用户切 English 就是 `save` 换快照、下次触发读新值。`system_locale` 不参与这条生命周期，它是 `Language::System` 这个**未求值表达式的求值输入**，与「用户改了什么」无关。判据三条：不来自 UI（没有控件能改它）、不落盘、进程内常量。
- 为什么不把 `System` 在保存时解析掉再落盘：那样「跟随系统」就成了伪选项——配置里固化的是一个具体语言，换机器、改系统语言都不会跟着变，UI 下拉里那一项也失去了意义。
- 为什么不塞进 `Config`（哪怕 `#[serde(skip)]`）让 `trigger` 只吃一份快照：① 设置页保存是**整份替换**（编辑副本由 `ui::settings::open(&config.snapshot())` 构造，环境字段不在 UI 的编辑模型里），每次保存都会把它冲掉，除非把环境事实一路拖进 UI 层；② `Config::default()` 在测试里构造得到，得给它编一个假系统语言，等于把环境事实悄悄常量化成「中文」；③ `ConfigHandle::save` 的不变量是「磁盘是唯一真相」，加一个从不落盘的字段会让这句话在类型上不再成立。
- 为什么不做进程级全局静态（`OnceLock<Locale>`）：core 不得依赖平台栈（依赖方向红线），而静态可变状态是隐藏依赖——测试里没法为不同 locale 造出不同实例，并行测试互相污染。
- 为什么不做端口（`LocaleProvider` 注入 core）：为一个 `Copy` 的二态枚举引入 trait 对象与动态派发，且 `machine` 会从纯函数变成带 IO 依赖的组件，它现有的测试全都要装 stub。当前是**值注入 + 纯映射**：`Language::resolve(system)` 是纯函数，`machine` 保持可测（一条 `prompt_locale_follows_config_language_and_the_system` 覆盖三种 Language × 两种系统语言）。
- 代价要认：`main.rs → app::run → GlossApp::new → drain_platform_events → machine.trigger` 是 5 层参数穿透，穿的是一个 `Copy` 枚举。可接受的理由是它与 `config` 走**同一处、同一时刻**（`drain_platform_events` 批首取快照、紧接着同一行传参），「一个任务一个一致视图」因此天然成立；若它走别的通路（全局静态、独立端口），反而要额外论证「配置是新的、环境是旧的」这种撕裂不会发生。
- 什么时候该换方案：一旦系统语言要支持运行期变化（不重启即生效），它就从「启动期常量」变成「会变的外部事件」，正确形态是让它成为平台事件源（进 `PlatformEvent` 或在壳内做可更新的 `Cell`），仍然不该进 `Config` 快照——因为它永远不会被用户保存。

## GlossApp::locale / draw / draw_settings（任务 09，2026-09-22）

- 为什么 locale **逐帧**从快照解析（`config.snapshot().language.resolve(system_locale)`）而不是在
  `GlossApp::new` 时算好存字段：语言要保存才生效（与 `theme` 同一条路径，`applied_theme` 也是
  逐帧读快照），而快照本身是处处可见的——存字段就多一份必须与快照同步的状态，忘记同步的表现是
  「保存了语言，界面还是旧语言」。逐帧解析的代价只是一次 Arc 读 + 一次纯映射。
- 为什么两个窗口各取一次而不是共用一份：它们在同一帧内取同一份快照，取值必然一致；共用反而要为
  「谁先画」排序。
- 为什么「即时生效」的边界是**保存之后**（而不是窗内改下拉即时预览）：与主题一致——设置窗的草稿
  不驱动渲染，保存成功才换快照（且保存成功即关窗）。窗内预览属于另一套语义（草稿驱动渲染），
  两处不一致比都不预览更难解释。真机验收口径因此是「保存后无需重启，浮层与设置窗都是新语言」。
- `draw`/`draw_settings` 在拿 `frame` 的可变借用**之前**先算 locale：`self.frame` 借用与
  `&self` 上的方法调用不能共存，而 locale 是 `Copy`，先算后借即可（同一个坑在 `apply_theme` 里
  是先调用、因为它不借 frame）。

## R4 拆分（模块组织）

- 2026-09-20 app.rs 按职责拆为 app/ 内部子模块（events/render/handler/channels/overlay/settings_session/theme），`GlossApp` 结构体与壳级编排（draw/draw_settings）留在 mod.rs。跨子模块的关联函数与 `impl` 方法一律 `pub(super)`（可见面=crate::app 子树），不给 crate 外扩权；测试辅助（driven_app 家族）收进 `#[cfg(test)] pub(super) mod test_support` 供各子模块测试共用。组装点唯一约束（main.rs 的适配器注入）不受影响——拆的是 app.rs 壳实现内部组织，公共 API（run/Waker/UserEvent/Frame/build_window_stack/render_frame*/centered_position）经 mod.rs 再导出保持原路径不变。

## 场景探针的注入（任务 14，2026-09-24）

- `GlossApp` 多一个 `scene: Arc<dyn SceneProbe>` 字段，与 `store` / `hotkeys` 同性质：
  都是**适配器**，由 main.rs 这一处组装点注入，壳只持有 trait object（见
  「组装点唯一约束」）。壳不认识 `SystemSceneProbe`，因此单测可以换桩。
- 构造与 `test_support` 的助手链一并扩了一档：`driven_app_with_scene(store, hotkeys, scene)`
  成为唯一的底层构造函数，`driven_app_using` / `driven_app_with` / `driven_app` 逐层
  往上前填默认桩。这样「加一个适配器」在测试侧只是多一层默认值，不必改任何
  既有用例的调用点——该模式是 R4 拆出 test_support 时就定下的。

## Env（外部服务句柄聚合）

- 配置、存储、场景探针、系统语言、取材授权门控、更新接线。句柄本身进程内不变（配置内容经句柄热更新；授权门控按运行中授权落定置位，见 `permissions_ready`）。
- `config`：运行时配置句柄：每批平台事件取一份快照交给状态机，配置保存后无需重启即对下一次触发生效。
- `store`：配置存储：设置页写 keychain 用——文档半边走句柄，密钥半边不进快照也不进句柄，经这里直查。
- `scene`：触发前场景探针：安全输入态与前台应用由它现读，壳只把它转交状态机作场景闸门判定（见 `drain_platform_events`）。
- `system_locale`：启动期读到的系统语言：配置里的 `Language::System` 靠它落定成具体的界面语言与 prompt 模板语言（进程内不变，改系统语言要重启）。
- `permissions_ready`：取材授权（辅助功能）的门控：启动时刻取预检结论；未就绪的会话由授权观察器在运行中落定后置位（见 `GlossApp::on_accessibility_granted`）。密钥预热据此门控——未就绪意味着系统权限引导弹窗在途，预热读会让 keychain 授权框抢到它之前。
- `update`：更新子系统的壳侧接线：设置页每帧读其 receiver 渲染，用户动作经出口转投模块（与主流程四通道隔离）。
- `clipboard_watch_enabled`：剪贴板图片哨兵的共享开关位：组装点创建、与事件线程的哨兵源同份。设置页保存成功时置位（热切换不重启），落盘失败不置位——与运行时快照同进退。

## Env::locale / target_theme

- `locale`：当前界面语言：配置里的三态偏好按启动期系统语言落定（与 prompt 选表同一处取值）。逐帧从快照取——设置页保存后下一帧即换文案表（逐帧取值的理由见「GlossApp::locale / draw / draw_settings」节）。
- `target_theme`：配置快照里的主题偏好：建帧（上下文建立时装入）与逐帧施加共用这一处取值。

## GlossApp 字段（组合根的职责划分）

- 每个所有者各持一类职责——任务决策在 `machine`、管线会话状态在 `session`、呈现资源在 `workspace`、服务句柄在 `env`；本结构体自身只剩绘制调度与设置窗编辑草稿。
- `workspace`：呈现资源：窗口管理器、两窗口渲染帧、各自的下一帧时刻、已施加主题。
- `endpoints`：组装点移交的通道端点（① 收、② 发、③ 发、④ 收）。
- `machine`：任务状态机（functional core，见 machine.rs）：纯状态转移，壳只做通道发送、浮层窗口操作与日志。
- `session`：触发/任务的管线会话状态（锚点、排查线索、显形挂起、任务 span）。
- `settings`：设置窗口的编辑会话；窗口可见时有值，关闭/保存完成即清（草稿随之丢弃）。
- `proxy`：自定义事件的投递端：后台线程（字体装载）向主线程发完成信号用。测试驱动（不经 run()）为 None——那条路径不触发字体装载。

## GlossApp::new

- 组装点移交的通道端点与外部服务句柄；窗口与帧状态在 `resumed` 时建立。`system_locale` 同样来自组装点（系统语言是平台适配器的事，壳只消费）。

## GlossApp::span_for

- 指定代数的任务 span（副本，供 `enter()` 借用）；代数不符或尚无任务时为 `None`。

## GlossApp::draw（浮层帧管线）

- 画一帧：egui 出绘制数据 → wgpu 呈现，并把 egui 要求的下一帧记下来；浮层上交的动作（失败卡与头部动作区）就地执行；页头拖动按指针累计位移换算窗口落点；浮层内容的期望尺寸就地应用（内容自适应高度，窗口管理器按显示器钳制）。
- 页头拖动：落点以窗口当前实际位置为基准（无增量记账，见 apply_overlay_drag），拖动中的每帧按需平移。
- 尺寸应用：流式期间走防抖尺寸（锁宽 + 步进增高），其余状态按精确尺寸重排——TaskDone 的定型重排也走这一支。

## GlossApp::draw_settings（设置窗帧）

- 画一帧设置窗口：草稿编辑 + 动作上交（保存/密钥变更/取消/更新动作）。渲染帧在这里惰性建立（首次绘制前），建帧失败即关闭设置窗——空窗挂着只会让用户对着一堵白墙重试。
- 取一份快照而非在整帧渲染期间持有 watch 读锁：更新任务写状态时不必等 GPU 帧画完。代价是每帧一次 `UpdateState` 克隆（`version` 为 `Some` 时有一次堆分配）——只在设置窗绘制帧发生。

## `pending_reveal` 字段文档里的「M4-①」括注移除（PR #72 评审，2026-09-27）

「M4-①」曾指开发计划里的「触发即显」条目；dev-plan 不在仓内，README 的
M4 指「配置与设置」里程碑，子编号 ①/② 仓内无定义，标记无法解析。字段的
行为描述（占代数的触发置位、下一次 drain_events 消费、显形判定在
`overlay::should_reveal`）保留。

## permissions_ready 字段注释（2026-10-09）

- 2026-10-09 字段语义从「两项授权的合取」改为「辅助功能预检结论」：输入监控引导移除后（见 permissions_rs_comment.md），组装点不再计算合取，门控含义不变——未就绪 ⇔ 系统授权引导对话框在途，密钥预热据此推迟。
