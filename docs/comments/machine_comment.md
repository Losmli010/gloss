# machine.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关符号时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## 三段式探测—确认—提交（2026-10-27，发送前确认方案）

- **问题现场**：两段式（见下节）把“误滑”挡在了取材之外，但取材产物一到
  达就立即下发推理——用户手一抖划到一个词，推理与 token 消费已经出发，
  取材成功与推理开始之间没有用户拍板的缝隙。
- **方案**：提交段再拆两步——`commit_probe` 只落待确认卡（
  `OverlayView::Pending { input, options }`，投影为 `PendingConfirm` 态），
  用户点“注疏”主按钮才经 `confirm_pending` 组装下发请求（壳侧
  `OverlayAction::Confirm` → `send_run`）；取消钮/Esc/关闭走既有
  `hide_overlay`。
- **为什么待确认态没有令牌、确认才领新令牌**：`commit_probe` 落卡时
  `current_cancel` 保持 `None`——待确认阶段没有在途推理，通道三零流量
  （壳侧测试 `the_pending_card_cancel_path_sends_no_channel3_message`
  钉住“取消路径不惊动推理侧”）；确认下发才由 `begin_run` 换新令牌。
  旧会话若仍在推理，`commit_probe` 仍取消它（让位语义不变——旧流的结果
  不可能被确认卡采纳）。“Some ⟺ Translating”不变量因此保持成立：
  `PendingConfirm` 与 `Idle` 同为无令牌的态。
- **为什么确认不回读配置**：待确认卡持有 `input + options` 的冻结快照，
  确认只是原样放行——从探测到下发，配置只读一次（探测时刻），
  “单次任务一套参数”的口径跨过确认环节不破（
  `confirm_pending_carries_the_frozen_options_snapshot` 钉住）。
- **守卫形状与 `retry` 同款**：`confirm_pending` 只认 `Pending` 视图——
  流式中、产物卡上、空白机器与已收起的卡上一律 `None`。确认恒作用于
  屏上当前卡（守卫读视图这一唯一事实源，不按卡记身份）：新提交顶掉旧卡
  后确认自然落在新卡，同一张卡只可确认一次（
  `a_new_commit_supersedes_the_pending_card_and_confirm_acts_on_the_current_card`
  钉住）。
- **代数语义不变**：探测编号仍在提交时提升为代数，确认**不动**代数——
  回传过滤、陈旧判定与两段式完全一致。
- **代价（明示接受）**：真实意图的推理比两段式晚一次点击；误划产物不再
  自动连坐一次推理与 token 消费。显形时序不变：提交即置 `pending_reveal`，
  用户看到的是待确认卡而非骨架。

## trigger_decision：触发去向（2026-09-23，2026-09-24 扩为三路）

- 原形态：`acquire_command_for` 一个 `match` 里混着两种 `None`——“事件还没
  接线”与“kind 被任务开关停用”，壳只能记一行 debug：“not wired yet, or
  its task kind is disabled”。默认日志级别（info）看不见 debug，于是“划了
  词毫无反应”既没有浮层、也没有痕迹，用户报障时无从查起（owner 真机走查
  的实际报障）。
- 拆法：`trigger_decision` 只做分类（含划词 kind 按取材源收口成文本类那一
  步，以及 2026-09-24 加进来的场景闸门），取材料理按
  `TriggerDecision::Acquire(kind)` 放行，壳按 `Disabled(kind)` 记 warn、按
  `Blocked(block)` 另记一行 warn。判定因此只有一处，每条日志各自准确——壳
  不再自己重解一遍事件。
- 为什么 `Disabled` 值得 warn、而未接线仍是 debug：前者是用户自己能修的
  配置问题（默认任务被停用 = 划词手势永久无声，或热键绑定的 kind 被关掉），
  后者是开发侧进度（框选路径没接），用户无动作可做。级别差异就是“这条
  日志该给谁看”的答案。
- 为什么把分类做成 `pub` 而不是让壳传一个 `reason` 参数给 `trigger`：状态机
  的入口签名（`Option<AcquireCommand>`）保持不变，全部既有调用点与测试不
  动；分类是纯函数，壳与状态机读到的是同一份结论。
- 2026-09-24 加 `Blocked` 而不是复用 `Disabled`：两者对用户的含义完全不同——
  `Disabled` 是“你去设置里把这个任务打开”，`Blocked` 是“这一处我们就是不
  读”。日志与因由文案都按这个分野写；若合成一路，用户会去设置里找一个不
  存在的开关。
- 2026-09-24 场景闸门排在 `is_kind_enabled` **之后**：被停用的 kind 本来就不
  取材，先报 `Disabled` 才能给出用户可修的那条提示；反过来先判场景，用户
  就会在两处都可能是原因时先看到“被拦”这个他改不了的结论。

## 划词探测—提交两段式（2026-10-01，弹窗快速关闭终修）

- **问题现场**：用户报“划词后弹窗快速消失”。日志实证：2026-10-01
  20:28–20:32（本地）连续 14 次划词中 11 次在取材开始后恒 ~400ms（=360ms
  让渡预算耗尽）被 `dismiss_overlay("empty selection")` 收回。前台应用对
  `kAXSelectedTextAttribute` **稳定**返回空——不是 2026-09-30 那次“还没写
  好”的竞态（让渡补读已修），是无障碍树未激活/不暴露选中文本的应用根本
  读不到。真实划词被“误滑静默收回”分支吃掉：先显骨架再收回，就是用户
  看到的闪现。
- **用户的三条行为裁决**：①取材正常走完（AX 让渡 → 剪贴板兜底）；
  ②误滑以“取材无内容”为判据，全程不出窗；③弹窗已正常显示时，误滑
  不得扰动当前内容。第三条否定了“触发即接管”：旧 `trigger` 一到手势就
  取消在途任务、换骨架视图，破坏已造成，事后无从恢复——取材必须挪到
  状态机之外，产物到达才“提交”。
- **修在哪一层**：划词改走 `begin_selection_probe`（探测段：只领编号、
  冻结配置、发取材命令，状态机零改动）+ `commit_selection`（提交段：
  编号提升为代数、旧会话让位、视图换流式卡）+ `commit_selection_failed`
  （空选区静默丢弃 / 权限缺失落卡）。`trigger` 收窄为热键即显路径。
  热键是显式请求，原语义（立即骨架、失败弹卡）原样保留。
- **编号不复用**：代数与探测编号共用单调计数器 `next_id`（`trigger` 与
  探测各领一个号），提交只是把探测编号**提升**为代数。若沿用
  “generation+1”作探测编号，热键在探测在途时触发就会发生编号碰撞
  （热键占走同一个号，迟到的探测产物被误当当前会话采纳）。陈旧过滤
  语义不变：可见会话按 `generation` 过滤，探测产物按“在途探测编号”
  过滤，两套互不串扰。
- **内容闸门在划词路径的新语义**：命中敏感闸门时探测作废但**当前显示
  保留**（旧热键语义是回 Idle 清视图）。理由：可见内容属于上一个会话，
  误滑与敏感内容都不该把它顶掉；一条 warn 仍是“到底发出去没有”的
  日志证伪点。热键路径的旧语义（Fetch 中被拦 → 收起浮层）保留。
- **显形时序**：挂起显形请求（`pending_reveal`）现在有两个置位点——热键
  触发（骨架即显）与划词提交（内容到达才显形）；`drain_events` 把
  `mem::take` 挪到批处理**之后**，同帧内提交的显形当帧消费，不拖帧。
  误滑因此从结构上不可能再出窗：没有置位点、没有失败卡、没有收回。
- **代价（明示接受）**：误滑会在后台注入一次 Cmd+C（事件线程最长 ~4.2s，
  与既有兜底同阻塞语义）；真实二次划词的旧流在被提交替换前多跑约一个
  取材时长（少量 LLM token）；划词取材期无骨架反馈（AX 正常 ~10ms 无感，
  盲区应用静默 ~0.5–0.9s 后直接出结果）。

## （已废止）accept_failed 的静默收回与触发源（2026-09-30，2026-10-01 迁出）

- 当时的裁决仍然成立：**手势无显式意图，弹“读取失败”卡是打扰；热键是
  显式请求，沉默才让人困惑**。但实现位置变了——静默分支已从 `accept_failed`
  迁至 `commit_selection_failed`（划词探测改走两段式，见上节），理由是
  “触发即接管”会先破坏可见会话，误滑零干扰要求取材发生在状态机之外。
- `TriggerSource` 随分支一起删除：静默与否不再由“当前代的触发源”决定，
  而由“失败来自哪条路径”决定——热键路径的失败永远弹卡，划词探测的
  空选区永远静默。`accept_failed` 只服务热键（Fetching）与推理
  （Translating）。
- `SilentlyDropped` 的壳侧动作随之变化：从“收起已显形的骨架窗口”变为
  “什么都不动、只记一条带前台应用的 info”——探测段从未出窗，却可能在
  误滑时顶掉可见内容，旧动作恰恰是要禁止的。

## InputOutcome::Blocked 与内容闸门（任务 14，2026-09-24）

- 命中即**丢弃这次取材**：任务不组装、不下发、浮层不露面。初版是“新驻留态
  `AwaitingConfirm` + 确认卡 + 仍要翻译”，2026-09-24 修订
  时一并删掉——确认卡把判断权交回给用户，而命中恰恰发生在用户没意识到自己
  在选凭据的时候（理由详见 guard_comment.md 与 config_comment.md）。状态机
  因此少了一个态，也少了一条“等用户输入才继续”的分支。
- 为什么 `accept_input` 的返回仍是三态 `InputOutcome`：`Option` 只能表达
  “采纳 / 不采纳”，而壳对“被拦下”要做的事与“陈旧丢弃”不同（前者 warn
  且收起浮层，后者只记 debug）。合成一个 `None` 会让壳重新去读状态机才能
  分辨，那正是三态存在的理由。
- **热键路径**回 `Idle` 而不是停在 `Fetching`：这次取材已经结束（没有命令在
  途、没有令牌、没有待组装任务），停在被拦下的态会让同代数的迟到
  `InputReady` 仍可被采纳。`Idle` 同时表达了“这次触发什么也没留下”——浮层
  不露面、无失败卡、无取消令牌，`overlay_view` 也清空（里面可能还挂着上一
  次的产物卡，而它属于另一次取材）；浮层窗口那半边由壳隐藏（
  `dismiss_overlay`，见 overlay_comment.md）。
- **划词探测路径**（2026-10-01，见两段式条目）正相反：探测作废但当前显示
  **保留**——`commit_selection` 不触碰状态/视图/令牌，只消费探测编号。被拦
  的内容属于那次误触的手势，可见内容属于上一个会话，前者没有资格清后者。
  壳侧同样只记 warn，不调 `dismiss_overlay`。
- 为什么 `SensitiveKind` 仍是 `InputOutcome` 的载荷：它是壳唯一能写进日志的
  东西（类别，不是原文）。命中的原文连状态机都不留：不组装 `Task`、不进
  `active_task`，因此也没有任何结构会把它带进快照或日志。
- `retry` 现在只有它自己一条放行路径，`redispatch(from)` 因此收进 `retry`
  本体：共用的那半边不再有第二个调用者，留着参数只会让读者去找那个不存在的
  调用点。

## fail_acquire / fail_transport 的状态守卫（2026-09-26 评审整改）

- 两个 `fail_*` 原先只比代数，而 `accept_*` 家族是“代数 + 状态”双守卫：
  被内容闸门拦下的那次取材（状态已回 `Idle`、代数不变）若随后收到同代通道
  故障，会把一次已经结束的尝试推成 `Error` 并摆出失败卡。今天没有活的调用
  者（`send_run` 只在 Dispatch 之后调用），但守卫不该靠调用者自觉——补成
  “只在 `Fetching` / `Translating` 收”，与 `accept_*` 同一条口径。
- 判据：去掉守卫后，`blocked_input_is_not_redispatched_by_any_later_path`
  里那句“通道故障不摆失败卡”的断言立刻红（`left: Error, right: Idle`）。

## TaskStateMachine::trigger / task_options

- 为什么系统语言作为 `trigger` 的**第三个入参**而不是状态机的字段：状态机是 functional core（模块文档：只决策、不副作用、可全时序驱动），把“进程级环境事实”塞进字段就等于给它加了一份隐式状态，测试要读到何种系统语言就得先构造它。作为入参，每处调用都显式说明“本次决议用了哪份系统语言”，测试也天然可注入不同值。
- 为什么不是“壳先算好 locale 再传给 machine”：`task_options` 目前是 kind × config 的纯函数，locale 的落定规则（`Language` 三态 → 二态）属于“按配置解析选项”这件事的一部分。若壳先算，落定规则就跑到壳里，配置口径会被摊成两处。
- 代价：`trigger` 多一个参数，改动面覆盖全部测试调用点（一次性的机械改动）。换来的是“配置解析只有一个入口”这条不变量。

## OverlayView::Failed 的 FailureCause（任务 09，2026-09-22）

- 原形态是 `message: String`——失败卡文案在状态机里就拼成了中文（`error_message` 把 `GlossError` 变体映射成句子）。任务 09 要按 locale 出文案，成品字符串必须离开状态机：`Failed` 改为携带 `cause: FailureCause`（`Task(GlossError)` / `AcquireChannel` / `TransportChannel`），措辞交给 `ui::popup` 的渲染帧（见 popup_comment.md、i18n_comment.md）。
- 为什么用 `FailureCause` 而不是直接存 `GlossError`：失败卡有三种起因，其中两种（取材通道、推理通道）不是 `GlossError` 的变体——它们是**通道**故障。硬塞进 `GlossError` 会污染那个领域枚举（core 的 `GlossError` 描述的是“可归因于哪一步的外部失败”），而多一个包装枚举让三种起因平权。
- 为什么不是“状态机存 `GlossError` 的英文 `Display`、渲染时按它反查文案”：R7 卡明令禁止。`Display` 是日志措辞，改它不该改界面；反查还会让界面文案悄悄依赖日志文本。
- 代价：`FailureCause` 与 `GlossError` 的变体表两处维护（`error_text_maps_every_variant_per_locale` 逐变体钉住 UI 侧覆盖）；换来的是“状态机不持有任何用户可见文案”。

## TaskStateMachine::begin_pasteboard_probe / ProbeTask.source（2026-10-10，剪贴板图片方案 §5.6）

- 为什么两路探测共用**同一个** `probe` 槽而不是各开一个字段：探测的互斥语义（“探针期间其它触发照常短路”）本来就是“新探测顶掉旧探测、旧探测迟到产物经编号过滤丢弃”——两个独立字段要把这套互斥重写一遍还得处理跨字段顶替；单槽下互斥就是 `Option` 的替换，`ProbeSource` 字段只负责记录“这个槽在等哪种产物”。
- 为什么自身前台（`SelfSuppressed`）对剪贴板观察不生效而只走场景闸门：防误触判定的靶子是 HID 全局 tap 连拖 Gloss 自己浮层的那类手势误触；剪贴板哨兵读的是 `changeCount` 元数据，Gloss 自身从不写剪贴板（兜底读取只读不写），不存在“自己触发自己”的通路。闸门（安全输入态/敏感应用名单）两种触发同权生效。
- 为什么剪贴板图片的探测失败**没有静默名单**（`commit_probe_failed` 按 `ProbeSource` 分流，`PasteboardImage` 恒不静默）：划词的静默名单对应“误滑”——一个纯日常形态；剪贴板观察没有误滑形态（`changeCount` 前进且含图像类型才触发），能走到失败的只剩超上限、读取失败这类真实故障，按“显式反馈优先”全部落失败卡。①与②之间剪贴板被覆盖的竞态由事件线程（platform 取材侧）静默兜底、不产生④事件，状态机看不见它——静默口径与 §4 时序图一致。
- 为什么 `commit_selection` 改名 `commit_probe`（连带 `commit_selection_failed` → `commit_probe_failed`）：方案定的是“commit 扩展为接受 `TaskInput::Image`”——一个提交段按“探测的取材模态 × 产物变体”配对分流，而不是两条平行提交路径；改名后名字说的是“提交探测”，文本/图像都归它。模态错配不消费探测的既有语义不变（先配对再 `take`）。
- 为什么文本分支保留“先 `take` 探测再过内容闸门”的旧序（Blocked 时探测已消费）：`suspicious_input_is_dropped_while_the_visible_session_survives` 钉住的行为是“命中即探测作废”，消费点挪到配对通过之后只是把这一语义写得更直白，外部可观察行为不变。
- 为什么 `attached_image` 放在状态机字段而不是 `OverlayView` 变体上：方案红线“不改 `OverlayView` 变体形状”——图像只在图像任务存在，若进视图就得给 `Streaming`/`Outcome` 各加字段并在文本路径恒填 `None`，视图形状随渲染细节分叉；字段 + 访问器让“经位布局开关 = `attached_image.is_some()`”不依赖 classified 到达时序，重试路径（`active_request` 持同一 Arc）天然可用。清理点只有两处：新任务提交时置定、`hide_overlay` 清空——失败卡与重试期间图像保留，失败卡可继续借它交代上下文。
- 为什么图像任务的流式视图 `source` 置空串：经注疏协议里“经”位对文本任务是原文、对图像任务就是图本身；空串让 `accept_done` 把空的经位原文随行进产物卡，渲染层以 `attached_image` 的有无决定经位画什么，两条任务路径共享同一套视图形状。

## should_reveal / 露面（2026-10-10）

- 剪贴板路径不加新的露面来源：`auto_show_for(InputReady)` 恒 false 与“显形随提交置位”的划词语义对图像路径原样成立（壳层 `commit_probe` 置 `pending_reveal`），无锚点时 `show_position` 走既有居中分支——露面策略零改动是本步骤的设计检查点之一。

## error_action：JsonModeUnsupported → OpenSettings（2026-10-11，契约校验与重试方案 §2）

- 与 `UnsupportedModality` 同类：环境问题（端点不支持 JSON 输出模式），唯一出路是换模型服务，入口在设置页；重试按钮不出现——重发同一个请求只会再被拒一次。识别面（4xx + 诊断启发式）见 llm_comment.md。

## accept_repairing 与 Streaming.repairing（2026-10-12，契约校验与重试方案 §6）

- 为什么修复标记是“清 raw + 置 repairing”而不是整卡重建：上一轮的违约正文不该留驻 UI（垃圾正文无展示价值），但原文经位（source）与分类判定要随行保留——整卡重建得重新传一遍这两样，还丢掉“同会话连续”的语义；守卫与 `accept_chunk` 同款（代数 + Translating 态），陈旧修复事件不得清掉新会话的稿。
- 为什么 repairing 进 `OverlayView::Streaming` 而不是独立视图变体：方案定稿“不加横幅、不加计数”——UI 上的全部差异是页脚一个词（正在注解 ↔ 重新注解），独立变体会逼渲染层复制整条流式渲染路径，为一个文案不值得。
- 为什么 `retry()` 不感知 repairing：修复轮耗尽落的是降级产物卡（degraded Show 态），retry 的既有守卫“Show 态且 degraded”恰好覆盖；Translating 态中的修复轮不需要用户出口（取消浮层即取消）。

## TaskStateMachine 状态字段（接口契约）

- `next_id`：代数与探测编号共用的单调计数器：两路探测都从这里领号，永不复用。
- `generation`：当前已提交会话的代数：`accept_chunk`/`accept_done`/`accept_failed` 的陈旧过滤基准。探测编号在提交时才提升为代数。
- `probe`：在途的探测（划词或剪贴板图片，同一时刻至多一个）。新探测与收起都会替换或清掉它。
- `current_cancel`：在途推理的取消令牌：提交与收起时取消（唯一取消机制）。只在 `Translating` 存在——完成、失败与收起都清空，重试时换新令牌。
- `active_request`：当前任务的请求副本（input + options）：推理期间随行，可重试失败后留在 Error 态供 `retry` 原样重发；完成、隐藏与不可重试失败即清。
- `attached_image`：当前会话附带的图像（`TaskInput::Image` 的字节 Arc，提交时零拷贝留存）：经注疏卡“经”位的渲染源，随新任务置定、收起清空。不进视图变体——图像只在图像任务存在，视图形状不随它分叉。
- `overlay_view`：当前浮层的内容视图；`None` 时浮层显示渲染自检卡，即 `Idle`。四个稳定态都由它唯一决定——机器不另存状态字段，“状态↔视图”不存在两个可以分别赋值的真值源。

## 访问器：probe_id / state / attached_image / current_cancel

- `probe_id`：在途探测的编号（划词或剪贴板图片）；无探测时为 `None`。壳据此把通道④的回传分流到探测提交段（探测编号在提交时才提升为代数，不能按代数匹配）。
- `state`：当前稳定态：视图的投影。机器只存 `overlay_view`，`PendingConfirm ⟺ Pending`、`Translating ⟺ Streaming`、`Show ⟺ Outcome`、`Error ⟺ Failed`、`Idle ⟺ None` 因此是定义而非约定——不存在两个字段对不上的中间态。
- `attached_image`：渲染“经”位用；非图像任务或无会话时为 `None`。图像字节不进视图变体，渲染层经这里取用——经位布局开关就是它的有无，不依赖分类结果的到达时序。
- `current_cancel`：在途任务的取消令牌（测试观察口）。

## begin_selection_probe / begin_pasteboard_probe（探测段接口）

- 探测段只领探测编号、冻结任务选项并返回取材命令——**不改状态、不换视图、不取消在途任务**，已显示的内容与在途推理全程无感，误滑因此零干扰。产物到达走 `commit_probe`，失败走 `commit_probe_failed`；新探测替换旧探测（旧探测的迟到产物经编号过滤丢弃）。未接线或被闸门拦下的手势返回 `None` 且无副作用。
- `config` 是壳在事件起手处取的快照，`system_locale` 供 `Language::System` 落定，`scene` 供场景闸门判定。
- 剪贴板入口镜像划词入口；本应用兜底取材写下的剪贴板变化由平台侧按轮次计数归因吞掉（`gloss_platform::clipboard::WatchRoundFlag`）。

## commit_probe（提交段流程）

- 编号提升为代数，旧会话让位（取消在途推理、作废请求副本，迟到的旧产物经代数过滤丢弃），视图整卡换**待确认卡**并进 `PendingConfirm`（2026-10-27 前这里直接换流式视图并进 `Translating`；确认才进推理，见“三段式探测—确认—提交”节）。选项取探测时冻结的那份，不再读配置。
- 产物与探测模态先配对再消费：编号不符（陈旧）或模态错配都不消费探测，同编号的后续合法产物仍可提交。文本分支命中内容闸门时探测作废但**当前显示保留**（它属于上一个会话，误划与敏感内容都不该把它顶掉），且没有放行出口——防护不交由用户控制。图像分支没有可扫描文本，不设内容闸门；字节以 Arc 零拷贝留存进 `attached_image`，确认下发的流式视图原文为空（“经”是图本身）。

## confirm_pending（确认段，2026-10-27）

- 只认 `Pending` 视图：把待确认卡持有的 `input + options` 原样组装成
  `RunRequest`（同代数——提交时已提升），落流式视图并进 `Translating`；
  取消令牌由 `begin_run` 换新。其余态一律 `None`（守卫形状与 `retry`
  同款）。
- 确认不回读配置、不重算选项：卡上的快照就是全部事实源；确认本身不动
  代数、不消费探测（探测在提交时已消费）。

## commit_probe_failed（探测失败分流）

- 划词的空选区/读不到算误滑——静默丢弃，状态机与当前显示一律不动（壳只记一条带前台应用的排查日志）；剪贴板图片没有误滑形态，全部失败（超上限、读取失败等）都是真实故障，落失败卡。编号不符按陈旧丢弃。

## drop_probe

- 探测作废（取材通道发送失败等）：清掉编号，防止滞留的探测吞掉后续同形产物。当前显示与状态机不动。

## land_error（落错误态的共用半边）

- 取消在途、作废请求副本（重试只认 `active_request`）、按错误映射给动作出口并落失败卡。
- 只有“原样重发有意义”的失败才留请求副本；其余类别（含通道级故障的 fail_* 降级路径）一律清掉，retry() 自然无从发起。副本缺失时不给 Retry 出口——别摆一颗点了没反应的死按钮。

## accept_* 家族的采纳语义

- `accept_chunk`：追加到流式视图的原始回复（`note` 的渐进提取在渲染层），返回是否需要重绘。这份累积只服务**流式显示**；完成态以 `TaskDone` 的 `outcome.note` 为权威源，整卡覆盖（见 `crate::runtime::pipeline`）。
- `accept_done`：`outcome.note` 直接覆盖流式累积；原文从流式视图随行进产物卡，供注疏的“经”位对照。**降级产物**（契约校验失败）照常上卡，但**不清 `active_request`**——重试按钮据此原样重发，守卫见 `retry`。流已终结时只丢弃令牌句柄、不取消——任务已经正常完成，保持“Some ⟺ Translating”。
- `accept_classified`：把判定的任务类型写进流式视图（含分类失败时的兜底 kind，排版如实反映即将执行的任务）。陈旧代数或非流式视图不采纳——迟到标签不得落在已定格的产物卡上。
- `accept_failed`：落 `Error` 态并展示失败信息与动作出口（可重试类带重试按钮并保留请求副本，配置/鉴权类引导去设置页）。这是推理路径的失败；探测失败走 `commit_probe_failed`。其余状态不采纳——失败卡不得把已收起的浮层弹回。

## retry（重试的守卫与原件移交）

- 原样重发失败/降级的那个请求（同代数——旧任务的流已终结，不会有两路同代回传），浮层回到流式视图。守卫：`Error` 态，或 `Show` 态且**当前产物是降级产物**（正常产物没有重试出口）；无可重试请求时返回 `None`。
- 取走在途请求原件：`begin_run` 会把流式视图与请求副本一并重新记回，这里只需交出状态。

## begin_run（下发共用半边）

- 换新取消令牌、落定流式视图（确认下发与重试同形——原文取自本次下发的输入）并把请求副本记在途（失败可重试）。调用方只有 `confirm_pending`（确认段）与 `retry`（重试）两处。
- `CancellationToken` 是 Arc 共享句柄：跨线程取消与在途留存各持一枚，clone 只递引用计数，零堆分配——勿改成深拷贝或字段拆解。

## hide_overlay（收起即放弃）

- 浮层收起（Esc / 关闭按钮）即放弃在途任务：取消令牌（唯一取消机制）、清空视图与随行图像并回 `Idle`；在途探测一并作废（收起后迟到的产物不得把浮层弹回）。放弃后的迟到产物经代数或状态守卫丢弃——为一个不可见的浮层继续推理与渲染纯属空转；重新触发即重新取材（旧产物本就可能已过期）。
- 待确认卡的收起同一入口且**没有令牌可取消**（待确认阶段无在途推理，通道三零流量）——收起只是把卡丢掉，确认出口随视图清空一并失效（收起后的确认不能复活成下发）。

## fail_transport（通道降级）

- 推理通道不可用（③发送失败）时的降级：直接落 `Error` 态。通道已死时重发只会再死一次，失败卡不带重试按钮。

## 注释里的“M4-①”里程碑括注移除（PR #72 评审，2026-09-27）

`OverlayView::Acquiring` 变体文档与 `trigger` 里置骨架视图的注释曾以
“M4-①”括注指开发计划里的“触发即显”条目。dev-plan 不在仓内、README
的 M4 指另一里程碑，标记无法解析；行为描述（占代数的触发立刻给出“正在
读取选区”骨架，取材产物到达后整卡替换，被闸门拦下的触发不出浮层）保留。

> 2026-10-01 更新：骨架即显自本日起只属于热键触发（显式请求）；划词手势
> 改走探测—提交两段式，产物到达才出窗，不再产生 `Acquiring` 视图。
> 决策全文见 machine_comment.md 的两段式条目。

## accept_done 收降级不清 active_request（契约校验步骤 3，2026-10-11）

- 正常产物落 Show 即清 `active_request`（没有重试出口）；`degraded == true` 的产物**不清**——降级卡的“重试”按钮据此原样重发。Show 从此有两种形态：可重试的降级卡 / 不可重试的正常卡，差异只有这一行。
- `retry()` 守卫扩展为“Error 态，或 Show 态且当前产物 degraded”：判定直接读 `overlay_view` 里的 `outcome.degraded`，不另设状态位——视图与守卫同一事实源，不会漂移。
- 降级 Show 的 `current_cancel` 维持既有约定不动（已完成任务的令牌随 begin_run 换新；“失败任务的令牌 drop 而非 cancel”在 Error 重试路径就是这个形态）。

## 采纳门槛与归一化搬去渲染层（同日，方案 §1.8/§4）

- `plausible_code_language` 与 accept_done 里的语言采纳计算整体删除，收敛为 `ui::code_hl::adopt(verdict)`（渲染时现算，纯函数）：`OverlayView::Outcome`/`Streaming` 的 `code_lang` 字段一并删除——“模型说的”（产物 `structured.code_language`）与“UI 用的”（渲染现算）永不漂移，视图不再缓存派生状态。
- 流式视图因此**恒无角标**（原先字段在但恒 None，是死状态）：唯一语言来源是产物回传，kittest 流式基线随之少一行角标（popup_code_streaming 重录，见 popup/mod.rs 记录）。
- `code_language` 从 `TaskOutcome` 顶层移入 `OutcomeStructured::Code`：顶层字段暗示“所有 kind 都可能有语言判定”；只有代码任务有，字段就该住在那个变体里。

## 流式视图落定收进 begin_run（P1-1 clone 削减，2026-10-13）

- 首次下发（commit_probe）与重试（retry）的流式视图同形，原先各拼一次、
  原文各复制一次；现在 `begin_run` 统一从本次下发的 `input` 现拼
  （`streaming_view`），commit_probe 的 Text 分支只剩敏感内容闸门，原文
  复制从每次会话两次降为一次（视图源一份 + 在途副本一份）。
- `begin_run` 里 `(input.clone(), options.clone())` 保留：同一份原文需
  同时活在“流式视图源、在途副本（重试用）、通道载荷”三处，String 形
  制下两次复制是所有权下限；`options.clone()` 经 model 改 `Arc<str>` 已
  零堆分配。取消令牌的 `CancellationToken::clone` 是 Arc 句柄（零分配），
  已加旁注防误改。
- `accept_failed`/`commit_probe_failed` 维持 `&GlossError`：壳在状态机
  转移**之后**按处置结果（Shown/Ignored）定日志级别并记 `error` 字段，
  探测失败还要求静默路径（错误不入视图）事后可记——错误按值收进机器就
  没法再记日志。失败卡视图里的那份 `FailureCause::Task` 是必要的所有权
  副本。

## task_options 全量显式构造（同日补充）

- `..Default::default()` 的结构体更新语法会**先完整构造缺省 TaskOptions
  再覆盖**——缺省模型 id（`Arc::from("deepseek-flash")`）被分配出来立即
  丢弃，每次探测白付一次分配（Arc 头比短 String 堆块还大，字节数反而
  回退）。改为四字段全量显式构造：新增选项字段时此处编译报错强制补齐，
  不会静默漏填。
