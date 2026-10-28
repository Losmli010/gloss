# engine.rs 决策记录

> 按符号锚定（不写行号，防漂移）。改到相关符号时同步更新本文件。
> 本文件随仓库跟踪（docs/comments/ 入库），文件内不留指向本文件的指针。

## parse_structured

- 2026-09-19 为什么从私有改 pub：根包 benches/core.rs 要量化围栏解析热点，而 bench 目标是独立编译单元，只能访问 pub 项。备选里 feature 门控（bench 还要开特性，编译矩阵变复杂）与复制实现（两份逻辑漂移）都更贵，公开函数（文档注释已备）最便宜；现有 `#[cfg(test)]` 测试经 `super::` 调用，不受影响。

## AiTaskService::execute

- 2026-09-22 缓存命中与未命中各留一条日志（命中 `info`、未命中 `debug`）：模板语言参与 `cache_key` 是本任务（R1）的验收点，而「同词换模板语言后应重新请求」在真机上只能靠**有没有真的发起网络调用**判断。此前只有 platform 侧的 `llm stream established`（debug 级）能间接推断，命中路径在日志里完全无痕，用户据此判定「切换没生效」。埋点放在 `execute` 而不是壳侧：只有这里能准确区分缓存分支与网络分支（壳侧看到的 `TaskDone` 两者一样）。core 并非无日志（`config_handle.rs` 的 `error!` 在先），此行不为 core 开新边界。

## engine.rs 决策记录

### execute 收窄为「渲染 → 转发 → 返回」（M1，2026-09-26）

- 备选一（采用）：execute 只渲染并逐 delta 原样转发，返回 `Result<(), GlossError>`；
  body 累积、产物组装、缓存查/写全部上移到 app 桥（pipeline.rs）。core 的
  职责面与 dev-plan 的模块职责总表对齐：core 不碰 markdown 拼接与缓存编排。
- 备选二：execute 保留缓存参数但去掉组装。否——缓存编排留在 core 会让
  M2 的「分类缓存 → 分类 → 主缓存」三段次序没有落点（次序决策属于编排，
  不属于渲染服务）；且服务签名还得为分类缓存再改一次。
- 备选三：execute 返回累积后的原始 body。否——桥在 on_chunk 回调里本来
  就要累积（TaskChunk 转发与累积天然同点），返回值再给一份是重复通道。

### finalize_outcome 独立成契约纯函数（M1，2026-09-26）

剥围栏 + parse_structured + 构造 TaskOutcome 三步合成一个入口，桥的完成态
代码只有一行组装；引擎失败到不了这里（桥先 `?` 返回），因此「产物恒可写
缓存」成为本函数的前置语义而非内部判断。parse_structured 保持 pub：M3 的
gloss-eval 与生产同源复用。

### execute 不打日志（M1，2026-09-26）

缓存命中/未命中、流收尾的日志都在桥上（span 在桥进入，代数天然齐全）；
core 保持无日志面，避免同一事实在两层各记一条。

### resolve_api_key 不做按代缓存（2026-09-30，块 D 评估后跳过）

- 设想（ux-fixes-plan 块 D 的 P2 项）：按配置代缓存密钥解析结果，省掉
  classify + 主请求各一次的同步 keychain 直查。评估后不做：缓存需要一个
  **失效信号**，而密钥的写入路径绕过配置句柄（设置页直接写 store 的
  keychain 半边，不进快照也不动 ConfigHandle）——按配置内容或句柄代数
  建缓存，用户换 key 后旧值仍命中，EngineAuth 会一直持续到重启或改无关
  配置。要正确的失效就得给 ConfigStore 加密钥侧代数（trait 变更扩散到
  全部实现与桩），复杂度与收益不成比例：keychain 直查对本机钥匙串是
  毫秒级，不在首 token 预算的主因里。
- 若未来要做：先给 ConfigStore 的密钥半边加代数/epoch（写入即递增），
  缓存按 (keychain_id, epoch) 键控。

### 分类兜底痕迹断言的窗口内有限重试（2026-10-08，PR #133 CI 修复）

- `classify_failure_falls_back_and_the_task_still_runs` 断言在最多 3 个全新
  capture 窗口内重试：CI 实测偶发一次「行为断言全过、痕迹行未被窗口收到」——
  log.rs capture 文档记录的残留竞争（进程级兴趣缓存/MAX_LEVEL 在窗口外无订阅
  者语境被改写后，窗口内调用点被宏级短路）叠加本测试独有放大面（窗口内含
  tokio 运行时构建 + block_on，窗口宽度远超其余 capture 测试的纳秒级单宏）。
  每轮窗口起点的兴趣缓存重建即修复窗口外污染（never → always、MAX=OFF →
  INFO 都落在重建范围内）；每轮全新 MockEngine，行为断言逐轮执行，隐私断言
  命中即失败——语义无放宽。本机 x86_64 32 轮未复现，按 capture 契约处置。
- 不换 capture_global：它与 init_is_idempotent 争进程级全局订阅者槽位且槽位
  永久（capture_global 静默忽略 set 失败拿空缓冲；init 的 .init() 遇已占用
  panic）——先跑者通吃、后跑者必炸，libtest 并行下顺序不可控。窗口重试没有
  这个顺序耦合。测试内原有两行注释按注释纪律迁入本记录。

### AiTaskService::run 的图像放行（2026-10-10，剪贴板图片方案 §5.4）

- 为什么图像输入跳过 LLM 分类：取材动作即显式意图——剪贴板图片任务就是图像解读，不存在划词那种「不知道用户要干什么」的判定问题。恒定映射 `ImageExplain` 与分类常量兜底同哲学（显式意图不交给模型猜）。`CLASSIFY_KINDS` 不动（分类清单仍只服务文本路径），引擎调用从 2 次降为 1 次，图像任务省一次付费往返。
- 为什么 `on_classified` 仍要先于任何 chunk 恰好一次：桥与 UI 的协议不变（任务标签以该回调为准），图像路径只是「已定好的 kind」也走同一回调，调用方零特判。
- 为什么 Audio 依旧入口即拒：语音是预留模态，取材端口未落地，任何 kind 都不放行（模态矩阵与入口双重收口）。

### RunOutput.finish_reason（2026-10-11，契约校验与重试方案 §5）

- 本步只采集不消费：`length`（截断）/ `content_filter` 的分支归修复轮步骤——先把事实带上来，消费策略（修复指令精确提示 / 直接失败卡）是下一步的决策，现在写死会把两步耦死。
- 服务端未声明 `[DONE]`（连接关闭、测试桩）为 None：与 SseStream 的产出契约一致。

### run() 任务契约修复轮（2026-10-12，契约校验与重试方案 §5/§6）

- 为什么修复轮在编排层（`AiTaskService::run`）而不是桥或状态机：修复是「同一请求代数内的重试」，事件序（`TaskChunk* → TaskRepairing → TaskChunk*`）必须经通道④保序送达主线程——放桥上方（状态机重发）会把一次任务拆成多个 `RunTask`，代数、取消与缓存语义全要重推；放 run future 内则取消经 select! 竞速天然覆盖，零新增机制。
- 为什么耗尽后返回 `Ok`（违约原文原样上交）而不是 `Err`：降级是「有痕迹的兜底产物」，走 `finalize::complete` 的既有两分支（校验不过 → `degraded` 卡 + 重试出口 + 不进缓存）；若改 `Err` 就会落到失败卡，丢掉「输出异常」与失败卡的语义区分，且半截流式的渲染收敛路径也要重推。
- 为什么零增量「empty completion」按固定诊断字符串识别（`llm::EMPTY_COMPLETION` 单点常量）而不引入新 `GlossError` 变体：方案定稿「不改 GlossError 变体」——该诊断只在传输适配器一处产出、run 循环一处消费，常量锁死两处措辞；不扩大错误面（新变体会牵动 i18n、失败卡映射与所有 match 臂）。同请求异常的区分靠「body 为空 + 精确措辞」双条件：已有增量后的中途协议错误、零增量的鉴权/限流都不命中，照旧上抛。
- 为什么 `finish_reason == content_filter` 在契约校验之前判定：内容被服务端拦截时重发与修复都无益，先判可少一次校验且语义清晰（「没产出可修的东西」）；`length` 则只作为修复指令的附加提示（「重试只打契约失败」，§1.2）——截断但 JSON 完整合法的回复照常采纳，避免把重试率推高。
- 为什么 `RunOutput.repairs` 只计任务段：分类修复在 `classify()` 内部消化、发生在 `on_classified` 之前，对调用方不可见也无 UI 语义；评测统计（方案步骤 7）关注的是任务产物的修复成本，分类预算是 `CLASSIFY_REPAIRS` 的 TTFT 问题。
- 为什么 run 新增第三个回调 `on_repair` 而不复用 `on_classified`：修复重置只清流式正文并切页脚文案，不改变任务类型——语义不同的回调分开，桥侧一行一个事件，状态机侧守卫独立（`accept_repairing` 同 `accept_chunk` 守卫）。

### 任务段循环抽出 run_task_rounds（2026-10-12，契约校验与重试方案步骤 7）

- 为什么把 run() 的任务段循环（渲染底座之后的流式转发 → 契约校验 → 修复轮）抽成 pub 自由函数：评测 live 轨需要以数据集给定的 kind 直达任务段（分类轨已由 `classify` 单独度量，混进任务轨会把两次模型调用与判型噪声搅进任务指标），而任务段的修复形态必须与生产同一份——抽函数让 `AiTaskService::run` 与 gloss-eval 消费同一循环，`RunOutput.repairs` 对评测才是真数字。备选「eval 自抄一份循环」被否：预算、EmptyReply 转换、content_filter/length 分支任何一处漂移，评测就在静默地度量另一个产品。
- 为什么以参数传 `PromptRegistry`/`locale`/`base` 而不是做成 `AiTaskService` 的第二个方法：评测侧没有 service 可依附（它自组底座），自由函数 + 显式参数让「生产 run = 分类 + 本函数」这个等式在签名上可见；参数虽多（九个），但个个独立缺一不可，捆参数结构只是把装拆包成本转嫁给两个调用方（clippy too_many_arguments 显式豁免，与 gloss-app 事件装配同理由）。
- `locale` 由 run 从 `options.prompt_locale` 解出后传入：修复指令语言与任务书同源（一次任务只认触发时定下的那一份），函数内不得再取缺省——这是抽取时最容易引入的静默行为漂移。

### run 不再复制 model id（P1-1 clone 削减，2026-10-13）

- 原 `AiTaskService::run` 在 options 被 move 进 `Task` 前先
  `options.model.clone()` 存局部；现在模型 id 借自本函数存活到结尾的
  `task.options.model`，`run_task_rounds` 收 `&str`，零复制。
- `run_task_rounds` 的 `base.clone()` 保留：修复轮要以首轮底座重拼对话，
  而引擎端口的 `BoxFuture<'static>` 要求请求按值持有消息——base 必须有
  一份在请求之外存活。每次 run 恰一份复制，已无更便宜的形。

### MAX_REPLY_BYTES(2026-10-16,内存优化方案)

- 为什么超限按 Err 失败而不是截断续跑:超 1MB 的回复不可能过裸 JSON 契约,截断只会把必违约的原文送进修复轮,把同样的失控输出再跑满 TASK_REPAIRS 轮(每轮还各累积一份);Err 走既有 TaskFailed 路径,UI 侧流式 raw 与桥侧 body 的量级被同一预算钉死(UI raw 只收转发的增量,总量 ≤ 预算)。

### TASK_REPAIRS

- 任务契约违约的修复轮预算：任务共 `1 + TASK_REPAIRS` 次尝试；耗尽后原始文本原样上交，由调用方落降级产物。

### MAX_REPLY_BYTES（常量注释迁入）

- 单轮流式回复的累积字节上限：卡片正文远用不到这个量级，而无上限的累积会把「模型失控输出」放大成无界内存（流式期间逐 chunk 追加）。超限按请求异常失败——半截超长回复不可能过 JSON 契约，进修复轮只会重跑同样的输出。

### FINISH_CONTENT_FILTER / CONTENT_FILTER_DIAGNOSTIC（常量注释迁入）

- `FINISH_CONTENT_FILTER`：服务端以内容审查收尾的 `finish_reason`。
- `CONTENT_FILTER_DIAGNOSTIC`：内容审查的失败诊断——内容被服务端拦截，重发同请求无益，不重试。

### RunOutput（源码注释迁入）

- 一次 `run` 的产出：判定的任务类型 + 引擎返回的**原始**完整文本，解析成产物归调用方。
- `kind`：本次任务最终执行的类型（含分类兜底）。
- `raw`：模型回复的原始完整文本（未解析、未剥离）。修复轮耗尽后是最后一轮的违约原文——调用方据此落降级产物。
- `finish_reason`：服务端声明的结束原因（`choices[0].finish_reason`）；服务端没给 `[DONE]`（连接关闭、脚本桩）为 `None`。以最后一轮为准。
- `repairs`：契约违约触发的修复轮次数（0 = 一次通过）。

### AiTaskService（源码注释迁入）

- 渲染、分类与转发编排：只依赖端口与模板。`new` 组装：引擎由入口注入。

### AiTaskService::run（源码注释迁入）

- 执行一条任务：入口校验（模型非空、模态放行）→ 意图定型 → prompt 渲染 → `run_task_rounds` 执行任务段。
- `input` 与 `options` 按值接收：任务在执行期间独占两者，调用方从通道解出后即交出所有权。
- 调用方契约：`on_classified` 在**任何** chunk 之前恰好调用一次（含兜底 kind 与图像的固定 kind）——界面的任务标签以它为准；`on_chunk` / `on_repair` 与返回值的契约见 `run_task_rounds`；取消不进本服务：调用方以 `CancellationToken` 竞速，丢弃本 future 即停止转发（修复轮也在 future 内，竞速天然覆盖）。
- 模型随 options 冻结而来：空白 = 未配置（引擎侧同规则兜一层），在花任何一次往返之前拒绝。
- 意图定型：文本输入没有显式意图，交给 LLM 分类（分类恒 Ok，这里的 Err 臂只是防御）；图像输入的取材动作即显式意图——剪贴板图片就是图像解读，固定映射并跳过分类，省一次往返。
- 任务形成与执行：构造即过模态约束（非法组合在进引擎前拒绝），模型恒取 options 冻结值（借自本函数存活的 task，不再复制）；修复指令语言同源——prompt_locale 一次任务只认触发时定下的那一份。
- too_many_arguments 豁免的缘由：三个回调是执行期注入的泵，泛型各不相同，捆进结构体只是把类型参数挪个位置；与 `TaskStage` 那类描述「执行什么」的静态输入不同形，不捆。

### RoundReply / collect_round（源码注释迁入）

- `RoundReply`：一轮回复的传输结果——累积正文、服务端结束原因与「零增量完成」标记。`empty_completion`：传输层以「成功收尾但一个增量都没有」终结，坏回复而非坏请求。
- `collect_round`：消费一轮流，增量经 `on_chunk` 逐条转交并累积，终结帧只取原因。超预算与中途协议错误按请求异常上抛（已转发的增量不撤回）。
- 超限处理：已转发的增量不撤回（调用方契约）；超限的失败以返回值为准丢弃半截正文。
- 零增量的「empty completion」是坏回复而非坏请求：转 EmptyReply 进修复轮；其余引擎错误（含已有增量后的中途协议错误）按请求异常上抛，不重试。

### TaskStage（源码注释迁入）

- 任务段的静态执行输入（意图定型之后的一切）：引擎与模板注册表、判定的 kind、首轮渲染底座、冻结的模型 id 与纠错指令语言。逐轮转交的两个回调不在其中——它们是泵，见 `run_task_rounds` 的调用方契约。

### run_task_rounds（源码注释迁入，调用方契约）

- 任务段执行循环（意图定型之后的完整执行段）：流式转发（原始增量经 `on_chunk` 逐条转交，同时累积）→ **任务契约校验**（`schema::validate_task`）→ 违约进修复轮（对话形态 `[system][user 原文/图][assistant 上一次坏回复][user 纠错指令]`，EmptyReply 原样重发），预算 `TASK_REPAIRS` 轮，每轮开始前经 `on_repair` 通知调用方重置 UI → 通过即返回，耗尽则原始文本原样上交（完成态解析落降级产物）。引擎错误（网络/鉴权/限流/JSON 模式不支持/多模态不支持/中途协议错误）一律不重试、原样上抛。
- 独立成 pub 供 gloss-eval 的 live 轨直达任务段：评测以数据集给定的 kind 渲染底座（`PromptRegistry::render` 的产物），与生产消费同一循环，评测数字度量的才是生产行为（`RunOutput.repairs` 即本函数的修复轮次）。
- 调用方契约（`AiTaskService::run` 与评测共用）：`on_chunk` 收到的是**原始流**（纯 JSON 契约下就是模型的原始输出），流式显示的渐进提取与完成态解析都在调用方；修复轮的增量接在违约轮之后继续到达（`on_repair` 是唯一的轮边界信号）；返回值 `raw` 是最后一轮文本的整体累积。`on_repair` 在每个修复轮开始前恰好调用一次（违约轮的 chunk 已全部送达之后）——调用方重置流式显示并切换「重新注解」。返回 `Err` 时**已转发的增量不撤回**——以返回值为准丢弃半截正文（状态机据此让 TaskChunk 之后接 TaskFailed 的渲染路径可收敛）；`Err` 只来自引擎、请求构造与流式失控（超 `MAX_REPLY_BYTES` 的回复按请求异常失败，不重试），契约违约不产生 `Err`。
- base 是首轮消息；修复轮以它为底座拼 [assistant 坏回复][user 纠错指令]（图像部件随 base 零拷贝重发）。
- 内容审查：服务端拦了内容，重发无益——不重试直接失败卡。
- 预算耗尽：违约原文原样上交，完成态解析落降级产物。
- 修复轮照常流式，开始前发事件重置 UI（清流式正文、切「重新注解」）。

### 语言体例门阶段（2026-10-28，输出语言体例门禁设计 §6）

- 挂点在 `run_task_rounds` 的契约收敛臂：格式契约通过后进 `run_lang_rounds`（`LangStage` 静态输入 + 两个泵回调，与 `TaskStage` 同形），任务段修复轮计数原样透传（`RunOutput.repairs` 语义不变，语言修复另算）。预算 `LANG_REPAIRS = 1`，与任务段 3 轮互不挤占（设计「独立一轮」）。
- `RunOutput` 新增 `lang_degraded`：格式降级与语言降级在调用方合流（桥侧把两者并进 `TaskOutcome.degraded`），core 层保持「格式违约 = 交付原文」与「语言违约 = 交付合格原文 + 标志」两条可区分的路径。
- 修复轮采纳守卫：语言修复轮的回复只有再次通过格式契约才替换当前回复——修复轮退化成散文时交付上一份契约合格回复并置 `lang_degraded`（宁交付可解析的违约产物，不交付不可解析文本；跨段预算纠缠由此避免）。该分支首轮不可达（进入即契约合格），防御臂按耗尽同型处理。
- 语言段的 `finish_reason == content_filter` 与任务段同判同败（服务端拦截重发无益）；`empty_completion` 不采纳（保持旧回复），引擎错误照旧上抛。
- `lang_violations` 用 `Option` 而非 `expect`：工作区 lint 禁 `expect_used`，且循环不变量「reply 恒契约合格」由采纳守卫维持，`None` 臂按耗尽降级处理是安全的兜底而非吞错（留痕：语言修复轮的 violated/exhausted warn 已在 RepairLoop 打过）。
- 为什么语言修复指令由 `PromptRegistry::render_language_repair` 渲染：违约行清单是判定器的产物，模板只给骨架（§7.6），与格式修复的 `render_repair` 对称。

### 评审修复（2026-10-28，PR #259 评审 P2-7）

- `lang_violations` 的防御 `None` 臂补 `warn`（「language gate saw a contract-invalid reply, delivering degraded」）：采纳守卫使该臂不可达，但「降级必有痕」的形态统一要求它真被触发时留痕（对照 `finalize.rs` 降级纪律）。
