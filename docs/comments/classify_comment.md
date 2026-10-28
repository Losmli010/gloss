# classify 决策记录（M2，2026-09-26）

## Auto 哨兵的活动范围

- validate_modality 放行 Auto+Text（运输合法性：哨兵以文本进编排），
  PromptRegistry::render 显式拒绝 Auto（ClassifyRequired）——模态表管
  「能不能运输」，渲染收口管「能不能渲染」，两层各司其职。
- Auto 不进 ALL_KINDS：设置任务开关清单天然不含它，settings 侧无需过滤
  代码；config 测试钉住这一点。
- 划词手势在 trigger_decision 固定 Acquire(Auto) 且不查任务开关：手势
  不带显式意图，开关管的是显式 kind（热键）；场景闸门照常生效。

## 分类模型与重建的取数点

- 分类模型 = resolved_model(default_text_kind)，由桥在分类时刻从**单次**
  配置快照解析（default_text_kind 误配图像 kind 时经 classify_fallback
  收口到 TranslateWord）；Auto 任务本身的 options.model_override 恒 None
  ——哨兵不执行，解析了也是死值。
- 重建任务在桥内完成（不回主线程）：TaskClassified 只服务界面标签；
  重建的 model_override = 快照 resolved_model(判定 kind)，此后执行段
  冻结不变。备选「机器侧 accept_classified 返回新 RunRequest 重派」被
  否：多一跳主线程往返，且机器不持配置、解析点还得挪。

## 分类失败的回退

- classify() 只报错不回退；回退是桥的编排决策（warn 无内容 + fallback
  kind + TaskClassified 照发）。备选「classify 内部吞错返回兜底」被否：
  纯函数不该埋编排策略，且测试面无法区分「分类成功」与「静默回退」。
- 分类缓存只存 LLM 判定成功的 kind；回退不写缓存——下次同文本可能因
  设置修复而分类成功。
- 缓存命中后再过一次 allowed 校验：设置变更（kind 停用）后旧判定不作数，
  当轮直接落兜底，不污染缓存。
- classify_key 不含 allowed 清单：清单变化靠命中后校验兜住，避免设置
  改动击穿全部缓存。

## 分类回复解析

- 裸 JSON 先行、围栏提取兜底（```gloss 与 ```json 都收）：契约请模型裸
  输出，围栏是不守约时的容错位。kind 标识必须能 serde 反序列化且在
  allowed 内，否则 EngineResponse（固定措辞，不携带回复原文）。
- max_tokens=128 + 回复累积 4KiB 双界：正确回复只有一个小 JSON，截断只
  落在跑偏回复上，解析失败自然走回退。

## 流式循环的增量解析提前退出（2026-09-30）

- 起因：分类路径严格串行在首 token 关键路径上（取材 → 分类 → 主请求），
  原实现等流自然结束才解析——裸 JSON 的闭合括号到达后，剩余的空转增量
  与结束帧把 TTFT 白白拖长一个 RTT 量级。改为每次累积后即尝试
  `parse_classify_reply`，首个能通过校验的完整 JSON 立即定型返回。
- 为什么只在 Ok 时退出：部分 JSON（截断、逐 chunk 到达中途）解析必然
  失败，继续等是唯一正确的动作；跑偏的长篇要么解析不过走兜底、要么在
  完整给出合法 kind 的那一刻就定型——两条路都不需要「流结束」这个信号。
  Err 仍立即上抛，语义不变（Err 不保证终结流，见 ports::TaskStream）。
- 成本：每次累积后的全量 parse 上界是回复上限（4KB）× 个位数 chunk，
  微秒级；相对 chunk 的毫秒级到达间隔与 LLM 的秒级 TTFT 可忽略。
- 提前返回即提前 drop 流：SSE 连接就此关闭，服务端看到客户端断开——
  这是特性不是副作用（省下的就是这段传输）。

## 越清单夹具 ImageOcr → ImageExplain（2026-10-10，契约校验与重试方案步骤 1a）

- `replies_outside_the_allowed_list_are_rejected` 的清单外 kind 夹具原用 `ImageOcr`（变体已删，字符串仍会被拒绝但不再对应任何域内值）；改用 `ImageExplain`——它仍是合法 serde 名、但不在 `CLASSIFY_KINDS`，拒绝路径走的是「反序列化成功 → 允许清单拦截」这一支，比未知标识（死在反序列化）多钉一层。eval 侧 `classify_verdicts_cover_the_matrix` 同改。

## classify（TaskChunk::Done 臂，2026-10-11，契约校验与重试方案 §5）

- 流条目升级后分类多一个 `Done` 臂、显式忽略：分类的判定要么已随首个完整 JSON 提前退出，要么落到循环外的整段解析，结束帧没有增量价值；`finish_reason` 是任务段（修复轮）的消费需求，分类路径不读。

## General 不在分类清单（2026-10-12，契约校验与重试方案步骤 4）

- `CLASSIFY_KINDS` 保持三个文本 kind、`CLASSIFY_FALLBACK` 保持 TranslateWord（换值归步骤 5）；本步只在常量测试里钉住「General ∉ 清单」。schema 侧 `validate_classify` 对 `{"kind":"General"}` 落 `KindNotAllowed`（合法 serde 名但清单外）——与 ImageExplain 同一支：反序列化成功后由允许清单拦截，比未知标识多钉一层。

## classify() 内部消化违约与请求异常（2026-10-12，契约校验与重试方案 §2/§6）

- 为什么校验从 `parse_classify_reply`（围栏容错）换成 `schema::validate_classify`（严格裸 JSON）：修复轮需要 `Violation` 固定措辞构造纠错指令，`parse_classify_reply` 的 `EngineResponse("unrecognized classify reply")` 分不出空回复/非 JSON/越清单；方案 §3 明确「围栏 = 违约进修复轮」，且生产与评测共用同一校验器是定稿决策。`parse_classify_reply` 保留给 gloss-eval（重放镜像），待步骤 6/7 随围栏容错一起删除。
- 为什么 `CLASSIFY_FALLBACK` 从 TranslateWord 改为 General：修复轮上线后「分类失败」的含义从「没判出来」升级为「判了两次仍不守约」——继续猜一个高频意图（词卡）的错律高于给一个通用落点；General 是步骤 4 专门铺的清单外执行 kind，天然是兜底位。用户无感（不出提示），warn 只记类别 + 轮次。
- 为什么分类阶段的请求异常也内部消化（引擎错误不重试、直接落兜底）：分类在 TTFT 之前，多一次重试就是首字延迟直接劣化；且任务段马上还会再打一次请求，网络/鉴权错误在任务段会以失败卡浮出，分类阶段不重复报障——这与步骤 5 之前 engine.rs 兜底 Err 臂的行为一致，只是收拢点从编排层移进分类自身（engine.rs 的 Err 臂保留为非文本输入的防御路径）。
- 为什么空回复的修复是原样重发（不带 assistant 轮、不带纠错指令）：无内容可纠，纠错指令没有指向对象；部分端点拒收空 assistant content；首轮请求本身已含完整契约，原样重发即给模型第二次机会。

## 围栏容错删除，严格裸 JSON 全面生效（2026-10-12，契约校验与重试方案步骤 6 / §10）

- 上一节预告的「`parse_classify_reply` 待步骤 6/7 随围栏容错一起删除」落地为**半删**：`fenced_json` 提取与 `STRUCTURED_FENCE` 常量整体删除（两处消费者——完成态围栏 fallback 归步骤 3、分类容错提取归本步——都没了），但 `parse_classify_reply` 本身保留，重写为 `schema::validate_classify` 的严格裸 JSON 薄封装（只做 `Violation` → `EngineResponse` 的错误形态折算）。为什么保留：它仍是 gloss-eval 分类轨的唯一生产校验入口（`metrics.rs` 的 `judge_classify_reply`），现在删掉它就得当场重接 eval——那是步骤 7「eval 同源校验器」的地盘；薄封装让评测在这两步之间继续度量生产行为，且校验判线单一（都出自 schema.rs）。
- 为什么围栏容错现在可以删而早先不行：模板与提示已不再要求围栏（步骤 3 全模板嵌套化 + 「不要围栏」字样），且修复轮已上线——围栏输出有了明确出口（`NotJson` → 修复轮纠正），容错提取只会把违约静默放行、剥夺模型的纠错机会；分类路径同理（围栏 → 修复轮 → 仍违约落 General，`a_fenced_reply_repairs_then_falls_back` 已在步骤 5 钉住）。
- 「三个 kind」措辞改「清单 3 个 + 兜底 General」（`CLASSIFY_KINDS`/`CLASSIFY_FALLBACK` 文档）：步骤 4 之后 General 落地，只说「三个 kind」会让人误读成文本路径的全部答案空间；「清单 3 个 + 兜底 General」把「分类器只在清单内选择」与「清单外唯一落点是兜底」一句话说全。同步 AGENTS.md 架构速览为「提示直通 → LLM 分类（1 次修复）→ 常量兜底」（方案 §10 明文条目）。
- `replies_outside_the_allowed_list_are_rejected` 的围栏断言翻转（原「围栏内合法 kind 放行」→「围栏即拒绝」）：这是旧容错的最后一处行为断言，翻转后与 `a_fenced_reply_repairs_then_falls_back`（编排级）、schema.rs 违约矩阵（校验器级）三层同向。
- eval 侧只动注释不动行为：`is_json_like`/`first_fenced_json` 原文档自称「镜像生产 parse_classify_reply 的裸先行、首围栏兜底提取」，生产删容错后该说法失真，注释改为「刻意比生产宽一档、只为把『非 JSON 形态』与『形态像但被拒』分列统计」。is_json_like 的围栏分支本身保留——那是指标分列语义不是容错，eval 的整体重接线（含 lib.rs 头注释里过时的「finalize 围栏 fallback」表述）归步骤 7。

## classify_traced 与 ClassifyDecision（2026-10-12，契约校验与重试方案步骤 7）

- 为什么抽 `classify_traced`（返回 `ClassifyDecision { kind, repairs, rounds }`）而 `classify` 变薄封装：评测 live 轨需要逐轮回复（折算违约类别）与修复轮数，生产编排只消费 kind——两个消费面共用一份循环，差别只在「请求异常消化与否」：`classify` 保持内部消化（落兜底 + warn，用户无感的既有语义零改动），`classify_traced` 刻意上抛——eval 要把引擎失败单列 `engine_errors`，消化了就分不出来。消化职责放外层而非 traced 层，warn 文案与调用点都留在生产路径上。
- 为什么 `rounds` 记完整回复而不是增量：分类的轮回复上界 4KiB、轮数 ≤ 2，全量携带零压力；而评测折算（逐轮 `validate_classify`）需要的恰是每轮的完整文本——增量切分对分类轨没有任何消费者（重放只拼接），不为形态保留无人读取的细节。

## 修复轮不再全程持有首轮底座（P1-1 clone 削减，2026-10-13）

- 原 `classify_traced` 每轮 `messages.clone()` 进请求、`classify_round`
  内部再 `request.clone()` 过引擎端口，加上 `base.clone()` 与
  `reply.clone()` 共四份深拷贝。改为：请求按值构造逐轮 move 进引擎；
  `classify_round` 按值收请求；回复校验后原样 push 进 `rounds`（修复轮
  的 assistant 历史借它拼好再 move）；底座在修复分支按同一注册表、同一
  输入重渲（`render_classify` 纯函数，逐字节同形），不再全程持有首轮
  副本。
- 「重渲代替持有」的等价性依据：`render_classify(locale, allowed, text)`
  是同参纯函数，产物与首轮逐字节一致，MockEngine 录制的请求比较不受
  影响；代价是修复轮（预算 1 轮、罕见路径）多一次模板渲染，常规路径
  （一次通过）零拷贝。

## 首条目截止 3s（2026-10-16）

- 生产日志：分类请求失败（empty completion 等）要等满服务端 TTFT 才落兜
  底，实测 2.7–11.4s，且分类串行在主任务之前——失败现场把开工拖后数秒。
  建连后 3s 无任何流条目即按请求异常收卷（warn 留痕），兜底 General 立即
  开工。
- **明示接受**：先思考后出字（reasoning 类）的端点若 3s 内无 content 增
  量，分类恒落兜底——标签精度换开工时延；warn 里的 deadline 痕迹可定位。
  截止只武装到首个条目：交付后节奏归还传输层空闲看门狗（60s），长流无虞。

## 分类类别集收进模板（2026-10-20，PR #218）

- 上文多处「允许清单 / `CLASSIFY_KINDS`」的机制描述自本日起失效：重写后
  的 classify 模板自带四类别（TranslateWord / TranslateSentence /
  ExplainCode / Fallback），`validate_classify` 不再收调用方清单，按
  模板类别硬校验；`CLASSIFY_KINDS` 常量与 allowed/rules 渲染块删除。
- `Fallback` 是模型可选类别，在 `validate_classify` 映射
  `TaskKind::General`——与常量兜底 `CLASSIFY_FALLBACK` 同一落点（通用
  任务模板）；`General` 标识永不进分类模板（分类模板说 Fallback，代码
  说 General，映射在 schema 层）。定型 info 行同行携带 `model_kind`（模
  型原始类别）与 `kind`（最终执行 kind）。
- `KindNotAllowed` 固定措辞随之去「清单」化：`"reply kind is not one of
  the classify categories"`（reason 回灌进 repair 模板与日志）。

## 零增量空完成按空回复进修复轮（2026-10-21，同 PR #218 后续）

- 任务段 `collect_round` 早有定论：零增量的 EmptyCompletion 是坏回复而非
  坏请求（转 EmptyReply 重发一次）；分类段此前把它按请求异常直接落兜底，
  两段口径不一致。生产日志（2026-10-20/21）里分类失败恰全是这一形态——
  每次都无重试直落兜底，「空回复重发」在分类段实际不可达。
- 收敛后分类段同口径：`classify_round` 对 `EmptyCompletion` 且累积为空时
  断流收尾，交整段校验得 EmptyReply 违约走既有修复轮（原样重发，预算仍
  `CLASSIFY_REPAIRS=1`）；实现上不需要镜像任务段的 `empty_completion`
  标记——空回复本就会校验出 EmptyReply，收敛点放在「错误转断流」一处。
- 代价（明示接受）：该失败形态的最坏 TTFT 从 1 次请求变 2 次；仅发生在
  端点异常收尾时，正常路径零开销。

## CLASSIFY_FALLBACK

- 分类失败（请求异常、修复轮后仍违约）时的兜底 kind：常量而非配置——兜底必须是可渲染、可执行的具体文本 kind。classify 模板的 `Fallback` 类别在校验层映射到同一个 kind（`crate::schema::validate_classify`）：「模板四类别 → 三个文本 kind + 兜底 General」构成文本路径的完整落点。

## CLASSIFY_REPAIRS

- 分类契约违约的修复轮预算：分类在 TTFT 之前，预算敏感——共 `1 + CLASSIFY_REPAIRS` 次尝试。

## CLASSIFY_MAX_TOKENS

- 分类回复的 token 上限（OpenAI 兼容 `max_tokens`）：正确回复是一个只含 kind 标识的小 JSON，截断只落在模型跑偏的长篇上——跑偏的回复解析不过，进修复轮。

## CLASSIFY_REPLY_CAP_BYTES

- 回复累积上限（字节）：`max_tokens` 之外的第二道界，防止不守约的端点把无界回复灌进内存。超限部分丢弃（按字符边界截断）——截断的 JSON 解析不过，同样进修复轮。pub 供 gloss-eval 的 live 轨复用同一道界（评测与生产同源）。

## push_capped

- 按预算累积回复：单条增量超预算时按字符边界截断。分类编排与评测 live 轨共用。

## FIRST_ITEM_DEADLINE

- 分类首条目截止（墙钟）：建连成功后这么久没有任何流条目（增量或终结帧）即判传输停摆，按请求异常收卷。分类在 TTFT 之前，等一个不出字的服务端会把主任务的开工拖在同一处；兜底 kind 恒可执行，判定损失只是标签精度。

## FirstItemDeadline

- 首条目截止的流包装：截止只武装到首个条目交付，之后的读取节奏归还传输层空闲看门狗——分类回复极小，不存在需要看门狗的长流。定时器惰性创建：内层流 Pending 时才需要它，即时产出的流（脚本桩）不碰时间驱动。

## classify

- 判定一条文本输入的任务类型（编排入口）。契约违约走修复轮（预算 `CLASSIFY_REPAIRS`），请求异常不重试；两者最终都落 `CLASSIFY_FALLBACK`——文本输入恒 `Ok`。
- 非 `Text` 输入直接拒：分类只服务划词路径；图像任务带着具体 kind 进编排，到不了这里。
- 分类阶段的请求异常不重试（重发同请求无益），内部消化落兜底——warn 只记错误类别，不含选区原文与模型回复。

## ClassifyDecision

- 一次分类的完整轨迹：判定的任务类型、契约修复轮次数与每一轮的完整回复（末轮是完成态判定的输入）。生产编排只消费 `kind`；评测（gloss-eval live 轨）据 `rounds`/`repairs` 把违约类别与修复轮次计入统计——两侧消费同一份轨迹，评测数字度量的才是生产行为。
- `kind`：判定的任务类型（含兜底 `CLASSIFY_FALLBACK`）。
- `repairs`：契约违约触发的修复轮次数（0 = 一次通过）。
- `rounds`：每一轮的完整回复，按轮序（修复轮由违约触发，逐轮跑 `crate::schema::validate_classify` 即还原违约轨迹）。

## classify_traced

- `classify` 的全轨迹形态：逐轮执行分类请求并记录回复，契约违约走修复轮（对话形态 `[system][user 原文][assistant 上一次坏回复][user 纠错指令]`，EmptyReply 原样重发不带 assistant 轮），预算 `CLASSIFY_REPAIRS` 耗尽落 `CLASSIFY_FALLBACK`（恒 `Ok`）。
- 与 `classify` 在错误处理上的差别只有一处：**请求异常不消化**、原样上抛——消化（落兜底 + warn）归 `classify`；评测需要区分「请求失败」与「回复违约」，故在 traced 层保持上抛。
- `text` 是待分类的选区文本。判定定型时把模型输出的原始类别与最终执行的 kind 同行落 info（兜底类别 `Fallback` 映射 `TaskKind::General` 的落点在此可见）。pub 供 gloss-eval 的 live 轨复用同一循环（评测与生产同源）。
- 底座只渲染一次：修复轮以它为底拼对话（与任务段同一形态），不再每轮重渲。
- messages 随请求 move 出去（engine 端口按值收请求）；修复轮由底座重拼。
- 流内已提前定型就不用对末轮缓冲再校验一遍；流自然结束时才整段校验一次（违约类别是修复轮与统计的输入）。

## settled_decision

- 定型收卷：原始类别与最终 kind 同行落 info（兜底映射的落点在此可见），并产出全轨迹决策。

## ClassifyRound

- 一轮分类的传输结果：累积回复与流内提前定型的判定。
- `settled`：完整 JSON 在流结束前到达时非 `None`——调用方无需对末轮缓冲再校验。

## classify_round

- 执行一轮分类请求并按预算累积回复：流式增量逐条累积，首个能通过校验的完整 JSON 立即提前收卷（不等流自然结束，提前还流即还回 TTFT）；只在 Ok 时提前退出——部分 JSON 继续等，跑偏与截断的回复照旧走完流、落到整段校验。每次累积后的全量校验上界是回复上限（4KB）× 个位数 chunk，相对 chunk 的毫秒级到达间隔可忽略。结束帧对分类没有增量价值。
- 建连后的首条目等待受 `FIRST_ITEM_DEADLINE` 约束（见模块文档）；零增量的空完成（`EmptyCompletion`）断流收尾、按空回复交整段校验（与任务段同口径，见模块文档），其余流错误按请求异常上抛。请求按值接收（引擎端口按值收请求，消息所有权随调用 move）。

## parse_classify_reply

- 解析模型回复：严格裸 JSON，无围栏容错——围栏输出即违约（与生产编排的 `crate::schema::validate_classify` 同一条判线，本函数只把 `Violation` 折算成 `GlossError::EngineResponse`，错误文本是固定措辞，不携带回复原文）。
- pub 供 gloss-eval 评测重放复用：评测与生产走同一个校验器。

## 修复调用点保持违约级行（期三，2026-10-29）

- 分类修复轮的纠错指令传无细节（render_repair 第三参为空）——分类段行是
  违约级措辞行（方案 §6 决策 3），原始回复在场也不做细节提取，细节通道
  只服务任务段。
