# pipeline.rs 决策记录

## 桥接管缓存编排与 body 累积（M1，2026-09-26）

- 查/写主缓存（cache_key）从 core 的 AiTaskService 移到 run_task：命中直出
  TaskDone 不调引擎的判定属于“编排次序”，桥是唯一知道完整次序的地方。
- body 累积在转发回调里完成：TaskChunk 上抛与累积是同一点的两面，不另设
  通道。完成态以 TaskDone 的 outcome.body 为权威源——machine 侧的流式
  累积只服务显示，accept_done 整卡覆盖（两处注释互相指认）。
- 模态校验进桥前没有显式加 task.validate()：execute 内的渲染已先行校验
  （非法组合进不了引擎），而缓存 key 含 kind+input，非法组合不可能有历史
  命中；多一道校验是死代码面。M2 引入 Auto kind 后若校验前置点需要移动，
  再随 classify 编排一起改。

## start_command_runtime 增 cache 参数（M1，2026-09-26）

缓存实例由组装点（main.rs）构造后注入桥，服务构造（build_service）不再
持有缓存：这样 M2 的分类缓存第二实例有对称的注入位（同在组装点建、同交
桥编排），依赖方向保持 app → core 单向。

## EngineRequest.max_tokens 随 M1 预埋（M1，2026-09-26）

端口字段此值恒 None（任务执行不设限）；真正消费它在 M2 的 classify（极小
回复防长生成）。platform 侧 None 时不上送该键——部分严格端点拒绝显式 null。

## 提示直通的截断点（M2 评审补记，2026-09-26）

hint_kind（classify 模块单点）在桥里先于缓存查询与模型解析调用：带代码
语言提示的输入答案确定，不花缓存查找与快照取数的成本，也不发分类请求；
classify() 内部同一规则兜住“绕过桥直接调 classify”的调用方（M3 评测
重放会这么用）。

## 降级产物不投毒缓存（契约校验步骤 3，2026-10-11）

- `run_task` 的缓存写从无条件 `cache.set` 改为 `if !outcome.degraded` 一行分流：
  违约回复（validate_task Err 的降级产物）落 Show 卡但不进缓存——否则同
  input+options 一个 TTL 内重复触发命中同一份垃圾（方案 §0 的动机本身）。
- 降级后的重试路径复用既有失败重试机制：用户点重试 → 同代数原样重发 →
  重新执行 → 新产物再按同一分流判定。缓存命中路径不可能回放降级产物
  （它从未被写入），`TaskClassified+TaskDone` 的命中事件序因此无需分叉。

## 桥接线 on_repair（2026-10-12，契约校验与重试方案 §6）

- 为什么修复标记经通道④直发（`Event::TaskRepairing`）而不在主线程推导：修复轮边界只有 LLM 层知道（违约判定在 run future 内），主线程能看到的只有“chunk 停了”——用超时推导修复等于把契约语义压回计时；一个不带轮次的事件把边界显式化，陈旧过滤复用代数机制。
- 分类阶段的修复不发事件：分类发生在 `on_classified` 之前，此时尚无“流式卡”可清稿，发事件只会让状态机空转——分类对用户无感是方案定稿的可见度分工。

## 任务桥 catch_unwind 与 release panic=abort（P2-1，2026-10-13）

- 为什么 release 上 `panic = "abort"` 后任务桥的 `catch_unwind` 保留而不删除：
  release 下后台 panic 不再转 TaskFailed、进程直接崩（行为变更须在 PR 描述
  显式记录，开 PR 时落实；决策已确认属产品行为变更）；dev/test 构建仍是
  unwind（test 目标由 cargo 强制 unwind），
  “panic → TaskFailed + 循环存活”的降级语义照常并被
  `background_panic_becomes_task_failed_and_the_loop_survives` 这条 L1 单测
  锁定——删除即同时退化测试 profile。`#[cfg(panic)]` 按构建策略走双路径的方案
  也不取：两份代码只差一个分支，维护面翻倍而 release 无任何行为差。
- futures 依赖因此无特性可收窄：`FutureExt::catch_unwind`（本文件）在
  futures-util 0.3.34 里位于 `#[cfg(feature = "std")]` 之后，是 std 特性的唯一
  使用证据；`StreamExt::next`（update 的流式读取）无特性门、零特性可用——维持
  `default-features = false, features = ["std"]` 不动。

## start_command_runtime 的 worker_threads（2026-10-16，内存优化方案）

- 为什么钉在 2：消费桥串行消费通道③，同一时刻至多一个任务 future 在跑（分类/修复轮/传输都在它的 await 链上，密钥预热甩 blocking 池），第二个 worker 只为传输 future 与主循环的并发兜底；默认按核数起 worker 只多出线程栈与 per-thread 分配器缓存的常驻。

## SHUTDOWN_TIMEOUT / CommandRuntime

- `SHUTDOWN_TIMEOUT`：关停运行时的等待上限：正常退出路径里通道③已关闭、消费循环已在收尾，只兜底极端悬挂。
- `CommandRuntime`：托管 tokio 运行时与消费任务；drop 即关停（等待上限见 `SHUTDOWN_TIMEOUT`）。`shutdown_timeout` 消费 Runtime 所有权，Drop 里经 take 取出。

## TaskBridge（消费桥的接线面）

- 任务服务、缓存站点与密钥存储（资源），回传通道与主线程唤醒回调（泵的出口）。资源以计数指针共享，每任务克隆只递引用计数；`wake` 装箱为 trait 对象，每条回传事件一次间接调用（与 `EventSink` 的 wake 同款）。
- `new`：组装接线面：任务服务、缓存、密钥存储（`Arc<dyn SecretStore>`，2026-10-30 端口拆分后桥只持密钥端口；预热命令经它直查 keychain，与引擎/设置页共享同一实例与进程内缓存）、回传通道与唤醒回调（每条回传事件入队后调用，唤醒睡在主线程事件循环里的 UI）。

## start_command_runtime

- 创建 tokio 运行时并启动消费循环。消费桥同时只跑一个任务（分类、修复轮与传输都在它的 await 链上，密钥预热甩 blocking 池），两个 worker 足以覆盖并发面；默认按核数起 worker 只多出线程栈与分配器缓存的常驻。

## consume_loop（消费循环）

- 抽干通道③，逐条执行并回传。通道③全部 Sender 关闭（App 随事件循环结束 drop）后循环结束。span 来自触发点，进入它让缓存与引擎内部的日志自动带上代数；预热命令不带任务 span（不属于任何任务）。

## run_one（单条任务命令的执行体）

- 取消与执行竞速：取消即时生效，覆盖缓存查询与流式读取的全部 await 点；被取消的任务不发任何回传——App 取消时已 gen+1，迟到产物本就该被丢弃。
- 执行体包在 catch_unwind 里：unwind 构建（dev/test）下后台 panic 转 TaskFailed（错误卡给用户“服务异常”而不是永悬的推理中），循环自身继续消费。生效边界：release 以 `panic = "abort"` 构建时 panic 在事发点 abort 进程，`Err(payload)` 分支不可达——屏障仅为 unwind 构建保留。

## prewarm_secret（密钥预热旁路）

- 读一次密钥进进程内缓存（启动期预热，与任务代数无关）。读密钥可能被系统授权框长时间阻塞：甩进 blocking 线程池，消费循环继续接收后续任务——首条 RunTask 不能排在密码框后面。预热不占代数也不需要取消，失败只留痕（首次任务会自然重读并按既有失败路径兜底）。
- 任务路径自身的同步读密撞上同一未回填首读时没有这份待遇：它嵌在 async 块里、阻塞段不在 await 点上，select! 的取消分支对它不生效，阻塞上界是一次授权框应答（keychain 读的按 key 闸门收敛并发首读，不会叠加成多框）。

## run_task（单个任务的桥侧执行体）

- 缓存站点 + 纯泵：查缓存 → 命中直出（事件序与 miss 同形：`TaskClassified` → `TaskDone`）→ miss 则 `service.run` 转发（增量上抛）→ 完成态解析 → 非降级回填缓存 → `TaskDone`。`Err` 交给调用方转 TaskFailed——失败路径不写缓存，下次触发重新执行。
- 钩子把 LLM 层的中间产物逐条泵回主线程：分类定型（LLM 判定或失败兜底 kind）、流式增量与修复轮开始。原始回复的完成态累积归 LLM 层（run 的返回值），这里只泵不存——完成态解析以 output.raw 为唯一真相源。分类阶段的修复不进通道：判定要等定型才有，用户无感。
- 降级产物不投毒缓存：违约回复命中同 key 会把垃圾保留一个 TTL——只缓存校验过的正常产物，降级产物随事件上卡、过期即逝。

## deliver_cached / panic_detail / send_event

- `deliver_cached`：缓存命中的直出路径：补发分类事件再上产物，事件序与 miss 路径同形。
- `panic_detail`：从 panic payload 提取诊断文本（只认 `&str` / `String` 载体，其余没有稳定形状）；上限与 SSE 诊断一致——这段文本会进 UI 与日志。
- `send_event`：回传事件 + 唤醒主线程；接收端消失（应用退出）时静默丢弃。

### 语言降级标志与缓存口径修订（2026-10-28，输出语言体例门禁设计 §6）

- 桥侧把 `RunOutput.lang_degraded` 并进产物 `degraded`：格式降级（空注）与语言降级（内容完整）共用一个标志，徽标文案因此取泛化措辞（设计决策 9）。
- 缓存条件从“非降级才写”改为“格式合格才写”（`!outcome.note.is_empty()`）：语言降级产物内容完整、对用户仍有价值，按设计“修复后（或降级接受）的最终 TaskOutcome 入缓存，缓存命中不重放修复轮”照常入缓存；格式降级产物（空注）依旧不投毒。`note` 是否为空是 `complete()` 两分支的精确判别式（正常路径注恒非空、降级路径恒空），不新增字段。
- `a_lang_degraded_done_caches_the_full_outcome_for_replay` 锁定新口径：首轮 3 次引擎调用（分类、任务、语言修复）后缓存直出，二次触发不再触引擎。

## 违约清单随产物落缓存（降级批注方案 §2，2026-10-30）

- 语言降级置标记处顺带把 `RunOutput.lang_violations` 搬入产物——清单随完成
  产物恒存缓存（“恒存完成产物”语义内），缓存命中回放原样带回，回放点
  不为此重跑判定器（重判随规则表升级漂移）。契约无效直降产物清单为空且
  不进缓存，语义不变。
