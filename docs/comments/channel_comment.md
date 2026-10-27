# channel.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关符号时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## PlatformEvent::PasteboardImageObserved（2026-10-10，剪贴板图片方案 §5.2/§5.6）

- 为什么无载荷：①通道的语义是「观察到有新图片」这一元数据事实（`changeCount` 前进且类型含图像），内容本身在②`AcquireClipboardImage` 命令到达事件线程时再读——①②之间剪贴板被覆盖的竞态由取材侧兜底，若①带内容就要在哨兵里做昂贵的图像拷贝（每 250ms 一次轮询的常驻成本），还把「读了不用」的浪费变成常态。
- 为什么 `PasteboardImageObserved` 进 `trigger_decision` 的场景闸门而不是 `Unwired` 直通：自动触发的暴露面比手势大（用户没有「正在触发」的心理预期），敏感输入态/敏感应用下的剪贴板图片同样不该出网，闸门语义与划词同权；warn 留痕复用既有「为什么划了没反应」的排查口径。

## AcquireCommand::AcquireClipboardImage（2026-10-10）

- 为什么镜像 `AcquireText` 的形状（只有 `generation`）：取材命令的契约是「代数 + 读什么」，不携带任何内容或选项——图像的编码参数（RGBA→PNG、上限检查）都是事件线程侧的实现细节；带 `rect` 之类载荷的只有 `CaptureRegion`，因为区域是触发方才知道的输入。
- 本步骤只加变体不接读取栈（`acquire_command_handler` 对它按未接线丢弃留痕）：读图（arboard `image-data` + RGBA→PNG）是 platform 适配器工作，归方案第 4 步；通道变体先行是为了状态机与编排测试能以公共 API 全时序驱动（探测入口 → ②命令 → ④产物）。

## PlatformEvent（通道①的变体语义）

- `SelectionGesture`：划词手势（文本任务），载荷是释放坐标（系统全局坐标，逻辑点）——浮层跟随划词位置的输入。
- `OpenSettingsRequested`：托盘请求打开设置。
- `AccessibilityGranted`：运行中观察到取材授权（辅助功能）落定——仅启动预检未就绪的会话由授权观察器产出一次（事件线程节流轮询，见 platform 的 `AccessibilityWatch`）；不占用代数（与设置入口同为不进状态机的直通事件）。

## AcquireCommand（通道②：线程亲和）

- 取材有线程亲和约束（选区读取与剪贴板图片读取是同步系统调用，必须发生在事件线程），不能与通道③合并发去 tokio；与系统事件同线程顺序消费，天然串行无锁。
- `AcquireText`：读前台应用选区文本。
- `generation()`：请求代数是两种取材共有的唯一字段（壳侧派发与追踪都按它核对）。

## Command（通道③）

- 用 tokio unbounded mpsc：消费端 async `recv().await`；主线程侧 `send` 永不阻塞、也不需要定容量策略——命令是轻量枚举，产量受用户手势限制。
- `RunTask`：执行一条任务：取材输入 + 触发时冻结的选项。任务类型不在其中——分类是 LLM 层（`AiTaskService::run`）的职责，桥与状态机都不预设 kind；判定的类型经事件④的 `TaskClassified` 与 `TaskDone` 回传。`options` 是触发时按配置快照冻结的任务选项（含模型 id）；`cancel` 由 App 为每次任务创建并 clone 下发，取消 = 调 `cancel()`。
- `PrewarmSecret`：读一次当前 provider 的密钥（启动期预热）：真实读 keychain 首次访问会弹系统授权框，前置到启动授权引导之后让弹窗受控出现；读到的值进存储的进程内缓存，首次划词不再弹。只带条目定位符——密钥值永不进通道载荷。

## Event（通道④）

- crossbeam MPMC——事件线程与 tokio 各持一个 Sender；主线程在渲染循环里 `try_recv` 非阻塞消费。
- `InputReady`：取材产物，触发→取材→推理链路的枢纽。
- `TaskClassified`：自动分类完成：本次任务判明的类型（含提示直通与失败兜底）。流式视图据此精化排版（代码解释的判定从首帧就位），执行由 LLM 层继续。
- `TaskChunk`：流式增量（markdown 正文片段）。
- `TaskRepairing`：修复轮开始（任务回复契约违约后的重发）：UI 清掉上一轮的流式正文并把页脚切到「重新注解」。不带轮次——界面无计数，轮次只在 core 日志；分类阶段的修复不进通道（分类对用户无感）。
- `TaskFailed`：失败原因，状态机据此分支重试策略。
- `SecretPrewarmed`：密钥预热完成（成功与否，不含密钥值）：失败不拦主流程——首次任务会自然重读并按既有失败路径兜底；`Err` 只携带失败原因（如用户拒绝授权）。

## Traced

- span 在触发点创建（代数在那里赋值），随通道②③下发到平台事件线程与 tokio——接收侧进入它，范围内的日志自动带上 `generation`（含取材读选区、推理引擎这些拿不到代数的深层日志点）。
- `untraced`：不带 span 的载荷：测试与不属于任何任务的命令用（进入空 span 是空操作）。

## CrossbeamPair

- ①②④ 共用这一选型（MPMC + 主线程 `try_recv` 非阻塞）。

## CommandChannel

- 通道③的 tokio half：Receiver 移交 tokio 消费任务，Sender 留在主线程。

## Channels

- 四条通道的组装点：入口 `main.rs` 创建一次并分发——① 的 Sender 交给平台事件源；② 的 Sender 留在 App；③ 的 Receiver 移交 tokio；④ 的 Sender 克隆给事件线程与 tokio 各一。

## AppEndpoints

- App 侧持有的通道端点：① 收平台事件、② 发取材命令、③ 发推理任务、④ 收回传事件。组装点创建 `Channels` 后拆出移交；其余端点归事件线程与 tokio。
